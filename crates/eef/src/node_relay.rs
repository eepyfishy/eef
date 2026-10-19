//! Coordinator adapter for the node-owned relay configuration service.
use anyhow::{Result, bail};
use eefn::relay_setup::{RelayCommand, RelayRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub node_id: String,
    pub expected_runtime_id: Option<String>,
    pub command: RelayCommand,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.node_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported remote relay setup schema")
        }
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        }
        if matches!(self.command, RelayCommand::Configure { .. }) {
            RelayRequest {
                schema_version: self.schema_version,
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
    command: RelayCommand,
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
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let mutation = matches!(request.command, RelayCommand::Configure { .. });
    if request.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "remote_relay_setup"
        || report.node_id != request.node_id
        || report.command != request.command
        || !report.success
        || report.mutation_requested != mutation
        || report.sending_attempted != mutation
        || report.outcome_unknown
        || report.execution_authorized
        || report.restart_requested
        || report.automatically_retried
    {
        return false;
    }
    let Some(result) = report.result else {
        return false;
    };
    let Some(runtime) = result["runtime_id"].as_str() else {
        return false;
    };
    if request
        .expected_runtime_id
        .as_deref()
        .is_some_and(|expected| runtime != expected)
    {
        return false;
    }
    eefn::relay_setup::confirmed(
        &result,
        &RelayRequest {
            schema_version: 1,
            expected_node_id: request.node_id.clone(),
            expected_runtime_id: runtime.into(),
            command: request.command.clone(),
        },
    )
}
pub async fn command(server: &eefn::NodeServer, request: Request) -> Result<Value> {
    request.validate()?;
    let mutation = matches!(request.command, RelayCommand::Configure { .. });
    let mut result = json!(Report {
        schema_version: 1,
        report_type: "remote_relay_setup".into(),
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
        let status = server
            .invoke_remote(
                &request.node_id,
                "node.configure",
                "status",
                json!({}),
                Duration::from_secs(5),
            )
            .await;
        let Ok(status) = status else {
            return Ok(result);
        };
        if !crate::node_control::control_status(&status, &request.node_id) {
            return Ok(result);
        }
        status["data"]["runtime_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing node runtime"))?
            .into()
    };
    let node = RelayRequest {
        schema_version: 1,
        expected_node_id: request.node_id.clone(),
        expected_runtime_id: runtime,
        command: request.command.clone(),
    };
    node.validate()?;
    result["sending_attempted"] = json!(mutation);
    result["outcome_unknown"] = json!(mutation);
    let reply = server
        .invoke_remote(
            &request.node_id,
            "node.configure",
            if mutation {
                "relay_command"
            } else {
                "relay_status"
            },
            serde_json::to_value(&node)?,
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true && eefn::relay_setup::confirmed(&reply["data"], &node) {
            result["success"] = json!(true);
            result["outcome_unknown"] = json!(false);
            result["result"] = reply["data"].clone();
            if !confirmed(&result, &request) {
                bail!("Remote relay checkpoint failed confirmation")
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relay_mutations_require_exact_runtime_and_delegate_native_validation() {
        let mut request = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            command: RelayCommand::Show,
        };
        request.validate().unwrap();
        request.command = RelayCommand::Configure {
            settings: eefn::relay_setup::Configuration {
                microphone: true,
                ..Default::default()
            },
            expected_digest: "a".repeat(64),
        };
        assert!(request.validate().is_err());
        request.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        request.validate().unwrap();
        let mut bad = request.clone();
        if let RelayCommand::Configure {
            expected_digest, ..
        } = &mut bad.command
        {
            *expected_digest = "bad".into();
        }
        assert!(bad.validate().is_err());
        assert!(!confirmed(
            &json!({"success":true,"execution_authorized":true}),
            &request
        ));
        request.command = RelayCommand::Show;
        assert!(!confirmed(
            &json!({"schema_version":1,"report_type":"remote_relay_setup","node_id":"node","command":{"operation":"show"},
            "success":true,"mutation_requested":false,"sending_attempted":false,"outcome_unknown":false,"result":{},
            "execution_authorized":false,"restart_requested":false,"automatically_retried":false}),
            &request
        ));
    }
}
