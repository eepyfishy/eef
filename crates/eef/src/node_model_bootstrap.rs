//! Coordinator adapter for the node-owned model bootstrap policy service.
use anyhow::{Result, bail};
use eefn::model_bootstrap::{Command, Request as NodeRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub node_id: String,
    pub expected_runtime_id: Option<String>,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.node_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported remote Bootstrap preferences schema")
        }
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        }
        if !matches!(self.command, Command::Show) {
            NodeRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                expected_runtime_id: self
                    .expected_runtime_id
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Exact node runtime required"))?,
                command: self.command.clone(),
            }
            .validate()?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    command: Command,
    success: bool,
    mutation_requested: bool,
    sending_attempted: bool,
    outcome_unknown: bool,
    result: Option<Value>,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let mutation = !matches!(request.command, Command::Show);
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "remote_model_bootstrap"
        || r.node_id != request.node_id
        || r.command != request.command
        || !r.success
        || r.mutation_requested != mutation
        || r.sending_attempted != mutation
        || r.outcome_unknown
        || r.execution_authorized
        || r.restart_requested
        || r.automatically_retried
    {
        return false;
    }
    let Some(result) = r.result else {
        return false;
    };
    let Some(runtime) = result["runtime_id"].as_str() else {
        return false;
    };
    if request
        .expected_runtime_id
        .as_deref()
        .is_some_and(|expected| expected != runtime)
    {
        return false;
    }
    eefn::model_bootstrap::confirmed(
        &result,
        &NodeRequest {
            schema_version: 1,
            expected_node_id: request.node_id.clone(),
            expected_runtime_id: runtime.into(),
            command: request.command.clone(),
        },
    )
}
pub async fn command(server: &eefn::NodeServer, request: Request) -> Result<Value> {
    request.validate()?;
    let mutation = !matches!(request.command, Command::Show);
    let mut report = json!(Report {
        schema_version: 1,
        report_type: "remote_model_bootstrap".into(),
        node_id: request.node_id.clone(),
        command: request.command.clone(),
        success: false,
        mutation_requested: mutation,
        sending_attempted: false,
        outcome_unknown: false,
        result: None,
        execution_authorized: false,
        restart_requested: false,
        automatically_retried: false
    });
    let runtime = if let Some(runtime) = &request.expected_runtime_id {
        runtime.clone()
    } else {
        let Ok(status) = server
            .invoke_remote(
                &request.node_id,
                "node.configure",
                "status",
                json!({}),
                Duration::from_secs(5),
            )
            .await
        else {
            return Ok(report);
        };
        if !crate::node_control::control_status(&status, &request.node_id) {
            return Ok(report);
        }
        status["data"]["runtime_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing node runtime"))?
            .into()
    };
    let node = NodeRequest {
        schema_version: 1,
        expected_node_id: request.node_id.clone(),
        expected_runtime_id: runtime,
        command: request.command.clone(),
    };
    node.validate()?;
    report["sending_attempted"] = json!(mutation);
    report["outcome_unknown"] = json!(mutation);
    let reply = server
        .invoke_remote(
            &request.node_id,
            "node.configure",
            if mutation {
                "bootstrap_policy"
            } else {
                "bootstrap_policy_status"
            },
            json!(node),
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true && eefn::model_bootstrap::confirmed(&reply["data"], &node) {
            report["success"] = json!(true);
            report["outcome_unknown"] = json!(false);
            report["result"] = reply["data"].clone();
            if !confirmed(&report, &request) {
                bail!("Remote Bootstrap preferences reply unconfirmed")
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_stop_requires_exact_runtime_policy_and_attempt_without_legacy_fallback() {
        let mut request = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            command: Command::Stop {
                previous: eefn::model_bootstrap::Saved::new(Default::default(), 0).unwrap(),
                attempt_id: uuid::Uuid::new_v4().to_string(),
            },
        };
        assert!(request.validate().is_err());
        request.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        request.validate().unwrap();
        assert!(!confirmed(&json!({"success":true}), &request));
        if let Command::Stop { attempt_id, .. } = &mut request.command {
            *attempt_id = "invalid".into();
        }
        assert!(request.validate().is_err());
        if let Command::Stop {
            attempt_id,
            previous,
        } = &mut request.command
        {
            *attempt_id = uuid::Uuid::new_v4().to_string();
            previous.digest.clear();
        }
        assert!(request.validate().is_err());
    }
    #[test]
    fn prompt_approval_is_a_mutation_requiring_exact_runtime_and_explicit_prompt_consent() {
        let mut policy = eefn::model_bootstrap::Policy::default();
        policy.mode = eefn::model_bootstrap::Mode::Prompt;
        policy.automation = true;
        policy.catalog = Some(eefn::model_bootstrap::CatalogPin {
            schema_version: 1,
            model_id: "fixture".into(),
            definition_sha256: "a".repeat(64),
            artifact_sha256: "b".repeat(64),
            artifact_bytes: 4,
            source_uri_sha256: "c".repeat(64),
            source_page_sha256: "d".repeat(64),
            declared_license: "test-only".into(),
        });
        let mut request = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            command: Command::Approve {
                previous: eefn::model_bootstrap::Saved::new(policy, 1).unwrap(),
            },
        };
        assert!(request.validate().is_err());
        request.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        request.validate().unwrap();
        if let Command::Approve { previous } = &mut request.command {
            previous.policy.automation = false;
            *previous = eefn::model_bootstrap::Saved::new(previous.policy.clone(), 2).unwrap();
        }
        assert!(request.validate().is_err());
    }
    #[test]
    fn bootstrap_remote_mutations_require_exact_runtime_and_versioned_policy() {
        let mut request = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            command: Command::Show,
        };
        request.validate().unwrap();
        request.command = Command::Configure {
            previous: eefn::model_bootstrap::Saved::new(Default::default(), 0).unwrap(),
            mode: Some(eefn::model_bootstrap::Mode::Auto),
            target: eefn::model_bootstrap::Target::Keep,
            automation: None,
            restart_after_setup: None,
        };
        assert!(request.validate().is_err());
        request.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        request.validate().unwrap();
        if let Command::Configure { previous, .. } = &mut request.command {
            previous.digest.clear();
        }
        assert!(request.validate().is_err());
        assert!(!confirmed(&json!({"success":true}), &request));
    }
}
