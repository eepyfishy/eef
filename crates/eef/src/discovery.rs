//! Coordinator-owned disclosure policy. Discovery never grants execution access.
use anyhow::{Result, bail};
use eefn::network::{PeerQuery, PeerRecord, validate_node_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const RESPONSE_BUDGET: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscoveryCommand {
    /// Show saved and currently applied disclosure policy (no secrets).
    Show,
    /// Allow one requester to inspect one target, after EEF restart.
    Grant {
        #[arg(long)]
        requester: String,
        #[arg(long)]
        target: String,
    },
    /// Remove one disclosure grant, after EEF restart. Self visibility remains.
    Revoke {
        #[arg(long)]
        requester: String,
        #[arg(long)]
        target: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: DiscoveryCommand,
}

/// Core command implementation shared by command adapters. No browser state.
pub fn command(
    config: &crate::config::Config,
    applied: &DiscoveryPolicy,
    request: &DiscoveryCommand,
) -> Result<Value> {
    config.edit_for_restart(|value| {
        let before = DiscoveryPolicy::parse(value.get("discovery"))?;
        let mut saved = before.clone();
        match request {
            DiscoveryCommand::Show => {},
            DiscoveryCommand::Grant { requester, target } | DiscoveryCommand::Revoke { requester, target } => {
                for id in [requester, target] {
                    validate_node_id(id)?;
                    if id == "*" { bail!("discovery grants require exact node IDs") }
                }
                if requester == target { bail!("self visibility is implicit and cannot be granted or revoked") }
                if matches!(request, DiscoveryCommand::Grant { .. }) {
                    saved.grants.entry(requester.clone()).or_default().insert(target.clone());
                } else if let Some(targets) = saved.grants.get_mut(requester) {
                    targets.remove(target);
                    if targets.is_empty() { saved.grants.remove(requester); }
                }
                saved.validate()?;
            }
        }
        let changed = saved != before;
        if changed { value["discovery"] = serde_json::to_value(&saved)?; }
        Ok((json!({"schema_version":1,"success":true,"changed":changed,
            "saved_policy":saved,"applied_policy":applied,"restart_required":saved != *applied,
            "direct_access_authorized":false,
            "note":"Directional disclosure only. Saved grants and revocations apply after EEF restart; self visibility remains."}), changed))
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoveryPolicy {
    /// Exact authenticated requester -> exact permitted target IDs. No wildcard.
    pub grants: BTreeMap<String, BTreeSet<String>>,
    pub freshness_seconds: u64,
}
impl Default for DiscoveryPolicy {
    fn default() -> Self {
        Self {
            grants: BTreeMap::new(),
            freshness_seconds: 30,
        }
    }
}
impl DiscoveryPolicy {
    pub fn parse(value: Option<&Value>) -> Result<Self> {
        let policy: Self = serde_json::from_value(value.cloned().unwrap_or_else(|| json!({})))?;
        policy.validate()?;
        Ok(policy)
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=300).contains(&self.freshness_seconds)
            || self.grants.len() > 256
            || self.grants.values().map(BTreeSet::len).sum::<usize>() > 4096
            || serde_json::to_vec(self)?.len() > 256 * 1024
        {
            bail!("discovery policy exceeds supported bounds")
        }
        for (requester, targets) in &self.grants {
            validate_node_id(requester)?;
            if requester == "*" || targets.len() > 256 {
                bail!("discovery grants require exact IDs and at most 256 targets per requester")
            }
            for target in targets {
                validate_node_id(target)?;
                if target == "*" {
                    bail!("wildcard discovery grants are not supported")
                }
            }
        }
        Ok(())
    }
    fn selection(&self, origin: &str, query: &PeerQuery) -> Result<BTreeSet<String>> {
        validate_node_id(origin)?;
        query.validate()?;
        let mut permitted = self.grants.get(origin).cloned().unwrap_or_default();
        permitted.insert(origin.into());
        if let Some(target) = &query.node_id {
            if !permitted.contains(target) {
                bail!("node unavailable or not authorized")
            }
            permitted.retain(|id| id == target);
        }
        if let Some(after) = &query.after {
            permitted.retain(|id| id > after);
        }
        Ok(permitted)
    }
    pub async fn view(
        &self,
        origin: &str,
        payload: Value,
        server: &eefn::NodeServer,
    ) -> Result<Value> {
        let query: PeerQuery = serde_json::from_value(payload)?;
        let selected = self.selection(origin, &query)?;
        let records = server.peer_records(&selected).await?;
        if query.node_id.is_some() && records.is_empty() {
            bail!("node unavailable or not authorized")
        }
        self.page(&query, records)
    }
    fn page(
        &self,
        query: &PeerQuery,
        records: Vec<(PeerRecord, std::time::Duration)>,
    ) -> Result<Value> {
        let mut nodes = Vec::new();
        let mut bytes = 0;
        let mut next_after = None;
        for (record, age) in records {
            let node = self.public_record(record, age);
            let size = serde_json::to_vec(&node)?.len();
            if nodes.len() >= query.limit || bytes + size > RESPONSE_BUDGET - 2048 {
                if nodes.is_empty() {
                    bail!("node discovery record exceeds response budget")
                }
                next_after = nodes
                    .last()
                    .and_then(|v: &Value| v["node_id"].as_str())
                    .map(str::to_owned);
                break;
            }
            bytes += size;
            nodes.push(node);
        }
        Ok(
            json!({"schema_version":1,"success":true,"nodes":nodes,"next_after":next_after,
            "freshness_seconds":self.freshness_seconds,"direct_access_authorized":false,
            "note":"Owner-authorized registration view, not connectivity proof or permission to execute"}),
        )
    }
    fn public_record(&self, record: PeerRecord, age: std::time::Duration) -> Value {
        let fresh = age < std::time::Duration::from_secs(self.freshness_seconds);
        let mut node = json!(record);
        node["status"] = json!(if fresh { "online" } else { "stale" });
        node["age_ms"] = json!(age.as_millis().min(u64::MAX as u128) as u64);
        node["remaining_fresh_ms"] = json!(
            (self.freshness_seconds * 1000)
                .saturating_sub(age.as_millis().min(u64::MAX as u128) as u64)
        );
        if !fresh {
            // Do not give stale endpoints/models the appearance of usability.
            node["network"] = json!({"advertised_address":null,"coordinator":null});
            node["capabilities"] = json!([]);
            node["models"] = json!([]);
        }
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn saved_config() -> (tempfile::TempDir, crate::config::Config) {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        let mut value = config.as_value();
        value["node"]["psk"] = json!("PRIVATE-TEST-SECRET");
        value["custom"] = json!({"preserve":true});
        config.save_for_restart(&value).unwrap();
        (dir, config)
    }
    #[test]
    fn commands_preserve_saved_settings_and_distinguish_applied_policy() {
        let (_dir, config) = saved_config();
        let applied = DiscoveryPolicy::default();
        let grant = DiscoveryCommand::Grant {
            requester: "a".into(),
            target: "b".into(),
        };
        let added = command(&config, &applied, &grant).unwrap();
        assert_eq!(added["saved_policy"]["grants"]["a"], json!(["b"]));
        assert_eq!(added["applied_policy"]["grants"], json!({}));
        assert_eq!(added["restart_required"], true);
        assert!(!added.to_string().contains("PRIVATE"));
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        assert_eq!(
            command(&config, &applied, &grant).unwrap()["changed"],
            false
        );
        command(&config, &applied, &DiscoveryCommand::Show).unwrap();
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        let latest = crate::config::Config::load(config.source()).unwrap();
        assert_eq!(latest.string("node.psk", ""), "PRIVATE-TEST-SECRET");
        assert_eq!(latest.get("custom.preserve"), Some(&json!(true)));
        let running = DiscoveryPolicy::parse(latest.get("discovery")).unwrap();
        let revoked = command(
            &config,
            &running,
            &DiscoveryCommand::Revoke {
                requester: "a".into(),
                target: "b".into(),
            },
        )
        .unwrap();
        assert_eq!(revoked["saved_policy"]["grants"], json!({}));
        assert_eq!(revoked["applied_policy"]["grants"]["a"], json!(["b"]));
        assert_eq!(revoked["restart_required"], true);
        assert_eq!(revoked["direct_access_authorized"], false);
    }
    #[test]
    fn commands_reject_invalid_input_without_overwriting_config() {
        let (_dir, config) = saved_config();
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        for (requester, target) in [("a", "*"), ("*", "b"), ("a", "a"), ("", "b")] {
            for request in [
                DiscoveryCommand::Grant {
                    requester: requester.into(),
                    target: target.into(),
                },
                DiscoveryCommand::Revoke {
                    requester: requester.into(),
                    target: target.into(),
                },
            ] {
                assert!(command(&config, &DiscoveryPolicy::default(), &request).is_err());
                assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
            }
        }
        assert!(
            serde_json::from_value::<DiscoveryCommand>(
                json!({"operation":"grant","requester":"a","target":"b","allow_execution":true})
            )
            .is_err()
        );
        std::fs::write(config.source().unwrap(), "[broken").unwrap();
        assert!(
            command(
                &config,
                &DiscoveryPolicy::default(),
                &DiscoveryCommand::Show
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(config.source().unwrap()).unwrap(),
            "[broken"
        );
    }
    #[test]
    fn concurrent_commands_merge_saved_grants_and_respect_limits() {
        let (_dir, config) = saved_config();
        std::thread::scope(|scope| {
            for i in 0..12 {
                let config = config.clone();
                scope.spawn(move || {
                    command(
                        &config,
                        &DiscoveryPolicy::default(),
                        &DiscoveryCommand::Grant {
                            requester: "a".into(),
                            target: format!("node-{i}"),
                        },
                    )
                    .unwrap()
                });
            }
        });
        let value = command(
            &config,
            &DiscoveryPolicy::default(),
            &DiscoveryCommand::Show,
        )
        .unwrap();
        assert_eq!(
            value["saved_policy"]["grants"]["a"]
                .as_array()
                .unwrap()
                .len(),
            12
        );
        let mut saved = crate::config::Config::load(config.source())
            .unwrap()
            .as_value();
        saved["discovery"]["grants"]["a"] =
            json!((0..256).map(|i| format!("node-{i}")).collect::<Vec<_>>());
        config.save_for_restart(&saved).unwrap();
        let before = std::fs::read(config.source().unwrap()).unwrap();
        assert!(
            command(
                &config,
                &DiscoveryPolicy::default(),
                &DiscoveryCommand::Grant {
                    requester: "a".into(),
                    target: "overflow".into()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), before);
    }
    fn record(id: &str) -> PeerRecord {
        PeerRecord::from_registration(&json!({"node_id":id,"name":"Laptop","network":{"advertised_address":"26.1.2.3","coordinator":{"coordinator_id":"eef-a","address":"26.1.2.3:51335","state":"standby"}},"models":[{"model_id":"model-a","backend":"ollama","modality":"text","password":"PRIVATE"}],"metadata":{"resources":[{"parameters":{"password":"PRIVATE"}}]},"source_ip":"PRIVATE","psk":"PRIVATE"})).unwrap()
    }
    #[test]
    fn default_self_only_exact_nontransitive_grants_and_query_validation() {
        let policy = DiscoveryPolicy::default();
        assert_eq!(
            policy.selection("a", &PeerQuery::default()).unwrap(),
            BTreeSet::from(["a".into()])
        );
        let policy =
            DiscoveryPolicy::parse(Some(&json!({"grants":{"a":["b"],"b":["c"]}}))).unwrap();
        assert_eq!(
            policy.selection("a", &PeerQuery::default()).unwrap(),
            BTreeSet::from(["a".into(), "b".into()])
        );
        assert!(
            policy
                .selection(
                    "a",
                    &PeerQuery {
                        node_id: Some("c".into()),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert!(
            policy
                .selection(
                    "a",
                    &PeerQuery {
                        limit: 33,
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert!(DiscoveryPolicy::parse(Some(&json!({"grants":{"a":["*"]}}))).is_err());
        assert!(DiscoveryPolicy::parse(Some(&json!({"freshness_seconds":0}))).is_err());
        assert!(DiscoveryPolicy::parse(Some(&json!({"unknown":true}))).is_err());
    }
    #[test]
    fn freshness_redacts_stale_endpoints_and_payloads_are_sanitized() {
        let policy = DiscoveryPolicy::default();
        let fresh = policy.public_record(record("a"), std::time::Duration::from_secs(1));
        assert_eq!(fresh["network"]["advertised_address"], "26.1.2.3");
        assert!(!fresh.to_string().contains("PRIVATE"));
        let stale = policy.public_record(record("a"), std::time::Duration::from_secs(30));
        assert_eq!(stale["status"], "stale");
        assert!(stale["network"]["coordinator"].is_null());
        assert_eq!(stale["models"], json!([]));
        assert_eq!(stale["remaining_fresh_ms"], 0);
    }
    #[test]
    fn pages_are_bounded_and_cursor_rechecks_current_grants() {
        let policy = DiscoveryPolicy::parse(Some(&json!({"grants":{"a":["b","c"]}}))).unwrap();
        let page = policy
            .page(
                &PeerQuery {
                    limit: 1,
                    ..Default::default()
                },
                vec![
                    (record("a"), std::time::Duration::ZERO),
                    (record("b"), std::time::Duration::ZERO),
                ],
            )
            .unwrap();
        assert_eq!(page["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(page["next_after"], "a");
        let query = PeerQuery {
            after: Some("a".into()),
            ..Default::default()
        };
        assert_eq!(
            DiscoveryPolicy::default()
                .selection("a", &query)
                .unwrap()
                .len(),
            0
        );
        assert!(!page["direct_access_authorized"].as_bool().unwrap());
    }

    #[test]
    fn response_byte_budget_does_not_expose_oversized_pages() {
        let mut records = Vec::new();
        for i in 0..32 {
            let mut r = record(&format!("node-{i:02}"));
            r.capabilities = vec!["x".repeat(128); 128];
            r.models = vec![
                eefn::network::PeerModel {
                    model_id: "m".repeat(256),
                    backend: Some("b".repeat(64)),
                    modality: Some("v".repeat(64)),
                    model_metadata: None,
                };
                64
            ];
            records.push((r, std::time::Duration::ZERO));
        }
        let page = DiscoveryPolicy::default()
            .page(&PeerQuery::default(), records)
            .unwrap();
        assert!(page["nodes"].as_array().unwrap().len() < 32);
        assert!(page["next_after"].is_string());
        assert!(serde_json::to_vec(&page).unwrap().len() < RESPONSE_BUDGET);
    }
}
