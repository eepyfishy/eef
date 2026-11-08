//! Coordinator adapter for the node-owned continuous-workload policy service.
use anyhow::{Result, bail};
use eefn::workload_policy::{Command, Request as NodeRequest};
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
            bail!("Unsupported remote workload policy schema")
        }
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        }
        if matches!(self.command, Command::Set { .. }) {
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
    capture_started: bool,
    restart_requested: bool,
    automatically_retried: bool,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let mutation = matches!(request.command, Command::Set { .. });
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "remote_workload_policy"
        || r.node_id != request.node_id
        || r.command != request.command
        || !r.success
        || r.mutation_requested != mutation
        || r.sending_attempted != mutation
        || r.outcome_unknown
        || r.capture_started
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
    eefn::workload_policy::confirmed(
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
    let mutation = matches!(request.command, Command::Set { .. });
    let mut report = json!(Report {
        schema_version: 1,
        report_type: "remote_workload_policy".into(),
        node_id: request.node_id.clone(),
        command: request.command.clone(),
        success: false,
        mutation_requested: mutation,
        sending_attempted: false,
        outcome_unknown: false,
        result: None,
        execution_authorized: false,
        capture_started: false,
        restart_requested: false,
        automatically_retried: false
    });
    // Policy changes target an independently authenticated, pinned node.
    // Configuration receipts never grant source execution or renew a workload.
    if server.grant_session(&request.node_id).await.is_err() {
        return Ok(report);
    }
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
                "workload_policy"
            } else {
                "workload_policy_status"
            },
            json!(node),
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true && eefn::workload_policy::confirmed(&reply["data"], &node) {
            report["success"] = json!(true);
            report["outcome_unknown"] = json!(false);
            report["result"] = reply["data"].clone();
            if !confirmed(&report, &request) {
                bail!("Remote workload policy reply unconfirmed")
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
#[path = "node_workload_policy_tests.rs"]
mod tests;
