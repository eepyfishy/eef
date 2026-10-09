//! Local owner interpretation preview and coordinator review; never execution.
use crate::{NodeEngine, NodeService};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

#[derive(Debug, clap::Subcommand)]
pub enum RequestAction {
    /// Inspect an untrusted proposal from the selected interpreter; never execute it.
    Preview {
        #[arg(long)]
        text: String,
    },
    /// Interpret locally and send a proposal to EEF for review; never execute it.
    Review {
        #[arg(long)]
        text: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub text: String,
}

/// Confirm the review-only response contract in both service and CLI adapters.
/// This checks identity and safety flags, not semantic model understanding.
pub fn confirmed_review(data: &Value, node_id: &str, request_id: &str, text: &str) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reviewed {
        schema_version: u32,
        original_text: String,
        proposal: crate::interpretation::Proposal,
        review_state: String,
        execution_authorized: bool,
        model_output_is_untrusted: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Report {
        schema_version: u32,
        report_type: String,
        request_context: crate::context::RequestContext,
        interpretation: Reviewed,
        execution_authorized: bool,
        dispatched: bool,
        persisted: bool,
        note: String,
    }
    let Ok(report) = serde_json::from_value::<Report>(data.clone()) else {
        return false;
    };
    let reviewed = &report.interpretation;
    let Ok(proposal) = serde_json::to_vec(&reviewed.proposal) else {
        return false;
    };
    // This client-side context only checks bounded shape and internal consistency.
    // EEF separately checked membership against gateway registration. No grant.
    let Ok(validated) =
        crate::interpretation::validate(text, &proposal, &reviewed.proposal.suggested_capabilities)
    else {
        return false;
    };
    let expected_state = match validated.review_state {
        crate::interpretation::ReviewState::ReviewRequired => "review_required",
        crate::interpretation::ReviewState::ClarificationRequired => "clarification_required",
    };
    report.schema_version == 1
        && report.report_type == "coordinator_request_review"
        && report.request_context.origin_node() == node_id
        && report.request_context.request_id() == request_id
        && reviewed.original_text == text
        && reviewed.schema_version == 1
        && reviewed.review_state == expected_state
        && !reviewed.execution_authorized
        && reviewed.model_output_is_untrusted
        && !report.execution_authorized
        && !report.dispatched
        && !report.persisted
        && report.note.len() <= 2048
}

#[derive(Clone)]
struct Binding {
    runtime_id: String,
    engine: Weak<NodeEngine>,
}

pub(crate) struct PreviewRuntime {
    binding: Mutex<Option<Binding>>,
    generation: watch::Sender<Option<String>>,
    capacity: Semaphore,
}
impl Default for PreviewRuntime {
    fn default() -> Self {
        Self {
            binding: Mutex::new(None),
            generation: watch::channel(None).0,
            capacity: Semaphore::new(1),
        }
    }
}

/// Dropped with run_node, including when its future is cancelled for restart.
pub struct PreviewGuard {
    runtime: Arc<PreviewRuntime>,
    runtime_id: String,
}
impl Drop for PreviewGuard {
    fn drop(&mut self) {
        let mut binding = self.runtime.binding.lock().unwrap();
        if binding
            .as_ref()
            .is_some_and(|b| b.runtime_id == self.runtime_id)
        {
            *binding = None;
            self.runtime.generation.send_replace(None);
        }
    }
}

impl NodeService {
    pub async fn review_request(&self, request: PreviewRequest) -> Value {
        let mut generation = self.preview.generation.subscribe();
        let preview = self.preview_request(request).await;
        let mut report = preview.clone();
        report["report_type"] = json!("request_review");
        report["coordinator_acknowledged"] = json!(false);
        report["outcome_unknown"] = json!(false);
        report.as_object_mut().unwrap().remove("result");
        if preview["success"] != true {
            return report;
        }
        if generation.borrow_and_update().as_deref() != preview["runtime_id"].as_str() {
            report["success"] = json!(false);
            report["error_code"] = json!("runtime_changed");
            return report;
        }
        let interpretation = &preview["result"]["interpretation"];
        let payload = json!({"schema_version":1,"original_text":interpretation["original_text"],
            "proposal":interpretation["proposal"]});
        let result = tokio::select! {
            biased;
            _ = generation.changed() => {
                report["success"] = json!(false);
                report["error_code"] = json!("runtime_changed");
                report["outcome_unknown"] = json!(true);
                report["note"] = json!("Node runtime changed while awaiting review. EEF may have reviewed the proposal; no execution was requested and no automatic replay was sent.");
                return report;
            }
            result = self.submissions.request_correlated("requests.review", payload,
                preview["request_id"].as_str().map(str::to_owned)) => result,
        };
        match result {
            Ok(data)
                if confirmed_review(
                    &data,
                    &self.node_id,
                    preview["request_id"].as_str().unwrap(),
                    interpretation["original_text"].as_str().unwrap(),
                ) && data["interpretation"] == *interpretation =>
            {
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = data;
            }
            Err(error)
                if error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                    .is_some() =>
            {
                report["success"] = json!(false);
                report["error_code"] = json!(match error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                {
                    Some(crate::submission::SubmissionFailure::NotSent(_)) =>
                        "coordinator_unavailable",
                    _ => "review_rejected",
                });
                report["error"] = json!(error.to_string());
            }
            _ => {
                report["success"] = json!(false);
                report["error_code"] = json!("review_unconfirmed");
                report["outcome_unknown"] = json!(true);
                report["note"] = json!(
                    "No confirmed review reply. EEF may have reviewed the proposal; no execution was requested and no automatic replay was sent."
                );
            }
        }
        report
    }

