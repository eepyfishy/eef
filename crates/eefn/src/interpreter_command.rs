//! Reference-only commands for an explicitly authorized network interpreter.
//! Neither caller prompts nor authority objects are accepted here.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Initial,
    Clarification,
    Method,
}
impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Clarification => "clarification",
            Self::Method => "method",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    #[arg(long)]
    #[serde(default)]
    pub local_only: bool,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub backend: Option<String>,
    #[arg(long)]
    pub model_id: Option<String>,
}
impl Preferences {
    pub fn validate(&self, origin: &str) -> Result<()> {
        if let Some(provider) = &self.provider {
            crate::network::validate_remote_node_id(provider)?;
            ensure!(
                !self.local_only || provider == origin,
                "Local-only preference conflicts with provider"
            );
        }
        ensure!(
            self.backend
                .as_ref()
                .is_none_or(|b| matches!(b.as_str(), "ollama" | "llamacpp")),
            "Unsupported interpreter backend"
        );
        if let Some(model) = &self.model_id {
            crate::model_selection::validate_id(model)?;
        }
        Ok(())
    }
    pub fn matches(&self, origin: &str, provider: &str, backend: &str, model: &str) -> bool {
        (!self.local_only || provider == origin)
            && self.provider.as_deref().is_none_or(|v| v == provider)
            && self.backend.as_deref().is_none_or(|v| v == backend)
            && self.model_id.as_deref().is_none_or(|v| v == model)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub stage: Stage,
    /// Initial input may let the backend pin its retained revision. Later
    /// stages must name the exact review the user is continuing.
    pub expected_revision: Option<u64>,
    pub operation_id: String,
    pub preferences: Preferences,
}
impl Command {
    pub fn validate(&self, origin: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::network::validate_remote_node_id(origin)?;
        self.preferences.validate(origin)?;
        ensure!(
            self.expected_revision.is_none_or(|v| v > 0 && v < u64::MAX)
                && (self.stage == Stage::Initial || self.expected_revision.is_some()),
            "This interpreter stage needs an exact positive source revision"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 4096,
            "Interpreter command exceeds bounds"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRequest {
    pub schema_version: u32,
    pub origin_runtime_id: String,
    pub command: Command,
}
impl WireRequest {
    pub fn validate(&self, origin: &str) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported interpreter command schema"
        );
        crate::interpreter_policy::uuid(&self.origin_runtime_id)?;
        self.command.validate(origin)
    }
}

/// Inspection data only. No grant body, raw prompt, token or serving credential.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub id: String,
    pub operation_id: String,
    pub source_revision: u64,
    pub phase: String,
    pub provider: String,
    pub backend: String,
    pub model_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub runtime_id: String,
    pub origin_node: String,
    pub command: Command,
    pub attempt: Attempt,
    pub success: bool,
    pub source_applied: bool,
    pub historical_snapshot: bool,
    pub model_inference_requested: bool,
    pub outcome_unknown: bool,
    pub view: Option<Value>,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, command: &Command) -> bool {
    let checked = || -> Result<()> {
        command.validate(node)?;
        let r: Report = serde_json::from_value(value.clone())?;
        ensure!(
            r.schema_version == 1
                && r.report_type == "coordinator_interpreter_command"
                && r.origin_node == node
                && r.command == *command
                && !r.execution_authorized
                && !r.dispatched
                && !r.automatically_retried
                && r.success == r.source_applied
                && !(r.historical_snapshot && r.model_inference_requested)
                && !(r.source_applied && r.outcome_unknown),
            "Interpreter reply differs"
        );
        crate::interpreter_policy::uuid(&r.runtime_id)?;
        crate::interpreter_policy::uuid(&r.attempt.id)?;
        crate::job_commands::validate_operation_id(&r.attempt.operation_id)?;
        crate::network::validate_remote_node_id(&r.attempt.provider)?;
        crate::model_selection::validate_id(&r.attempt.model_id)?;
        ensure!(
            r.attempt.source_revision > 0
                && command
                    .expected_revision
                    .is_none_or(|v| v == r.attempt.source_revision)
                && matches!(
                    r.attempt.phase.as_str(),
                    "reserved"
                        | "delivery_intent"
                        | "accepted"
                        | "final"
                        | "interrupted"
                        | "not_sent"
                )
                && matches!(r.attempt.backend.as_str(), "ollama" | "llamacpp")
                && command.preferences.matches(
                    node,
                    &r.attempt.provider,
                    &r.attempt.backend,
                    &r.attempt.model_id
                )
                && (r.historical_snapshot || r.attempt.operation_id == command.operation_id),
            "Interpreter attempt differs"
        );
        if let Some(view) = r.view {
            ensure!(
                crate::request_planning::confirmed(
                    &view,
                    node,
                    &command.operation_id,
                    &crate::request_planning::PlanningCommand::Inspect {
                        id: command.id.clone()
                    }
                ),
                "Interpreter source view differs"
            );
        } else {
            ensure!(
                !r.source_applied,
                "Applied interpreter result has no review"
            );
        }
        Ok(())
    };
    checked().is_ok()
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
            "Unsupported node interpreter command schema"
        );
        crate::interpreter_policy::uuid(&self.expected_runtime_id)?;
        self.command.validate(&self.expected_node_id)
    }
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
    diagnostic: Option<String>,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
}
pub fn unconfirmed(request: &Request) -> Value {
    failure(request, "local_reply_unconfirmed", true, None)
}
fn failure(request: &Request, code: &str, unknown: bool, diagnostic: Option<String>) -> Value {
    serde_json::json!(NodeReport {
        schema_version: 1,
        report_type: "node_interpreter_command".into(),
        node_id: request.expected_node_id.clone(),
        runtime_id: request.expected_runtime_id.clone(),
        command: request.command.clone(),
        success: false,
        coordinator_acknowledged: false,
        outcome_unknown: unknown,
        result: None,
        error_code: Some(code.into()),
        diagnostic,
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false,
    })
}
pub fn confirmed_node(value: &Value, request: &Request) -> bool {
    let checked = || -> Result<()> {
        request.validate()?;
        let r: NodeReport = serde_json::from_value(value.clone())?;
        ensure!(
            r.schema_version == 1
                && r.report_type == "node_interpreter_command"
                && r.node_id == request.expected_node_id
                && r.runtime_id == request.expected_runtime_id
                && r.command == request.command
                && !r.execution_authorized
                && !r.dispatched
                && !r.automatically_retried,
            "Node interpreter reply differs"
        );
        if r.coordinator_acknowledged {
            let result = r.result.context("Missing interpreter reply")?;
            ensure!(
                r.error_code.is_none()
                    && r.diagnostic.is_none()
                    && confirmed(&result, &r.node_id, &r.command)
                    && result["success"] == r.success
                    && result["outcome_unknown"] == r.outcome_unknown,
                "Unconfirmed coordinator interpreter reply"
            );
        } else {
            ensure!(
                !r.success
                    && r.result.is_none()
                    && r.error_code.as_deref().is_some_and(|c| matches!(
                        c,
                        "runtime_unavailable"
                            | "runtime_changed"
                            | "busy"
                            | "not_sent"
                            | "coordinator_rejected"
                            | "reply_unconfirmed"
                            | "local_reply_unconfirmed"
                    ))
                    && r.diagnostic
                        .as_ref()
                        .is_none_or(|v| v.len() <= 1024 && !v.chars().any(char::is_control)),
                "Invalid node interpreter failure"
            );
        }
        Ok(())
    };
    checked().is_ok()
}

