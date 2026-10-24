//! Coordinator transport adapter; the node owns installation policy and consent.
use anyhow::{Result, bail};
use eefn::package_setup::policy::{Command, Request as NodeRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub node_id: String,
    pub expected_runtime_id: Option<String>,
    pub request_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported remote package policy schema")
        }
        eefn::network::validate_remote_node_id(&self.node_id)?;
        eefn::automation_occurrence::uuid(&self.request_id)?;
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        } else if !matches!(self.command, Command::Show) {
            bail!("Exact inspected runtime required for policy save")
        }
        self.command.validate()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    request_id: String,
    command: Command,
    success: bool,
    mutation_requested: bool,
    sending_attempted: bool,
    outcome_unknown: bool,
    result: Option<Value>,
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
        || r.report_type != "remote_repair_setup_policy"
        || !r.success
        || r.node_id != request.node_id
        || r.request_id != request.request_id
        || r.command != request.command
        || r.mutation_requested != mutation
        || r.sending_attempted != mutation
        || r.outcome_unknown
        || r.restart_requested
        || r.automatically_retried
    {
        return false;
    }
    let Some(result) = r.result else { return false };
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
    eefn::package_setup::policy::confirmed(
        &result,
        &NodeRequest {
            schema_version: 1,
            expected_node_id: request.node_id.clone(),
            expected_runtime_id: runtime.into(),
            request_id: request.request_id.clone(),
            command: request.command.clone(),
        },
        true,
    )
}
pub async fn command(server: &eefn::NodeServer, request: Request) -> Result<Value> {
    request.validate()?;
    let mutation = !matches!(request.command, Command::Show);
    let mut report = json!(Report {
        schema_version: 1,
        report_type: "remote_repair_setup_policy".into(),
        node_id: request.node_id.clone(),
        request_id: request.request_id.clone(),
        command: request.command.clone(),
        success: false,
        mutation_requested: mutation,
        sending_attempted: false,
        outcome_unknown: false,
        result: None,
        restart_requested: false,
        automatically_retried: false,
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
        request_id: request.request_id.clone(),
        command: request.command.clone(),
    };
    node.validate()?;
    report["sending_attempted"] = json!(mutation);
    report["outcome_unknown"] = json!(mutation);
    if let Ok(reply) = server
        .invoke_remote(
            &request.node_id,
            "node.configure",
            if mutation {
                "repair_setup_policy"
            } else {
                "repair_setup_policy_status"
            },
            json!(node),
            Duration::from_secs(10),
        )
        .await
    {
        if reply["success"] == true
            && eefn::package_setup::policy::confirmed(&reply["data"], &node, true)
        {
            report["success"] = json!(true);
            report["outcome_unknown"] = json!(false);
            report["result"] = reply["data"].clone();
            if !confirmed(&report, &request) {
                bail!("Remote package policy reply unconfirmed")
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_runtime_cas_and_correlation_required_no_approval_or_run_command() {
        let mut r = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            command: Command::Show,
        };
        r.validate().unwrap();
        r.command = Command::Configure {
            previous: eefn::package_setup::policy::Version {
                revision: 0,
                digest: "a".repeat(64),
            },
            mode: eefn::package_setup::Mode::Off,
            allow_automatic_repair: false,
            manifest: None,
        };
        assert!(r.validate().is_err());
        r.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        r.validate().unwrap();
        assert!(!confirmed(&json!({"success":true}), &r));
        r.request_id = "bad".into();
        assert!(r.validate().is_err());
        for operation in ["run", "approve"] {
            assert!(serde_json::from_value::<Command>(json!({"operation":operation})).is_err());
        }
    }
}
