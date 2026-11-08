//! Coordinator owner adapter; camera configuration remains owned by the node.
use anyhow::{Context, Result, ensure};
use eefn::camera_setup::{Command, Request as NodeRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
        ensure!(
            self.schema_version == 1,
            "Unsupported remote camera setup schema"
        );
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        }
        if matches!(self.command, Command::Configure { .. }) {
            NodeRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                expected_runtime_id: self
                    .expected_runtime_id
                    .clone()
                    .context("Exact node runtime required")?,
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
    request_sha256: String,
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
    let mutation = matches!(request.command, Command::Configure { .. });
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "remote_camera_setup"
        || r.node_id != request.node_id
        || !eefn::grants::definition_digest(&json!(request)).is_ok_and(|d| d == r.request_sha256)
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
    eefn::camera_setup::confirmed(
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
    let mutation = matches!(request.command, Command::Configure { .. });
    let mut report = json!(Report {
        schema_version: 1,
        report_type: "remote_camera_setup".into(),
        node_id: request.node_id.clone(),
        request_sha256: eefn::grants::definition_digest(&json!(request))?,
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
        let Some(state) = server.node_state(&request.node_id).await else {
            return Ok(report);
        };
        let offer: eefn::registration::Offer =
            serde_json::from_value(state.registration["registration_receipt"].clone())?;
        offer.validate()?;
        offer.runtime_id
    };
    let node = NodeRequest {
        schema_version: 1,
        expected_node_id: request.node_id.clone(),
        expected_runtime_id: runtime,
        command: request.command.clone(),
    };
    node.validate()?;
    // This attempt can fail before send as well; after deciding to mutate,
    // transport errors remain conservative uncertainty and never trigger retry.
    report["sending_attempted"] = json!(mutation);
    report["outcome_unknown"] = json!(mutation);
    if let Ok(reply) = server.invoke_camera_setup(node.clone()).await {
        if reply["success"] == true && eefn::camera_setup::confirmed(&reply["data"], &node) {
            report["success"] = json!(true);
            report["outcome_unknown"] = json!(false);
            report["result"] = reply["data"].clone();
            ensure!(
                confirmed(&report, &request),
                "Remote camera setup reply unconfirmed"
            );
        }
    }
    Ok(report)
}