pub fn render(value: &Value) -> Result<String> {
    let report: NodeReport = serde_json::from_value(value.clone())?;
    let request = Request {
        schema_version: 1,
        expected_node_id: report.node_id.clone(),
        expected_runtime_id: report.runtime_id.clone(),
        command: report.command.clone(),
    };
    ensure!(
        confirmed_node(value, &request),
        "Cannot render an unconfirmed interpreter reply"
    );
    let mut text = String::new();
    let heading = if report.success {
        "Interpreter result saved"
    } else {
        "Interpreter result unavailable"
    };
    text.push_str(&format!("{heading} for request {}\n", report.command.id));
    if let Some(result) = &report.result {
        if result["historical_snapshot"] == true {
            text.push_str("Retained history; this command did not request another inference.\n");
        }
        if let Some(status) = result.pointer("/view/status").and_then(Value::as_str) {
            text.push_str(&format!("Request status: {status}\n"));
        }
        if result["source_applied"] == true {
            text.push_str("The saved result still needs the ordinary review and approval steps.\n");
        }
    }
    if let Some(diagnostic) = &report.diagnostic {
        text.push_str(diagnostic);
        text.push('\n');
    }
    if report.outcome_unknown {
        text.push_str(
            "An inference or source update may have happened. Inspect before trying again.\n",
        );
    }
    text.push_str(&format!("Inspect: eefn jobs get {}\nStop: eefn jobs stop {}\nOperation: {}\nNo automatic retry or tool execution was requested.\n",
        report.command.id,report.command.id,report.command.operation_id));
    Ok(text)
}

