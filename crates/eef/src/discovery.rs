//! Coordinator-owned disclosure policy. Discovery never grants execution access.
use anyhow::{Result, bail};
use eefn::network::{PeerQuery, PeerRecord, validate_node_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const RESPONSE_BUDGET: usize = 512 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
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
                    modality: Some("v".repeat(64))
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
