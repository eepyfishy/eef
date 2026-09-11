//! Read-only, bounded normalization of existing registered model metadata.
//! This is not a backend scan, resource estimate, execution grant or load command.
use anyhow::{Result, bail};
use eefn::network::{PeerModel, PeerRecord, validate_node_id};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

const BUDGET: usize = 512 * 1024;
fn default_limit() -> usize {
    8
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryQuery {
    pub node_id: Option<String>,
    pub capability: Option<String>,
    pub after: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
impl InventoryQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=32).contains(&self.limit) {
            bail!("inventory node-page limit must be 1-32")
        }
        if self.node_id.is_some() && self.after.is_some() {
            bail!("node filter cannot be combined with a cursor")
        }
        for id in self.node_id.iter().chain(self.after.iter()) {
            validate_node_id(id)?;
        }
        if self.capability.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 64
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        }) {
            bail!("capability must be a bounded capability identifier")
        }
        Ok(())
    }
}

fn model(node_id: &str, entry: &PeerModel) -> Value {
    let metadata = entry.normalized_metadata();
    json!({"instance":{"node_id":node_id,"backend":entry.backend,"model_id":entry.model_id},
        "capabilities":metadata.capabilities,"input_modalities":metadata.input_modalities,"output_modalities":metadata.output_modalities,
        "legacy_modality":entry.modality,"metadata_source":if entry.model_metadata.is_some(){"model_metadata_v1"}else{"legacy_registration"},
        "roles":metadata.roles,"lifecycle":metadata.lifecycle,"availability":metadata.availability,"resource_estimates":metadata.resource_estimates})
}