    pub fn attach_preview_runtime(
        &self,
        engine: &Arc<NodeEngine>,
        runtime_id: String,
    ) -> PreviewGuard {
        let runtime = self.preview.clone();
        {
            let mut binding = runtime.binding.lock().unwrap();
            *binding = Some(Binding {
                runtime_id: runtime_id.clone(),
                engine: Arc::downgrade(engine),
            });
            runtime.generation.send_replace(Some(runtime_id.clone()));
        }
        PreviewGuard {
            runtime,
            runtime_id,
        }
    }

    pub async fn preview_request(&self, request: PreviewRequest) -> Value {
        let mut runtime_id = None;
        let result: Result<Value, &'static str> = async {
            if request.schema_version != 1 || request.expected_node_id != self.node_id
                || uuid::Uuid::parse_str(&request.request_id).ok().map(|id| id.to_string()).as_deref() != Some(&request.request_id) {
                return Err("invalid_preview_request");
            }
            // Validate user text before acquiring capacity or consulting a model.
            crate::interpretation::messages(&request.text, &[]).map_err(|_| "invalid_preview_input")?;
            let _permit = self.preview.capacity.try_acquire().map_err(|_| "interpreter_busy")?;
            let mut generation = self.preview.generation.subscribe();
            let binding = self.preview.binding.lock().unwrap().clone().ok_or("runtime_unavailable")?;
            runtime_id = Some(binding.runtime_id.clone());
            let engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            if generation.borrow_and_update().as_ref() != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            let proposal = tokio::select! {
                biased;
                _ = generation.changed() => return Err("runtime_changed"),
                result = tokio::time::timeout(Duration::from_secs(20), engine.preview_interpretation(&request.text)) => {
                    result.map_err(|_| "interpretation_timeout")??
                },
            };
            if self.preview.binding.lock().unwrap().as_ref().map(|b| &b.runtime_id) != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            Ok(proposal)
        }.await;
        let mut report = match result {
            Ok(value) => json!({"success":true,"result":value}),
            Err(code) => json!({"success":false,"error_code":code,
                "error":"Preview unavailable or rejected. No job was submitted and no action was authorized; inspect model selection/status before retrying."}),
        };
        report["schema_version"] = json!(1);
        report["report_type"] = json!("request_preview");
        report["node_id"] = json!(self.node_id);
        report["request_id"] = json!(
            uuid::Uuid::parse_str(&request.request_id)
                .ok()
                .map(|id| id.to_string())
                .filter(|id| id == &request.request_id)
        );
        report["runtime_id"] = json!(runtime_id);
        report["execution_authorized"] = json!(false);
        report["dispatched"] = json!(false);
        report["automatically_retried"] = json!(false);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmed_review_rejects_changed_identity_text_and_authority_flags() {
        let value = json!({"schema_version":1,"report_type":"coordinator_request_review",
            "request_context":{"request_id":"receipt","origin_node":"node","origin_area":["Home"]},
            "interpretation":{"schema_version":1,"original_text":"original","execution_authorized":false,"model_output_is_untrusted":true,
                "review_state":"review_required","proposal":{"schema_version":1,"intent":"information","goal":"Check status",
                "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple",
                "workload":"one_shot","needs_clarification":false,"clarification":null}},
            "execution_authorized":false,"dispatched":false,"persisted":false,"note":"Review only"});
        assert!(confirmed_review(&value, "node", "receipt", "original"));
        for (pointer, replacement) in [
            ("/request_context/origin_node", json!("other")),
            ("/request_context/request_id", json!("other")),
            ("/request_context/origin_area", json!(["bad/area"])),
            ("/interpretation/original_text", json!("other")),
            ("/interpretation/execution_authorized", json!(true)),
            ("/interpretation/model_output_is_untrusted", json!(false)),
            ("/interpretation/review_state", json!("approved")),
            ("/interpretation/proposal/goal", json!("x".repeat(2049))),
            ("/interpretation/proposal", json!({})),
            ("/execution_authorized", json!(true)),
            ("/dispatched", json!(true)),
            ("/persisted", json!(true)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                !confirmed_review(&changed, "node", "receipt", "original"),
                "{pointer}"
            );
        }
    }
    fn request() -> PreviewRequest {
        PreviewRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "Check this node".into(),
        }
    }
    #[tokio::test]
    async fn review_failure_never_enqueues_a_submission_or_starts_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let mut receiver = service.submissions.connect();
        let result = service.review_request(request()).await;
        assert_eq!(result["report_type"], "request_review");
        assert_eq!(result["error_code"], "runtime_unavailable");
        assert_eq!(result["coordinator_acknowledged"], false);
        assert_eq!(result["execution_authorized"], false);
        assert_eq!(result["dispatched"], false);
        assert!(result.get("result").is_none());
        assert!(receiver.try_recv().is_err());
        assert!(!dir.path().join("config.json").exists());
    }
    #[test]
    fn preview_binding_does_not_keep_engine_service_cycles_alive() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let weak = Arc::downgrade(&service);
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone()),
        );
        let guard = service.attach_preview_runtime(&engine, "runtime".into());
        drop(service);
        drop(engine);
        assert!(weak.upgrade().is_none());
        drop(guard);
    }
    #[tokio::test]
    async fn preview_validates_target_input_runtime_and_does_not_start_node() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let mut wrong = request();
        wrong.expected_node_id = "other".into();
        assert_eq!(
            service.preview_request(wrong).await["error_code"],
            "invalid_preview_request"
        );
        let mut wrong = request();
        wrong.text = " ".into();
        assert_eq!(
            service.preview_request(wrong).await["error_code"],
            "invalid_preview_input"
        );
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "runtime_unavailable"
        );
        assert!(!dir.path().join("config.json").exists());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, "first".into());
        let report = service.preview_request(request()).await;
        assert_eq!(report["error_code"], "interpreter_unavailable");
        assert_eq!(report["execution_authorized"], false);
        let _permit = service.preview.capacity.acquire().await.unwrap();
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "interpreter_busy"
        );
        drop(_permit);
        let replacement = service.attach_preview_runtime(&engine, "second".into());
        drop(guard);
        assert_eq!(
            service.preview_request(request()).await["runtime_id"],
            "second"
        );
        drop(replacement);
        assert_eq!(
            service.preview_request(request()).await["error_code"],
            "runtime_unavailable"
        );
    }
}
