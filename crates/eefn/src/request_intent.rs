//! Durable raw input contracts. Saving is neither interpretation nor execution.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TEMPLATE: &str = "request_intent";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema_version: u32,
    pub original_text: String,
}
impl Input {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Unsupported saved input schema");
        crate::interpretation::messages(&self.original_text, &[])?;
        ensure!(
            serde_json::to_vec(self)?.len() <= 64 * 1024,
            "Saved input exceeds its encoded byte limit"
        );
        Ok(())
    }
    pub fn description(&self) -> String {
        format!(
            "Input awaiting interpretation: {}",
            self.original_text.chars().take(128).collect::<String>()
        )
    }
    pub fn digest(&self, context: &crate::context::RequestContext) -> Result<String> {
        self.validate()?;
        // RequestContext is immutable and validates on construction/deserialization.
        crate::grants::definition_digest(&json!({"input":self,"request_context":context}))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub id: String,
    pub revision: u64,
    pub status: String,
    pub operation_id: String,
    pub request_context: crate::context::RequestContext,
    pub input: Input,
    pub source_sha256: String,
    pub reused: bool,
    pub persisted: bool,
    pub interpretation_requested: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
}
impl Saved {
    pub fn validate(&self) -> Result<()> {
        self.input.validate()?;
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        ensure!(
            self.schema_version == 1
                && self.report_type == "coordinator_request_intent"
                && self.success
                && self.persisted
                && self.revision > 0
                && matches!(self.status.as_str(), "review" | "cancelled")
                && self.operation_id == self.request_context.request_id()
                && self.source_sha256 == self.input.digest(&self.request_context)?
                && !self.interpretation_requested
                && !self.execution_authorized
                && !self.dispatched,
            "Saved input reply was not confirmed"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub operation_id: String,
    pub original_text: String,
}
impl Command {
    pub fn input(&self) -> Result<Input> {
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        let input = Input {
            schema_version: 1,
            original_text: self.original_text.clone(),
        };
        input.validate()?;
        Ok(input)
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
        ensure!(self.schema_version == 1, "Unsupported input save schema");
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        self.command.input()?;
        Ok(())
    }
}
pub fn confirms_saved(value: &Value, node: &str, command: &Command) -> bool {
    let Ok(saved) = serde_json::from_value::<Saved>(value.clone()) else {
        return false;
    };
    saved.validate().is_ok()
        && command.input().is_ok()
        && saved.request_context.origin_node() == node
        && saved.operation_id == command.operation_id
        && saved.input.original_text == command.original_text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Failure {
    RuntimeUnavailable,
    RuntimeChanged,
    NotSent,
    Refused,
    Unconfirmed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    operation_id: String,
    success: bool,
    saving_attempted: bool,
    outcome_unknown: bool,
    error_code: Option<Failure>,
    result: Option<Saved>,
    interpretation_requested: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
}
impl Report {
    fn new(request: &Request) -> Self {
        Self {
            schema_version: 1,
            report_type: "node_request_intent".into(),
            node_id: request.expected_node_id.clone(),
            runtime_id: request.expected_runtime_id.clone(),
            operation_id: request.command.operation_id.clone(),
            success: false,
            saving_attempted: false,
            outcome_unknown: false,
            error_code: Some(Failure::RuntimeUnavailable),
            result: None,
            interpretation_requested: false,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
        }
    }
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "node_request_intent"
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && r.operation_id == request.command.operation_id
        && !r.interpretation_requested
        && !r.execution_authorized
        && !r.dispatched
        && !r.automatically_retried
        && if r.success {
            r.saving_attempted
                && !r.outcome_unknown
                && r.error_code.is_none()
                && r.result
                    .as_ref()
                    .is_some_and(|s| confirms_saved(&json!(s), &r.node_id, &request.command))
        } else {
            r.result.is_none()
                && match r.error_code {
                    Some(Failure::RuntimeUnavailable) => !r.saving_attempted && !r.outcome_unknown,
                    Some(Failure::RuntimeChanged) => r.outcome_unknown == r.saving_attempted,
                    Some(Failure::NotSent | Failure::Refused) => {
                        r.saving_attempted && !r.outcome_unknown
                    }
                    Some(Failure::Unconfirmed) => r.saving_attempted && r.outcome_unknown,
                    None => false,
                }
        }
}
pub fn unconfirmed(request: &Request) -> Value {
    let mut report = Report::new(request);
    report.saving_attempted = true;
    report.outcome_unknown = true;
    report.error_code = Some(Failure::Unconfirmed);
    json!(report)
}
impl crate::NodeService {
    pub async fn save_input(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Saved input targets another node"
        );
        let mut report = Report::new(&request);
        let Some((runtime, _, mut generation)) = self.input_workflow_runtime() else {
            return Ok(json!(report));
        };
        if runtime != request.expected_runtime_id {
            report.error_code = Some(Failure::RuntimeChanged);
            return Ok(json!(report));
        }
        let input = request.command.input()?;
        let mut attempted = false;
        let result = crate::request_input::within_runtime(&mut generation, &runtime, async {
            attempted = true;
            Ok(tokio::time::timeout(
                std::time::Duration::from_secs(15),
                self.submissions.request_correlated(
                    "requests.intent",
                    json!(input),
                    Some(request.command.operation_id.clone()),
                ),
            )
            .await)
        })
        .await;
        report.saving_attempted = attempted;
        report.outcome_unknown = attempted;
        report.error_code = Some(Failure::Unconfirmed);
        match result {
            Err(_) => report.error_code = Some(Failure::RuntimeChanged),
            Ok(Err(_)) => {}
            Ok(Ok(Err(error))) => {
                match error.downcast_ref::<crate::submission::SubmissionFailure>() {
                    Some(crate::submission::SubmissionFailure::NotSent(_)) => {
                        report.outcome_unknown = false;
                        report.error_code = Some(Failure::NotSent);
                    }
                    Some(crate::submission::SubmissionFailure::Rejected(_)) => {
                        report.outcome_unknown = false;
                        report.error_code = Some(Failure::Refused);
                    }
                    None => {}
                }
            }
            Ok(Ok(Ok(value))) if confirms_saved(&value, &self.node_id, &request.command) => {
                report.result = Some(serde_json::from_value(value)?);
                report.success = true;
                report.outcome_unknown = false;
                report.error_code = None;
            }
            _ => {}
        }
        let value = json!(report);
        ensure!(
            confirmed(&value, &request),
            "Input save outcome unconfirmed; inspect the exact operation receipt"
        );
        Ok(value)
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    if let Some(saved) = r.result {
        return Ok(format!(
            "{} input {} ({}). Operation: {}. No model or workload was started.\n",
            if saved.reused {
                "Inspected retained"
            } else {
                "Saved"
            },
            saved.id,
            if saved.status == "cancelled" {
                "cancelled"
            } else {
                "awaiting interpretation"
            },
            r.operation_id
        ));
    }
    if !r.outcome_unknown {
        let reason = match r.error_code {
            Some(Failure::RuntimeUnavailable) => "the node runtime is unavailable",
            Some(Failure::RuntimeChanged) => "the node restarted before sending",
            Some(Failure::NotSent) => "the coordinator connection is unavailable or busy",
            _ => "the coordinator refused the save",
        };
        return Ok(format!(
            "Input was not saved: {reason}. Check the running node and its coordinator connection before trying again. Operation: {}. No model or workload was started.\n",
            r.operation_id
        ));
    }
    Ok(format!(
        "Input save was not confirmed. Inspect jobs find --operation-id {} before sending again. No automatic retry or model call was requested.\n",
        r.operation_id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn request() -> Request {
        Request {
            schema_version: 1,
            expected_node_id: "input-node".into(),
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            command: Command {
                operation_id: uuid::Uuid::new_v4().to_string(),
                original_text: "Can another node interpret this?".into(),
            },
        }
    }
    fn saved(r: &Request) -> Value {
        let context = crate::context::RequestContext::new(
            r.command.operation_id.clone(),
            r.expected_node_id.clone(),
            vec!["bedroom".into()],
        )
        .unwrap();
        let input = r.command.input().unwrap();
        json!(Saved {
            schema_version: 1,
            report_type: "coordinator_request_intent".into(),
            success: true,
            id: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            status: "review".into(),
            operation_id: r.command.operation_id.clone(),
            source_sha256: input.digest(&context).unwrap(),
            request_context: context,
            input,
            reused: false,
            persisted: true,
            interpretation_requested: false,
            execution_authorized: false,
            dispatched: false,
        })
    }
    #[test]
    fn receipts_bind_raw_text_origin_digest_and_non_execution() {
        let r = request();
        let good = saved(&r);
        assert!(confirms_saved(&good, &r.expected_node_id, &r.command));
        for (key, value) in [
            ("source_sha256", json!("0".repeat(64))),
            ("operation_id", json!(uuid::Uuid::new_v4().to_string())),
            ("interpretation_requested", json!(true)),
            ("execution_authorized", json!(true)),
            ("dispatched", json!(true)),
            ("persisted", json!(false)),
            ("revision", json!(0)),
            ("unknown", json!(true)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(!confirms_saved(&bad, &r.expected_node_id, &r.command));
        }
        let mut changed = r.command.clone();
        changed.original_text.push('?');
        assert!(!confirms_saved(&good, &r.expected_node_id, &changed));
        assert!(!confirms_saved(&good, "other-node", &r.command));
        let mut bad = good.clone();
        bad["input"]["original_text"] = json!("Changed");
        assert!(!confirms_saved(&bad, &r.expected_node_id, &r.command));
        let mut bad = good;
        bad["request_context"]["origin_area"] = json!(["elsewhere"]);
        assert!(!confirms_saved(&bad, &r.expected_node_id, &r.command));
    }
    #[test]
    fn raw_input_rejects_unknown_fields_controls_and_encoded_overflow() {
        for text in [
            "".into(),
            " ".into(),
            "x\u{0001}".into(),
            "x".repeat(32769),
            "\"".repeat(32768),
        ] {
            assert!(
                Input {
                    schema_version: 1,
                    original_text: text
                }
                .validate()
                .is_err()
            );
        }
        let input = Input {
            schema_version: 1,
            original_text: "こんにちは 👋".into(),
        };
        input.validate().unwrap();
        assert!(
            serde_json::from_value::<Input>(
                json!({"schema_version":1,"original_text":"hello","origin_node":"forged"})
            )
            .is_err()
        );
        let r = request();
        let unknown = unconfirmed(&r);
        assert!(confirmed(&unknown, &r));
        let mut forged = unknown.clone();
        forged["automatically_retried"] = json!(true);
        assert!(!confirmed(&forged, &r));
        let mut forged = unknown;
        forged["outcome_unknown"] = json!(false);
        assert!(!confirmed(&forged, &r));
    }
    #[tokio::test]
    async fn stopped_or_unconnected_runtime_cannot_claim_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::NodeService::new(dir.path().join("node.json"), "input-node".into());
        let r = request();
        let stopped = service.save_input(r.clone()).await.unwrap();
        assert!(confirmed(&stopped, &r));
        assert_eq!(stopped["error_code"], "runtime_unavailable");
        assert_eq!(stopped["saving_attempted"], false);
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
        let disconnected = service.save_input(r.clone()).await.unwrap();
        assert!(confirmed(&disconnected, &r));
        assert_eq!(disconnected["error_code"], "not_sent");
        assert_eq!(disconnected["outcome_unknown"], false);
        assert!(!dir.path().join("node.json").exists());
    }
    #[tokio::test]
    async fn model_free_runtime_saves_only_exact_raw_submission_and_never_replays() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::NodeService::new(dir.path().join("node.json"), "input-node".into());
        let r = request();
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
        let mut rx = service.submissions.connect();
        let local = service.clone();
        let command = r.clone();
        let task = tokio::spawn(async move { local.save_input(command).await });
        let message = rx.recv().await.unwrap();
        assert_eq!(message.kind, "requests.intent");
        assert_eq!(
            message.operation_id.as_deref(),
            Some(r.command.operation_id.as_str())
        );
        assert_eq!(message.payload, json!(r.command.input().unwrap()));
        message.reply.send(Ok(saved(&r))).unwrap();
        let report = task.await.unwrap().unwrap();
        assert!(confirmed(&report, &r));
        assert_eq!(report["success"], true);
        assert!(render(&report).unwrap().contains("awaiting interpretation"));
        assert!(rx.try_recv().is_err());
        let mut replacement = service.submissions.connect();
        assert!(replacement.try_recv().is_err());
        assert!(!dir.path().join("node.json").exists());
    }
    #[tokio::test]
    async fn refusal_is_known_but_bad_or_lost_ack_is_unknown_without_error_disclosure() {
        for mode in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let service =
                crate::NodeService::new(dir.path().join("node.json"), "input-node".into());
            let r = request();
            let engine =
                Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
            let _guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
            let mut rx = service.submissions.connect();
            let local = service.clone();
            let command = r.clone();
            let task = tokio::spawn(async move { local.save_input(command).await });
            let message = rx.recv().await.unwrap();
            if mode == 0 {
                message
                    .reply
                    .send(Err(crate::submission::SubmissionFailure::Rejected(
                        "private server path".into(),
                    )
                    .into()))
                    .unwrap();
            } else if mode == 1 {
                message
                    .reply
                    .send(Ok(json!({"success":true,"error":"private server path"})))
                    .unwrap();
            } else {
                drop(message);
            }
            let report = task.await.unwrap().unwrap();
            assert!(confirmed(&report, &r));
            assert_eq!(report["success"], false);
            assert_eq!(report["outcome_unknown"], mode != 0);
            assert!(!report.to_string().contains("private server path"));
            assert!(!render(&report).unwrap().contains("private server path"));
            assert!(rx.try_recv().is_err());
        }
    }
    #[tokio::test]
    async fn replacement_with_same_runtime_id_fences_even_a_successful_ack() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::NodeService::new(dir.path().join("node.json"), "input-node".into());
        let r = request();
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
        let mut rx = service.submissions.connect();
        let local = service.clone();
        let command = r.clone();
        let task = tokio::spawn(async move { local.save_input(command).await });
        let message = rx.recv().await.unwrap();
        let _next = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
        drop(guard);
        let _ = message.reply.send(Ok(saved(&r)));
        let report = task.await.unwrap().unwrap();
        assert!(confirmed(&report, &r));
        assert_eq!(report["error_code"], "runtime_changed");
        assert_eq!(report["outcome_unknown"], true);
        assert_eq!(report["operation_id"], r.command.operation_id);
        assert!(report["result"].is_null());
        assert!(rx.try_recv().is_err());
    }
}