pub async fn list(
    server: &eefn::NodeServer,
    freshness_seconds: u64,
    query: InventoryQuery,
) -> Result<Value> {
    query.validate()?;
    let connected = server.connected_nodes().await;
    if query
        .node_id
        .as_ref()
        .is_some_and(|id| !connected.contains_key(id))
    {
        bail!("node is not connected")
    }
    let ids = connected
        .keys()
        .filter(|id| {
            query.node_id.as_ref().is_none_or(|wanted| wanted == *id)
                && query.after.as_ref().is_none_or(|after| *id > after)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    let selected = ids.into_iter().take(query.limit + 1).collect();
    let records = server.peer_records(&selected).await?;
    if query.node_id.is_some() && records.is_empty() {
        bail!("node registration is not available")
    }
    page(records, freshness_seconds, &query)
}

fn page(
    records: Vec<(PeerRecord, Duration)>,
    freshness_seconds: u64,
    query: &InventoryQuery,
) -> Result<Value> {
    let mut nodes: Vec<Value> = Vec::new();
    let mut bytes = 0;
    let mut next_after = None;
    for (record, age) in records {
        let mut identities = BTreeSet::new();
        for entry in &record.models {
            if !identities.insert((&entry.backend, &entry.model_id)) {
                bail!("node registration contains duplicate backend/model identities")
            }
        }
        let mut entries = record
            .models
            .iter()
            .map(|entry| model(&record.node_id, entry))
            .filter(|entry| {
                query.capability.as_ref().is_none_or(|cap| {
                    entry["capabilities"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|v| v == cap))
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|v| {
            (
                v["instance"]["backend"].as_str().unwrap_or("").to_owned(),
                v["instance"]["model_id"].as_str().unwrap_or("").to_owned(),
            )
        });
        let node = json!({"node_id":record.node_id,"registration_state":if age<Duration::from_secs(freshness_seconds){"fresh"}else{"stale"},
            "age_ms":age.as_millis().min(u64::MAX as u128) as u64,"models":entries});
        let size = serde_json::to_vec(&node)?.len();
        if nodes.len() >= query.limit || bytes + size > BUDGET - 2048 {
            if nodes.is_empty() {
                bail!("model inventory node exceeds response budget")
            }
            next_after = nodes
                .last()
                .and_then(|v| v["node_id"].as_str())
                .map(str::to_owned);
            break;
        }
        bytes += size;
        nodes.push(node);
    }
    Ok(
        json!({"schema_version":1,"success":true,"nodes":nodes,"next_after":next_after,
        "freshness_seconds":freshness_seconds,"scope":"connected_node_registrations","direct_access_authorized":false,
        "note":"Advertised models only, not all installed files. Versioned metadata or legacy modality hints are node-reported, not verified functionality; unreported state/resources remain unknown. No download, load or inference was performed."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_metadata_reaches_inventory_without_legacy_overrides() {
        let mut r = record("a");
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(None);
        metadata.capabilities = vec!["ocr".into(), "embedding".into()];
        metadata.roles = Some(vec!["search".into()]);
        metadata.lifecycle = Some(eefn::model_metadata::ModelLifecycle::Ready);
        metadata.resource_estimates.ram_mb = Some(512);
        r.models[0].model_metadata = Some(metadata);
        let mut q = query();
        q.capability = Some("ocr".into());
        let result = page(vec![(r, Duration::ZERO)], 30, &q).unwrap();
        let models = result["nodes"][0]["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["metadata_source"], "model_metadata_v1");
        assert_eq!(models[0]["lifecycle"], "ready");
        assert_eq!(models[0]["resource_estimates"]["ram_mb"], 512);
        assert!(models[0]["resource_estimates"]["vram_mb"].is_null());
        assert_eq!(models[0]["roles"], json!(["search"]));
        assert_eq!(models[0]["capabilities"], json!(["ocr", "embedding"]));
    }
    fn query() -> InventoryQuery {
        serde_json::from_value(json!({})).unwrap()
    }
    fn record(id: &str) -> PeerRecord {
        PeerRecord::from_registration(&json!({"node_id":id,"name":"PRIVATE-NAME","network":{"advertised_address":"192.0.2.1"},"models":[
        {"model_id":"same","backend":"ollama","modality":"text","path":"PRIVATE-PATH","secret":"PRIVATE-SECRET"},
        {"model_id":"same","backend":"llamacpp","modality":"vlm"}]})).unwrap()
    }
    #[test]
    fn legacy_models_are_normalized_without_inventing_resources_or_leaking_fields() {
        let result = page(vec![(record("a"), Duration::ZERO)], 30, &query()).unwrap();
        assert!(!result.to_string().contains("PRIVATE"));
        assert!(!result.to_string().contains("192.0.2.1"));
        let models = result["nodes"][0]["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_ne!(models[0]["instance"], models[1]["instance"]);
        for model in models {
            assert!(model["lifecycle"].is_null());
            assert!(model["resource_estimates"]["ram_mb"].is_null());
        }
        assert_eq!(
            models[0]["capabilities"],
            json!(["llm.infer", "vlm.analyze"])
        );
    }
    #[test]
    fn filtering_pagination_staleness_and_empty_nodes_are_explicit() {
        let mut q = query();
        q.limit = 1;
        q.capability = Some("vlm.analyze".into());
        let result = page(
            vec![
                (record("a"), Duration::from_secs(31)),
                (record("b"), Duration::ZERO),
            ],
            30,
            &q,
        )
        .unwrap();
        assert_eq!(result["next_after"], "a");
        assert_eq!(result["nodes"][0]["registration_state"], "stale");
        assert_eq!(result["nodes"][0]["models"].as_array().unwrap().len(), 1);
        q.capability = Some("audio.stt".into());
        assert_eq!(
            page(vec![(record("a"), Duration::ZERO)], 30, &q).unwrap()["nodes"][0]["models"],
            json!([])
        );
        let unknown = PeerModel {
            model_id: "unknown".into(),
            backend: None,
            modality: None,
            model_metadata: None,
        };
        assert_eq!(model("a", &unknown)["capabilities"], json!([]));
    }
    #[test]
    fn queries_and_large_inventory_are_bounded() {
        for invalid in [
            json!({"limit":0}),
            json!({"limit":33}),
            json!({"node_id":"a","after":"b"}),
            json!({"capability":"bad name"}),
        ] {
            assert!(
                serde_json::from_value::<InventoryQuery>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        assert!(serde_json::from_value::<InventoryQuery>(json!({"psk":"forbidden"})).is_err());
        let mut records = vec![];
        for i in 0..32 {
            let mut r = record(&format!("node-{i:02}"));
            r.models = (0..64)
                .map(|j| PeerModel {
                    model_id: format!("{j:03}{}", "x".repeat(250)),
                    backend: Some("b".repeat(64)),
                    modality: Some("vlm".into()),
                    model_metadata: None,
                })
                .collect();
            records.push((r, Duration::ZERO));
        }
        let mut q = query();
        q.limit = 32;
        let result = page(records, 30, &q).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() < BUDGET);
        assert!(result["next_after"].is_string());
    }

    #[test]
    fn duplicate_instance_is_rejected_instead_of_silently_selecting_metadata() {
        let mut r = record("a");
        r.models.push(r.models[0].clone());
        assert!(page(vec![(r, Duration::ZERO)], 30, &query()).is_err());
    }
}
