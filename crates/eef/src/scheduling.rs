//! Local-owner non-model scheduling policy. This never grants execution access.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SchedulingPolicy {
    pub freshness_seconds: u64,
    /// Zero removes this additional coordinator-wide ceiling.
    pub max_in_flight: u32,
    pub per_provider_concurrency: u32,
    /// Exact node ID -> aggregate ceiling across its non-model providers.
    pub node_limits: BTreeMap<String, u32>,
}
impl Default for SchedulingPolicy {
    fn default() -> Self {
        Self {
            freshness_seconds: 10,
            max_in_flight: 0,
            per_provider_concurrency: 0,
            node_limits: BTreeMap::new(),
        }
    }
}
impl SchedulingPolicy {
    pub fn parse(value: Option<&Value>) -> Result<Self> {
        let policy: Self = serde_json::from_value(value.cloned().unwrap_or_else(|| json!({})))
            .map_err(|_| anyhow::anyhow!("invalid scheduling policy"))?;
        policy.validate()?;
        Ok(policy)
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=300).contains(&self.freshness_seconds)
            || self.max_in_flight > 4096
            || self.per_provider_concurrency > 4096
            || self.node_limits.len() > 256
        {
            bail!("scheduling policy exceeds supported bounds")
        }
        for (node, limit) in &self.node_limits {
            eefn::network::validate_node_id(node)?;
            if node == "*" || *limit > 4096 {
                bail!("node scheduling limits require exact IDs and a limit of 0-4096")
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchedulingCommand {
    /// Inspect saved/applied policy and coordinator-local in-flight reservations.
    Show {},
    /// Save limits for explicit eef restart; zero means no additional ceiling.
    Limits {
        #[arg(long)]
        freshness_seconds: Option<u64>,
        #[arg(long)]
        max_in_flight: Option<u32>,
        #[arg(long)]
        per_provider_concurrency: Option<u32>,
    },
    /// Save an aggregate non-model concurrency ceiling for one exact node.
    Node {
        #[arg(long)]
        node: String,
        #[arg(long)]
        max_in_flight: u32,
    },
    /// Remove the exact node override; global/provider ceilings still apply.
    RemoveNode {
        #[arg(long)]
        node: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulingRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: SchedulingCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    success: bool,
    changed: bool,
    saved_policy: SchedulingPolicy,
    applied_policy: SchedulingPolicy,
    restart_required: bool,
    execution_authorized: bool,
    dispatched: bool,
    scope: String,
    note: String,
    runtime_id: String,
    admission: Admission,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    total_in_flight: usize,
    retired_in_flight: usize,
    node_counts: Vec<NodeCount>,
    nodes_truncated: bool,
    providers: Vec<ProviderCount>,
    providers_truncated: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeCount {
    node_id: String,
    in_flight: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderCount {
    node_id: String,
    capability: String,
    action: String,
    in_flight: u32,
    effective_per_provider_concurrency: u32,
}

/// CLI confirmation checks identity, safety flags and the requested policy delta.
/// A lost or inconsistent reply is unknown, never permission to replay mutations.
pub fn confirmed_report(
    value: &Value,
    runtime_id: Option<&str>,
    request: &SchedulingCommand,
) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if report.schema_version != 1
        || report.report_type != "coordinator_scheduling"
        || !report.success
        || report.execution_authorized
        || report.dispatched
        || report.scope != "coordinator_local_non_model_reservations"
        || report.note.len() > 2048
        || report.runtime_id.is_empty()
        || report.runtime_id.len() > 128
        || runtime_id.is_some_and(|id| id != report.runtime_id)
        || report.saved_policy.validate().is_err()
        || report.applied_policy.validate().is_err()
        || json!(report.saved_policy) != value["saved_policy"]
        || json!(report.applied_policy) != value["applied_policy"]
        || report.restart_required != (report.saved_policy != report.applied_policy)
    {
        return false;
    }
    let admission = &report.admission;
    if admission.node_counts.len() > 256
        || admission.providers.len() > 512
        || admission.retired_in_flight > admission.total_in_flight
    {
        return false;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for node in &admission.node_counts {
        if eefn::network::validate_node_id(&node.node_id).is_err()
            || node.node_id == "*"
            || !seen.insert(&node.node_id)
        {
            return false;
        }
        let Some(next) = total.checked_add(node.in_flight) else {
            return false;
        };
        total = next;
    }
    if total > admission.total_in_flight
        || (!admission.nodes_truncated && total != admission.total_in_flight)
    {
        return false;
    }
    let mut keys = std::collections::BTreeSet::new();
    for provider in &admission.providers {
        if eefn::network::validate_node_id(&provider.node_id).is_err()
            || provider.node_id == "*"
            || provider.capability.is_empty()
            || provider.capability.len() > 64
            || provider.action.is_empty()
            || provider.action.len() > 64
            || matches!(provider.capability.as_str(), "llm.infer" | "vlm.analyze")
            || !keys.insert((&provider.node_id, &provider.capability, &provider.action))
        {
            return false;
        }
        // Counters may include older external accounting and executor limits;
        // they are observational, not proof that a remote side effect stopped.
        let _ = (
            provider.in_flight,
            provider.effective_per_provider_concurrency,
        );
    }
    let _ = admission.providers_truncated;
    match request {
        SchedulingCommand::Show {} => !report.changed,
        SchedulingCommand::Limits {
            freshness_seconds,
            max_in_flight,
            per_provider_concurrency,
        } => {
            freshness_seconds.is_none_or(|v| v == report.saved_policy.freshness_seconds)
                && max_in_flight.is_none_or(|v| v == report.saved_policy.max_in_flight)
                && per_provider_concurrency
                    .is_none_or(|v| v == report.saved_policy.per_provider_concurrency)
        }
        SchedulingCommand::Node {
            node,
            max_in_flight,
        } => report.saved_policy.node_limits.get(node) == Some(max_in_flight),
        SchedulingCommand::RemoveNode { node } => {
            !report.saved_policy.node_limits.contains_key(node)
        }
    }
}

pub fn command(
    config: &crate::config::Config,
    applied: &SchedulingPolicy,
    request: &SchedulingCommand,
) -> Result<Value> {
    config.edit_for_restart(|value| {
        let before = SchedulingPolicy::parse(value.get("scheduling"))?;
        let mut saved = before.clone();
        match request {
            SchedulingCommand::Show {} => {},
            SchedulingCommand::Limits { freshness_seconds, max_in_flight, per_provider_concurrency } => {
                if freshness_seconds.is_none() && max_in_flight.is_none() && per_provider_concurrency.is_none() {
                    bail!("select at least one scheduling limit")
                }
                if let Some(value) = freshness_seconds { saved.freshness_seconds = *value; }
                if let Some(value) = max_in_flight { saved.max_in_flight = *value; }
                if let Some(value) = per_provider_concurrency { saved.per_provider_concurrency = *value; }
            },
            SchedulingCommand::Node { node, max_in_flight } => { saved.node_limits.insert(node.clone(), *max_in_flight); },
            SchedulingCommand::RemoveNode { node } => {
                eefn::network::validate_node_id(node)?;
                if node == "*" { bail!("node scheduling limits require exact IDs") }
                saved.node_limits.remove(node);
            },
        }
        saved.validate()?;
        let changed = saved != before;
        if changed { value["scheduling"] = serde_json::to_value(&saved)?; }
        Ok((json!({"schema_version":1,"report_type":"coordinator_scheduling","success":true,
            "changed":changed,"saved_policy":saved,"applied_policy":applied,
            "restart_required":saved != *applied,"execution_authorized":false,"dispatched":false,
            "scope":"coordinator_local_non_model_reservations",
            "note":"Saved limits apply after explicit eef restart. Zero removes an additional ceiling, not executor limits or permissions. Reservations track local futures, not remote cancellation or physical resources. Model routing and direct owner recovery commands are separate."}),changed))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_and_confirmation_reject_extra_authority_and_inconsistent_results() {
        for operation in [
            json!({"operation":"show"}),
            json!({"operation":"limits","max_in_flight":1}),
            json!({"operation":"node","node":"n","max_in_flight":1}),
            json!({"operation":"remove_node","node":"n"}),
        ] {
            let mut command = operation.clone();
            command["permissions"] = json!(true);
            assert!(
                serde_json::from_value::<SchedulingRequest>(
                    json!({"schema_version":1,"expected_runtime_id":"runtime","command":command})
                )
                .is_err()
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        config.save_for_restart(&config.as_value()).unwrap();
        let policy = SchedulingPolicy::default();
        let mut report = command(&config, &policy, &SchedulingCommand::Show {}).unwrap();
        report["runtime_id"] = json!("runtime");
        report["admission"] = crate::capability::CapabilityRegistry::default().admission_snapshot();
        assert!(confirmed_report(
            &report,
            Some("runtime"),
            &SchedulingCommand::Show {}
        ));
        assert!(!confirmed_report(
            &report,
            Some("stale"),
            &SchedulingCommand::Show {}
        ));
        for (field, value) in [
            ("execution_authorized", json!(true)),
            ("dispatched", json!(true)),
            ("restart_required", json!(true)),
            ("report_type", json!("another")),
            ("unexpected", json!(true)),
        ] {
            let mut bad = report.clone();
            bad[field] = value;
            assert!(
                !confirmed_report(&bad, Some("runtime"), &SchedulingCommand::Show {}),
                "{field}"
            );
        }
        let mut bad = report.clone();
        bad["saved_policy"]
            .as_object_mut()
            .unwrap()
            .remove("node_limits");
        assert!(!confirmed_report(&bad, None, &SchedulingCommand::Show {}));
        let mut bad = report.clone();
        bad["admission"]["total_in_flight"] = json!(1);
        assert!(!confirmed_report(&bad, None, &SchedulingCommand::Show {}));
        assert!(!confirmed_report(
            &report,
            None,
            &SchedulingCommand::Limits {
                freshness_seconds: None,
                max_in_flight: Some(1),
                per_provider_concurrency: None
            }
        ));
        assert!(!confirmed_report(
            &report,
            None,
            &SchedulingCommand::Node {
                node: "node".into(),
                max_in_flight: 1
            }
        ));
    }
    #[test]
    fn policy_defaults_bounds_and_strict_schema() {
        assert_eq!(
            SchedulingPolicy::parse(None).unwrap(),
            SchedulingPolicy::default()
        );
        for value in [
            json!(null),
            json!({"freshness_seconds":0}),
            json!({"freshness_seconds":301}),
            json!({"max_in_flight":4097}),
            json!({"max_in_flight":-1}),
            json!({"per_provider_concurrency":"2"}),
            json!({"node_limits":{"*":1}}),
            json!({"node_limits":{"node":4097}}),
            json!({"grants":true}),
        ] {
            assert!(SchedulingPolicy::parse(Some(&value)).is_err(), "{value}");
        }
    }
    #[test]
    fn commands_preserve_latest_config_and_never_change_applied_policy() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        let mut value = config.as_value();
        value["custom"] = json!(42);
        config.save_for_restart(&value).unwrap();
        let applied = SchedulingPolicy::default();
        let limits = SchedulingCommand::Limits {
            freshness_seconds: Some(30),
            max_in_flight: Some(2),
            per_provider_concurrency: Some(1),
        };
        let report = command(&config, &applied, &limits).unwrap();
        assert_eq!(report["changed"], true);
        assert_eq!(report["restart_required"], true);
        assert_eq!(report["applied_policy"], json!(applied));
        command(
            &config,
            &applied,
            &SchedulingCommand::Node {
                node: "node".into(),
                max_in_flight: 1,
            },
        )
        .unwrap();
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        assert_eq!(
            command(&config, &applied, &limits).unwrap()["changed"],
            false
        );
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        assert!(
            command(
                &config,
                &applied,
                &SchedulingCommand::Node {
                    node: "*".into(),
                    max_in_flight: 1
                }
            )
            .is_err()
        );
        assert!(
            command(
                &config,
                &applied,
                &SchedulingCommand::Limits {
                    freshness_seconds: None,
                    max_in_flight: None,
                    per_provider_concurrency: None
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        let latest = crate::config::Config::load(config.source()).unwrap();
        assert_eq!(latest.get("custom"), Some(&json!(42)));
        assert_eq!(latest.get("scheduling.node_limits.node"), Some(&json!(1)));
        command(
            &config,
            &applied,
            &SchedulingCommand::RemoveNode {
                node: "node".into(),
            },
        )
        .unwrap();
        assert_eq!(
            command(&config, &applied, &SchedulingCommand::Show {}).unwrap()["saved_policy"]["node_limits"],
            json!({})
        );
    }
}