impl crate::NodeService {
    pub async fn interpreter_command(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Interpreter command targets another node"
        );
        let _admission = match self.input_workflow_permit() {
            Ok(p) => p,
            Err(_) => return Ok(failure(&request, "busy", false, None)),
        };
        let Some((runtime, _, mut generation)) = self.input_workflow_runtime() else {
            return Ok(failure(&request, "runtime_unavailable", false, None));
        };
        if runtime != request.expected_runtime_id {
            return Ok(failure(&request, "runtime_changed", false, None));
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
                    "requests.interpreter",
                    serde_json::json!(wire),
                    Some(request.command.operation_id.clone()),
                )
                .await)
        })
        .await;
        let value = match submitted {
            Err(_) => failure(&request, "runtime_changed", true, None),
            Ok(Ok(result)) if confirmed(&result, &self.node_id, &request.command) => {
                serde_json::json!(NodeReport {
                    schema_version: 1,
                    report_type: "node_interpreter_command".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: runtime,
                    command: request.command.clone(),
                    success: result["success"] == true,
                    coordinator_acknowledged: true,
                    outcome_unknown: result["outcome_unknown"] == true,
                    result: Some(result),
                    error_code: None,
                    diagnostic: None,
                    execution_authorized: false,
                    dispatched: false,
                    automatically_retried: false,
                })
            }
            Ok(Err(error)) => match error.downcast_ref::<crate::submission::SubmissionFailure>() {
                Some(crate::submission::SubmissionFailure::NotSent(_)) => {
                    failure(&request, "not_sent", false, None)
                }
                Some(crate::submission::SubmissionFailure::Rejected(reason)) => {
                    let mut diagnostic = String::new();
                    for c in reason.chars().filter(|c| !c.is_control()) {
                        if diagnostic.len() + c.len_utf8() > 1024 {
                            break;
                        }
                        diagnostic.push(c);
                    }
                    failure(&request, "coordinator_rejected", true, Some(diagnostic))
                }
                None => failure(&request, "reply_unconfirmed", true, None),
            },
            _ => failure(&request, "reply_unconfirmed", true, None),
        };
        ensure!(
            confirmed_node(&value, &request),
            "Node interpreter command could not be confirmed"
        );
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn interpreter_commands_accept_only_references_and_explicit_bounded_preferences() {
        let value = json!({"id":"saved","stage":"initial","expected_revision":null,
            "operation_id":uuid::Uuid::new_v4().to_string(),"preferences":{"local_only":false,
            "provider":null,"backend":null,"model_id":null}});
        let command: Command = serde_json::from_value(value.clone()).unwrap();
        command.validate("owner").unwrap();
        for field in [
            "prompt",
            "messages",
            "grant",
            "claim",
            "origin_node",
            "execution_authorized",
        ] {
            let mut forged = value.clone();
            forged[field] = json!(true);
            assert!(serde_json::from_value::<Command>(forged).is_err());
        }
        for stage in [Stage::Clarification, Stage::Method] {
            let mut bad = command.clone();
            bad.stage = stage;
            assert!(bad.validate("owner").is_err());
            bad.expected_revision = Some(1);
            assert!(bad.validate("owner").is_ok());
        }
        let mut bad = command;
        bad.preferences.local_only = true;
        bad.preferences.provider = Some("peer".into());
        assert!(bad.validate("owner").is_err());
    }
    #[test]
    fn node_interpreter_failures_cannot_claim_a_checkpoint_or_execution() {
        let request = Request {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            command: Command {
                id: "saved".into(),
                stage: Stage::Initial,
                expected_revision: None,
                operation_id: uuid::Uuid::new_v4().to_string(),
                preferences: Preferences::default(),
            },
        };
        let value = unconfirmed(&request);
        assert!(confirmed_node(&value, &request));
        let plain = render(&value).unwrap();
        assert!(
            plain.contains("eefn jobs get saved") && plain.contains("Inspect before trying again")
        );
        for field in [
            "success",
            "coordinator_acknowledged",
            "execution_authorized",
            "dispatched",
            "automatically_retried",
        ] {
            let mut forged = value.clone();
            forged[field] = json!(true);
            assert!(!confirmed_node(&forged, &request), "{field}");
        }
        let mut forged = value.clone();
        forged["grant"] = json!({});
        assert!(!confirmed_node(&forged, &request));
        let mut forged = value;
        forged["diagnostic"] = json!("unsafe\nterminal");
        assert!(!confirmed_node(&forged, &request));
    }
}
