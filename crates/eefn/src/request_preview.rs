//! Local owner interpretation preview and coordinator review; never execution.
use crate::{NodeEngine, NodeService};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

#[derive(Debug, clap::Subcommand)]
pub enum RequestAction {
    /// Inspect a retained request's executor receipt through EEF; never replay or reconcile.
    Receipt {
        id: String,
        #[arg(long)]
        grant_id: String,
    },
    /// Ask EEF to execute one exact owner-approved request from this running node.
    Execute {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        approval_digest: String,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Interpret and save an origin-owned durable review; never execute it.
    Open {
        #[arg(long)]
        text: String,
    },
    /// Save a supplied proposal as a durable review without calling a model.
    OpenProposal {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    /// Inspect a durable review owned by this node, including original text/answers.
    Draft { id: String },
    /// Record a clarification answer against the inspected revision; never guess a resolution.
    Answer {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        text: String,
    },
    /// Append a revised supplied proposal, preserving the original request and answers.
    ReviseProposal {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        file: std::path::PathBuf,
    },
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
    /// Send a bounded structured proposal for review without calling a model.
    ReviewProposal {
        #[arg(long)]
        file: std::path::PathBuf,
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedProposal {
    pub schema_version: u32,
    pub original_text: String,
    pub proposal: crate::interpretation::Proposal,
}

pub fn read_proposal(path: &std::path::Path) -> anyhow::Result<SuppliedProposal> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        anyhow::bail!("proposal file exceeds bounds")
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let value: SuppliedProposal = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("invalid supplied proposal schema"))?;
    if value.schema_version != 1 {
        anyhow::bail!("unsupported supplied proposal version")
    }
    // Local file shape validation only. Trusted runtime/EEF contexts independently
    // check membership; a file cannot grant its own capabilities or authority.
    crate::interpretation::validate(
        &value.original_text,
        &serde_json::to_vec(&value.proposal)?,
        &value.proposal.suggested_capabilities,
    )?;
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalReviewRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub text: String,
    pub proposal: crate::interpretation::Proposal,
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
        let generation = self.preview.generation.subscribe();
        let preview = self.preview_request(request).await;
        self.review_prepared(preview, generation, false).await
    }
    pub async fn open_request(&self, request: PreviewRequest) -> Value {
        let generation = self.preview.generation.subscribe();
        let preview = self.preview_request(request).await;
        self.review_prepared(preview, generation, true).await
    }

    pub async fn review_supplied_proposal(&self, request: ProposalReviewRequest) -> Value {
        self.prepare_supplied_proposal(request, false).await
    }
    pub async fn open_supplied_proposal(&self, request: ProposalReviewRequest) -> Value {
        self.prepare_supplied_proposal(request, true).await
    }
    async fn prepare_supplied_proposal(
        &self,
        request: ProposalReviewRequest,
        persist: bool,
    ) -> Value {
        let generation = self.preview.generation.subscribe();
        let canonical_id = uuid::Uuid::parse_str(&request.request_id)
            .ok()
            .map(|id| id.to_string())
            .filter(|id| id == &request.request_id);
        let mut prepared = json!({"schema_version":1,"report_type":"request_preview","success":false,
            "node_id":self.node_id,"request_id":canonical_id,"runtime_id":null,
            "execution_authorized":false,"dispatched":false,"automatically_retried":false,
            "interpretation_source":"supplied_proposal"});
        let result: Result<Value, &'static str> = (|| {
            if request.schema_version != 1
                || request.expected_node_id != self.node_id
                || uuid::Uuid::parse_str(&request.request_id)
                    .ok()
                    .map(|id| id.to_string())
                    .as_deref()
                    != Some(&request.request_id)
            {
                return Err("invalid_proposal_request");
            }
            let binding = self
                .preview
                .binding
                .lock()
                .unwrap()
                .clone()
                .ok_or("runtime_unavailable")?;
            let engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            if generation.borrow().as_ref() != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            prepared["runtime_id"] = json!(binding.runtime_id);
            let proposal =
                serde_json::to_vec(&request.proposal).map_err(|_| "invalid_supplied_proposal")?;
            let validated =
                crate::interpretation::validate(&request.text, &proposal, &engine.capabilities())
                    .map_err(|_| "invalid_supplied_proposal")?;
            Ok(json!({"interpretation":validated}))
        })();
        match result {
            Ok(result) => {
                prepared["success"] = json!(true);
                prepared["result"] = result;
            }
            Err(code) => {
                prepared["error_code"] = json!(code);
                prepared["error"] = json!(
                    "Proposal rejected or runtime unavailable. No model inference, job or execution was requested."
                );
            }
        }
        self.review_prepared(prepared, generation, persist).await
    }

    async fn review_prepared(
        &self,
        preview: Value,
        mut generation: watch::Receiver<Option<String>>,
        persist: bool,
    ) -> Value {
        let mut report = preview.clone();
        report["report_type"] = json!(if persist {
            "request_draft"
        } else {
            "request_review"
        });
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
                report["note"] = json!(if persist {
                    "Node runtime changed while awaiting save. EEF may have persisted the draft; inspect jobs.find with this receipt. No execution or automatic replay was requested."
                } else {
                    "Node runtime changed while awaiting review. EEF may have reviewed the proposal; no execution was requested and no automatic replay was sent."
                });
                return report;
            }
            result = self.submissions.request_correlated(if persist {"requests.open"} else {"requests.review"}, payload,
                preview["request_id"].as_str().map(str::to_owned)) => result,
        };
        match result {
            Ok(data)
                if if persist {
                    crate::request_drafts::confirmed_report(
                        &data,
                        &self.node_id,
                        preview["request_id"].as_str().unwrap(),
                        None,
                    ) && data["request_context"]["request_id"] == preview["request_id"]
                        && data["draft"]["original_text"] == interpretation["original_text"]
                        && data["draft"]["proposals"][0]["proposal"] == interpretation["proposal"]
                } else {
                    confirmed_review(
                        &data,
                        &self.node_id,
                        preview["request_id"].as_str().unwrap(),
                        interpretation["original_text"].as_str().unwrap(),
                    ) && data["interpretation"] == *interpretation
                } =>
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
                report["outcome_unknown"] = json!(
                    persist
                        && !matches!(
                            error.downcast_ref::<crate::submission::SubmissionFailure>(),
                            Some(crate::submission::SubmissionFailure::NotSent(_))
                        )
                );
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
        if persist {
            report["note"] = json!(
                "Durable review only, never executable work. After an unconfirmed save inspect jobs.find with this request ID; do not replay automatically."
            );
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
    fn supplied() -> ProposalReviewRequest {
        serde_json::from_value(json!({"schema_version":1,"expected_node_id":"node",
            "request_id":uuid::Uuid::new_v4().to_string(),"text":"  Check status\n",
            "proposal":{"schema_version":1,"intent":"information","goal":"Check status",
                "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],
                "complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null}})).unwrap()
    }
    #[test]
    fn supplied_files_are_strict_bounded_and_never_import_authority() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proposal.json");
        let request = supplied();
        let value =
            json!({"schema_version":1,"original_text":request.text,"proposal":request.proposal});
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            read_proposal(&path).unwrap().original_text,
            "  Check status\n"
        );
        for bad in [
            "request_context",
            "origin_node",
            "permissions",
            "execution_authorized",
            "command",
        ] {
            let mut input = value.clone();
            input[bad] = json!(true);
            std::fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
            assert!(read_proposal(&path).is_err());
            let mut input = value.clone();
            input["proposal"][bad] = json!(true);
            std::fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
            assert!(read_proposal(&path).is_err());
        }
        std::fs::write(&path, vec![b'x'; 64 * 1024 + 1]).unwrap();
        assert!(read_proposal(&path).is_err());
    }
    #[tokio::test]
    async fn supplied_review_needs_no_model_and_uses_same_confirmed_review_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, "runtime".into());
        let mut receiver = service.submissions.connect();
        let request = supplied();
        let id = request.request_id.clone();
        // Holding all inference capacity cannot block the deterministic adapter.
        let _permit = service.preview.capacity.acquire().await.unwrap();
        let local = service.clone();
        let pending = tokio::spawn(async move { local.review_supplied_proposal(request).await });
        let command = receiver.recv().await.unwrap();
        assert_eq!(command.kind, "requests.review");
        assert_eq!(command.operation_id.as_deref(), Some(id.as_str()));
        let validated = crate::interpretation::validate(
            command.payload["original_text"].as_str().unwrap(),
            &serde_json::to_vec(&command.payload["proposal"]).unwrap(),
            &["system.info".into()],
        )
        .unwrap();
        command.reply.send(Ok(json!({"schema_version":1,"report_type":"coordinator_request_review",
            "request_context":crate::context::RequestContext::new(id,"node".into(),vec![]).unwrap(),
            "interpretation":validated,"execution_authorized":false,"dispatched":false,"persisted":false,"note":"Review only"}))).unwrap();
        let report = pending.await.unwrap();
        assert_eq!(report["success"], true);
        assert_eq!(report["coordinator_acknowledged"], true);
        assert_eq!(report["interpretation_source"], "supplied_proposal");
        assert_eq!(
            report["result"]["interpretation"]["original_text"],
            "  Check status\n"
        );
        assert_eq!(report["execution_authorized"], false);
        assert_eq!(report["dispatched"], false);
        assert!(!dir.path().join("config.json").exists());
    }
    #[tokio::test]
    async fn durable_open_uses_one_bound_mailbox_save_and_rejects_changed_proposal() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, "runtime".into());
        let mut receiver = service.submissions.connect();
        let _permit = service.preview.capacity.acquire().await.unwrap();
        for mode in ["good", "changed", "lost"] {
            let request = supplied();
            let id = request.request_id.clone();
            let local = service.clone();
            let future = tokio::spawn(async move { local.open_supplied_proposal(request).await });
            let command = receiver.recv().await.unwrap();
            assert_eq!(command.kind, "requests.open");
            assert_eq!(command.operation_id.as_deref(), Some(id.as_str()));
            let mut proposal = command.payload["proposal"].clone();
            if mode == "changed" {
                proposal["goal"] = json!("Other valid goal");
            }
            if mode == "lost" {
                drop(command.reply);
            } else {
                command.reply.send(Ok(json!({"schema_version":1,"report_type":"coordinator_request_draft","id":uuid::Uuid::new_v4().simple().to_string(),
                    "revision":1,"status":"review","operation_id":id,"request_context":{"request_id":id,"origin_node":"node","origin_area":[]},
                    "draft":{"schema_version":1,"original_text":command.payload["original_text"],"proposals":[{"operation_id":id,"proposal":proposal,
                        "allowed_capabilities":["system.info"],"at_ms":1}],"answers":[]},
                    "review_state":"review_required","reused":false,"execution_authorized":false,"dispatched":false,"persisted":true,"note":"Review only"}))).unwrap();
            }
            let report = future.await.unwrap();
            assert_eq!(report["report_type"], "request_draft");
            assert_eq!(report["success"], mode == "good");
            assert_eq!(report["outcome_unknown"], mode != "good");
            assert_eq!(report["execution_authorized"], false);
            assert!(receiver.try_recv().is_err());
        }
        let local = service.clone();
        let future = tokio::spawn(async move { local.open_supplied_proposal(supplied()).await });
        let command = receiver.recv().await.unwrap();
        drop(guard);
        let result = future.await.unwrap();
        assert_eq!(result["error_code"], "runtime_changed");
        assert_eq!(result["outcome_unknown"], true);
        assert!(result["note"].as_str().unwrap().contains("persisted"));
        assert!(receiver.try_recv().is_err());
        drop(command);
        assert!(!dir.path().join("config.json").exists());
    }
    #[tokio::test]
    async fn supplied_review_rejects_identity_and_capabilities_and_does_not_replay_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, "runtime".into());
        let mut receiver = service.submissions.connect();
        let mut wrong = supplied();
        wrong.expected_node_id = "other".into();
        assert_eq!(
            service.review_supplied_proposal(wrong).await["error_code"],
            "invalid_proposal_request"
        );
        let mut wrong = supplied();
        wrong.proposal.suggested_capabilities = vec!["invented.capability".into()];
        assert_eq!(
            service.review_supplied_proposal(wrong).await["error_code"],
            "invalid_supplied_proposal"
        );
        assert!(receiver.try_recv().is_err());
        let local = service.clone();
        let pending = tokio::spawn(async move { local.review_supplied_proposal(supplied()).await });
        let command = receiver.recv().await.unwrap();
        drop(guard);
        let report = pending.await.unwrap();
        assert_eq!(report["error_code"], "runtime_changed");
        assert_eq!(report["outcome_unknown"], true);
        assert_eq!(report["execution_authorized"], false);
        assert!(receiver.try_recv().is_err());
        drop(command);
    }
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
