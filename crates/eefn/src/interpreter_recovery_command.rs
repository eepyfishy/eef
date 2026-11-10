//! Origin-scoped inspection and explicit recovery of saved semantic data.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Inspect {
        id: String,
        operation_id: String,
    },
    Recover {
        command: crate::interpreter_recovery::Command,
    },
}
impl Command {
    pub fn id(&self) -> &str {
        match self {
            Self::Inspect { id, .. } => id,
            Self::Recover { command } => &command.id,
        }
    }
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Inspect { operation_id, .. } => operation_id,
            Self::Recover { command } => &command.operation_id,
        }
    }
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(self.id())?;
        crate::interpreter_policy::uuid(self.operation_id())?;
        if let Self::Recover { command } = self {
            command.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter recovery schema"
        );
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::interpreter_policy::uuid(&self.expected_runtime_id)?;
        self.command.validate()?;
        ensure!(
            serde_json::to_vec(self)?.len() <= 8192,
            "Recovery command exceeds bounds"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRequest {
    pub schema_version: u32,
    pub origin_runtime_id: String,
    pub command: Command,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub attempt: crate::interpreter_command::Attempt,
    pub stage: crate::interpreter_command::Stage,
    pub completed_result: bool,
    pub source_applied: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub runtime_id: String,
    pub origin_node: String,
    pub command: Command,
    pub revision: u64,
    pub status: String,
    pub attempts: Vec<Attempt>,
    pub truncated: bool,
    pub recovery: Option<crate::interpreter_recovery::Record>,
    pub reused: bool,
    pub model_inference_requested: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, command: &Command, origin: &str) -> bool {
    let check = || -> Result<()> {
        command.validate()?;
        let r: Report = serde_json::from_value(value.clone())?;
        crate::interpreter_policy::uuid(&r.runtime_id)?;
        ensure!(
            r.schema_version == 1
                && r.report_type == "coordinator_interpreter_recovery"
                && r.command == *command
                && r.origin_node == origin
                && r.revision > 0
                && matches!(
                    r.status.as_str(),
                    "review"
                        | "awaiting_approval"
                        | "approved"
                        | "pending"
                        | "running"
                        | "pausing"
                        | "paused"
                        | "stopping"
                        | "interrupted"
                        | "cancelled"
                        | "completed"
                        | "failed"
                        | "abandoned"
                )
                && !r.model_inference_requested
                && !r.execution_authorized
                && !r.dispatched
                && !r.automatically_retried
                && r.attempts.len() <= 48
                && serde_json::to_vec(&r)?.len() <= 64 * 1024,
            "Invalid interpreter inspection/recovery reply"
        );
        let mut ids = std::collections::BTreeSet::new();
        for item in &r.attempts {
            let a = &item.attempt;
            crate::interpreter_policy::uuid(&a.id)?;
            crate::interpreter_policy::uuid(&a.operation_id)?;
            crate::network::validate_remote_node_id(&a.provider)?;
            crate::model_selection::validate_id(&a.model_id)?;
            ensure!(
                ids.insert(&a.id)
                    && a.source_revision > 0
                    && a.source_revision <= r.revision
                    && matches!(
                        a.phase.as_str(),
                        "reserved"
                            | "delivery_intent"
                            | "accepted"
                            | "final"
                            | "interrupted"
                            | "not_sent"
                    )
                    && matches!(a.backend.as_str(), "ollama" | "llamacpp")
                    && a.execution_limit_ms
                        .is_some_and(|n| (1..=crate::grants::MAX_DURATION_MS).contains(&n))
                    && (!item.completed_result || a.phase == "final"),
                "Invalid retained interpreter header"
            );
        }
        match command {
            Command::Inspect { .. } => ensure!(
                r.recovery.is_none() && !r.reused,
                "Inspection cannot recover a result"
            ),
            Command::Recover { command } => {
                let receipt = r.recovery.context("Missing recovery receipt")?;
                receipt.validate()?;
                ensure!(
                    receipt.command == *command
                        && receipt.actor == crate::interpreter_recovery::RecoveryActor::OriginNode
                        && r.revision >= receipt.applied_revision
                        && r.attempts.is_empty()
                        && !r.truncated,
                    "Recovery receipt differs from the explicit decision"
                );
            }
        }
        Ok(())
    };
    check().is_ok()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    command: Command,
    success: bool,
    coordinator_acknowledged: bool,
    outcome_unknown: bool,
    result: Option<Value>,
    error_code: Option<String>,
    model_inference_requested: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
}
fn failure(request: &Request, code: &str, unknown: bool) -> Value {
    json!(NodeReport {
        schema_version: 1,
        report_type: "node_interpreter_recovery".into(),
        node_id: request.expected_node_id.clone(),
        runtime_id: request.expected_runtime_id.clone(),
        command: request.command.clone(),
        success: false,
        coordinator_acknowledged: false,
        outcome_unknown: unknown,
        result: None,
        error_code: Some(code.into()),
        model_inference_requested: false,
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false
    })
}
pub fn unconfirmed(request: &Request) -> Value {
    failure(request, "reply_unconfirmed", true)
}
pub fn confirmed_node(value: &Value, request: &Request) -> bool {
    let check = || -> Result<()> {
        request.validate()?;
        let r: NodeReport = serde_json::from_value(value.clone())?;
        ensure!(
            r.schema_version == 1
                && r.report_type == "node_interpreter_recovery"
                && r.node_id == request.expected_node_id
                && r.runtime_id == request.expected_runtime_id
                && r.command == request.command
                && !r.model_inference_requested
                && !r.execution_authorized
                && !r.dispatched
                && !r.automatically_retried,
            "Node recovery reply differs"
        );
        if r.success {
            ensure!(
                r.coordinator_acknowledged
                    && !r.outcome_unknown
                    && r.error_code.is_none()
                    && r.result.as_ref().is_some_and(|v| confirmed(
                        v,
                        &request.command,
                        &r.node_id
                    )),
                "Unconfirmed recovery reply"
            );
        } else {
            ensure!(
                !r.coordinator_acknowledged
                    && r.result.is_none()
                    && r.error_code.as_deref().is_some_and(|c| matches!(
                        c,
                        "runtime_unavailable"
                            | "runtime_changed"
                            | "busy"
                            | "not_sent"
                            | "coordinator_rejected"
                            | "reply_unconfirmed"
                    )),
                "Invalid recovery failure"
            );
        }
        Ok(())
    };
    check().is_ok()
}
pub fn render(value: &Value) -> Result<String> {
    let r: NodeReport = serde_json::from_value(value.clone())?;
    let request = Request {
        schema_version: 1,
        expected_node_id: r.node_id.clone(),
        expected_runtime_id: r.runtime_id.clone(),
        command: r.command.clone(),
    };
    ensure!(confirmed_node(value, &request), "Invalid recovery output");
    let mut text = format!(
        "Request: {}\nOperation: {}\n",
        r.command.id(),
        r.command.operation_id()
    );
    if !r.success {
        text.push_str(if r.outcome_unknown { "The source outcome is unconfirmed. Inspect before trying again; no inference or tool execution was requested.\n" } else { "The command was not sent. Check the running node and its connection.\n" });
        return Ok(text);
    }
    let report: Report = serde_json::from_value(r.result.unwrap())?;
    text.push_str(&format!(
        "Status: {}\nRevision: {}\n",
        report.status, report.revision
    ));
    if let Some(receipt) = report.recovery {
        text.push_str(&format!("Saved result recovered: {}\nApplied revision: {}\nNo new inference or tool execution.\n",receipt.command.grant_id,receipt.applied_revision));
    } else {
        text.push_str("Saved interpreter attempts (inspection only):\n");
        for item in report.attempts {
            text.push_str(&format!(
                "  {} | {} | {} | completed result: {} | applied: {}\n",
                item.attempt.id,
                item.stage.as_str(),
                item.attempt.phase,
                item.completed_result,
                item.source_applied
            ));
        }
        if report.truncated {
            text.push_str("Only the latest 48 attempts are shown.\n");
        }
        text.push_str("A completed result is evidence, not permission to apply or execute it.\n");
    }
    Ok(text)
}
impl crate::NodeService {
    pub async fn interpreter_recovery_command(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Recovery targets another node"
        );
        let _admission = match self.input_workflow_permit() {
            Ok(p) => p,
            Err(_) => return Ok(failure(&request, "busy", false)),
        };
        let Some((runtime, _, mut generation)) = self.input_workflow_runtime() else {
            return Ok(failure(&request, "runtime_unavailable", false));
        };
        if runtime != request.expected_runtime_id {
            return Ok(failure(&request, "runtime_changed", false));
        }
        let wire = WireRequest {
            schema_version: 1,
            origin_runtime_id: runtime.clone(),
            command: request.command.clone(),
        };
        let submitted = crate::request_input::within_runtime(&mut generation, &runtime, async {
            Ok(self
                .submissions
                .request_correlated(
                    "requests.interpreter_recovery",
                    json!(wire),
                    Some(request.command.operation_id().into()),
                )
                .await)
        })
        .await;
        let reply = match submitted {
            Ok(Ok(result)) if confirmed(&result, &request.command, &self.node_id) => {
                json!(NodeReport {
                    schema_version: 1,
                    report_type: "node_interpreter_recovery".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: runtime,
                    command: request.command.clone(),
                    success: true,
                    coordinator_acknowledged: true,
                    outcome_unknown: false,
                    result: Some(result),
                    error_code: None,
                    model_inference_requested: false,
                    execution_authorized: false,
                    dispatched: false,
                    automatically_retried: false
                })
            }
            Err(_) => failure(&request, "runtime_changed", true),
            Ok(Err(error)) => match error.downcast_ref::<crate::submission::SubmissionFailure>() {
                Some(crate::submission::SubmissionFailure::NotSent(_)) => {
                    failure(&request, "not_sent", false)
                }
                Some(crate::submission::SubmissionFailure::Rejected(_)) => {
                    failure(&request, "coordinator_rejected", true)
                }
                None => unconfirmed(&request),
            },
            _ => unconfirmed(&request),
        };
        ensure!(
            confirmed_node(&reply, &request),
            "Node recovery reply could not be confirmed"
        );
        Ok(reply)
    }
}

#[cfg(test)]
#[path = "interpreter_recovery_command_tests.rs"]
mod tests;
