//! Local owner interpretation preview and coordinator review; never execution.
use crate::{NodeEngine, NodeService};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Semaphore, watch};

#[derive(Debug, clap::Subcommand)]
pub enum RequestAction {
    /// Answer an exact saved question; resolve/prepare according to applied policy, never execute.
    Respond {
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        text: String,
        /// Preserve this exact command after uncertainty; reuse only inspects history.
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Interpret/save input and prepare an eligible method according to node policy; never execute.
    Input {
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
        #[arg(long)]
        text: String,
        /// Preserve this receipt after an uncertain reply; reuse only inspects retained history.
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Inspect or save the node's input preparation policy; changes require restart.
    Workflow {
        #[command(subcommand)]
        command: crate::request_input::PolicyAction,
    },
    /// Reconcile one uncertain origin-owned attempt from retained completion; never resume.
    RecoverResult {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        grant_id: String,
        /// Keep this exact receipt after an uncertain reply; no automatic retry.
        #[arg(long)]
        operation_id: String,
    },
    /// Save/compile explicit one-step input without a model; never approve/execute.
    Submit {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "Explicit request")]
        title: String,
        #[arg(long,required_unless_present="template",conflicts_with_all=["template","node"])]
        file: Option<std::path::PathBuf>,
        #[arg(long, required_unless_present = "file")]
        template: Option<String>,
        #[arg(long, requires = "template")]
        node: Option<String>,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Save/compile a literal program/argument request without JSON or a model; never execute.
    SubmitProcess(crate::request_submission::ProcessSubmission),
    /// Propose and save a source-bound method or clarification; never approve/execute.
    Prepare {
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Inspect this origin's retained concrete plan without contacting an executor.
    Plan { id: String },
    /// Ask EEF to compile a reviewed request; separate owner approval is required.
    Compile {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long,required_unless_present="template",conflicts_with_all=["template","node"])]
        file: Option<std::path::PathBuf>,
        /// Parameter-free template (e.g. system_info). Use --file for other inputs.
        #[arg(long, required_unless_present = "file")]
        template: Option<String>,
        #[arg(long, requires = "template")]
        node: Option<String>,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Close this origin's local tracking of uncertain work, not remote effects.
    Abandon {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        grant_id: String,
        #[arg(long, required = true)]
        acknowledge_effects_may_continue: bool,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Inspect a retained request's executor receipt through EEF; never replay or reconcile.
    Receipt {
        id: String,
        #[arg(long)]
        grant_id: String,
    },
    /// Explicitly read retained executor output through EEF; never replay or reconcile.
    Result {
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
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
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
    /// Reinterpret recorded clarification answers once and save a review-only proposal.
    Resolve {
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: Option<String>,
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
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
        #[arg(long)]
        text: String,
    },
    /// Interpret locally and send a proposal to EEF for review; never execute it.
    Review {
        #[command(flatten)]
        inference: crate::inference_budget::InferenceOptions,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_timeout_seconds: Option<u64>,
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
    // EEF separately checked fresh owner-disclosed network names. No grant.
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
    input_policy: crate::request_input::InputPolicy,
}

pub(crate) struct PreviewRuntime {
    binding: Mutex<Option<Binding>>,
    generation: watch::Sender<Option<String>>,
    capacity: Semaphore,
    input_capacity: Arc<Semaphore>,
}
impl Default for PreviewRuntime {
    fn default() -> Self {
        Self {
            binding: Mutex::new(None),
            generation: watch::channel(None).0,
            capacity: Semaphore::new(1),
            input_capacity: Arc::new(Semaphore::new(1)),
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
    pub async fn prepare_request(
        &self,
        request: crate::request_preparation::PrepareRequest,
    ) -> Value {
        use crate::request_planning::{PlanningCommand, PlanningView};
        use crate::request_preparation::{PreparationCheckpoint, PreparationSource};
        let command = &request.command;
        let mut report = json!({"schema_version":1,"report_type":"request_preparation","node_id":self.node_id,
            "runtime_id":null,"command":command,"success":false,"coordinator_acknowledged":false,
            "execution_authorized":false,"dispatched":false,"automatically_retried":false,"outcome_unknown":false,
            "checkpoint_saved":false,"inspection_only":false,"model_inference_requested":false,"saving_attempted":false,
            "coordinator_operation_id":null,"result":null,"error_code":null,
            "note":"Explicit source-bound preparation only. Inspect the retained plan or reuse this exact operation ID after a lost reply. A saved method awaits separate owner approval; a saved question is answerable. No execution or automatic replay."});
        let result: Result<(), &'static str> = async {
            let budget = crate::inference_budget::InferenceBudget::new(request.inference_timeout_seconds)?;
            if request.schema_version != 1 || request.expected_node_id != self.node_id || command.validate().is_err() {
                return Err("invalid_preparation_request");
            }
            let mut generation = self.preview.generation.subscribe();
            let binding = self.preview.binding.lock().unwrap().clone().ok_or("runtime_unavailable")?;
            let engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            report["runtime_id"] = json!(binding.runtime_id);
            if generation.borrow_and_update().as_ref() != Some(&binding.runtime_id) {return Err("runtime_changed");}
            let inspection_id = uuid::Uuid::new_v4().to_string();
            let inspect = PlanningCommand::Inspect {id: command.id.clone()};
            let source = tokio::select! {
                biased;
                _ = generation.changed() => return Err("runtime_changed"),
                result = tokio::time::timeout(Duration::from_secs(5), self.submissions.request_correlated(
                    "requests.planning", json!(inspect), Some(inspection_id.clone()),
                )) => result.map_err(|_| "source_unavailable")?.map_err(|_| "source_unavailable")?,
            };
            if !crate::request_planning::confirmed(&source, &self.node_id, &inspection_id, &inspect) {
                return Err("source_unconfirmed");
            }
            let view: PlanningView = serde_json::from_value(source.clone()).map_err(|_| "source_unconfirmed")?;
            if crate::request_preparation::saved_checkpoint(&view, command).map_err(|_| "preparation_receipt_conflict")?.is_some() {
                report["inspection_only"] = json!(true);
                report["coordinator_operation_id"] = json!(inspection_id);
                report["result"] = source;
            } else {
                if view.input.is_some() || view.status != "review" || view.revision != command.expected_revision {
                    return Err("source_revision_changed");
                }
                crate::request_preparation::messages(&view.draft, &view.draft.current().suggested_capabilities)
                    .map_err(|_| "invalid_preparation_context")?;
                let _permit = self.preview.capacity.try_acquire().map_err(|_| "interpreter_busy")?;
                let capabilities = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = self.network_interpretation_context() => result?,
                };
                report["model_inference_requested"] = json!(true);
                let prepared = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = tokio::time::timeout(budget.duration(), engine.prepare_method_candidate_with_budget(
                        &command.id, command.expected_revision, &view.draft, &capabilities, budget,
                    )) => result.map_err(|_| "preparation_timeout")??,
                };
                let checkpoint = PreparationCheckpoint {schema_version:1, operation_id: command.operation_id.clone(),
                    source: PreparationSource {evidence: prepared.evidence, request_context: view.request_context.clone()},
                    proposal: prepared.proposal};
                checkpoint.confirms_source(&view.draft, &view.request_context, &capabilities).map_err(|_| "method_proposal_invalid")?;
                if checkpoint.proposal.clarification.is_some() && view.draft.proposals.len() >= 17 {
                    return Err("proposal_history_full");
                }
                report["saving_attempted"] = json!(true);
                report["outcome_unknown"] = json!(true);
                let saved = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = tokio::time::timeout(Duration::from_secs(30), self.submissions.request_correlated(
                        "requests.preparation_checkpoint", json!(checkpoint), Some(command.operation_id.clone()),
                    )) => match result {
                        Ok(result) => crate::request_preparation::checkpoint_submission(&mut report, result)?,
                        Err(_) => return Err("preparation_unconfirmed"),
                    },
                };
                if !crate::request_preparation::confirmed_checkpoint(&saved, &self.node_id, &checkpoint) {
                    return Err("preparation_unconfirmed");
                }
                report["coordinator_operation_id"] = json!(command.operation_id);
                report["result"] = saved;
                report["outcome_unknown"] = json!(false);
            }
            report["success"] = json!(true);
            report["coordinator_acknowledged"] = json!(true);
            report["checkpoint_saved"] = json!(true);
            if !crate::request_preparation::confirmed(&report, &self.node_id, command) {
                report["success"] = json!(false); report["coordinator_acknowledged"] = json!(false);
                report["checkpoint_saved"] = json!(false); report["result"] = Value::Null;
                report["outcome_unknown"] = report["saving_attempted"].clone();
                return Err("preparation_unconfirmed");
            }
            Ok(())
        }.await;
        if let Err(code) = result {
            report["error_code"] = json!(code);
        }
        annotate_rejection(&mut report);
        report
    }

    pub async fn resolve_request(
        &self,
        request: crate::request_resolution::ResolveRequest,
    ) -> Value {
        use crate::request_drafts::{DraftCommand, DraftRecord};
        use crate::request_resolution::ResolutionEvidence;
        let command = &request.command;
        let mut report = json!({"schema_version":1,"report_type":"request_resolution","node_id":self.node_id,
            "runtime_id":null,"command":command,"success":false,"coordinator_acknowledged":false,
            "execution_authorized":false,"dispatched":false,"automatically_retried":false,"outcome_unknown":false,
            "proposal_saved":false,"reused":false,"interpretation_source":"node_selected_interpreter",
            "interpretation_context":"coordinator_disclosed_network","saving_attempted":false,"model_inference_requested":false,
            "coordinator_operation_id":null,"result":null,"error_code":null,
            "note":"Explicit clarification reinterpretation only. Preserve the operation ID after a lost reply and inspect the draft before retrying. No plan, approval, grant, execution or automatic replay."});
        let result: Result<(), &'static str> = async {
            let budget = crate::inference_budget::InferenceBudget::new(request.inference_timeout_seconds)?;
            if request.schema_version != 1 || request.expected_node_id != self.node_id || command.validate().is_err() {
                return Err("invalid_resolution_request");
            }
            let mut generation = self.preview.generation.subscribe();
            let binding = self.preview.binding.lock().unwrap().clone().ok_or("runtime_unavailable")?;
            let engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            report["runtime_id"] = json!(binding.runtime_id);
            if generation.borrow_and_update().as_ref() != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            let inspection_id = uuid::Uuid::new_v4().to_string();
            let source = tokio::select! {
                biased;
                _ = generation.changed() => return Err("runtime_changed"),
                result = tokio::time::timeout(Duration::from_secs(5), self.submissions.request_correlated(
                    "requests.draft_get", json!({"id":command.id}), Some(inspection_id.clone()),
                )) => result.map_err(|_| "draft_unavailable")?.map_err(|_| "draft_unavailable")?,
            };
            if !crate::request_drafts::confirmed_report(&source, &self.node_id, &inspection_id, Some(&command.id)) {
                return Err("draft_unconfirmed");
            }
            let draft: DraftRecord = serde_json::from_value(source["draft"].clone()).map_err(|_| "draft_unconfirmed")?;
            if let Some(previous) = draft.proposals.iter().find(|p| p.operation_id == command.operation_id) {
                if previous.resolution.as_ref().is_none_or(|e| e.source_revision != command.expected_revision) {
                    return Err("resolution_receipt_conflict");
                }
                // Inspect a saved resolution even if the interpreter is now
                // missing/busy or this request has since been compiled/executed.
                report["coordinator_operation_id"] = json!(inspection_id);
                report["result"] = source;
                report["reused"] = json!(true);
            } else {
                if draft.answers.iter().any(|a| a.operation_id == command.operation_id) {
                    return Err("resolution_receipt_conflict");
                }
                if source["status"] != "review" || source["revision"].as_u64() != Some(command.expected_revision) {
                    return Err("draft_revision_changed");
                }
                if !draft.resolvable() {
                    return Err("clarification_answer_required");
                }
                if draft.proposals.len() >= 17 {
                    return Err("proposal_history_full");
                }
                // Fail oversized history before capacity, lookup or inference.
                crate::interpretation::clarification_messages(&draft, &[]).map_err(|_| "invalid_clarification_context")?;
                let _permit = self.preview.capacity.try_acquire().map_err(|_| "interpreter_busy")?;
                let capabilities = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = self.network_interpretation_context() => result?,
                };
                report["model_inference_requested"] = json!(true);
                let interpreted = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = tokio::time::timeout(budget.duration(), engine.resolve_interpretation(&draft, &capabilities, budget)) =>
                        result.map_err(|_| "interpretation_timeout")??,
                };
                let proposal: crate::interpretation::Proposal = serde_json::from_value(interpreted["interpretation"]["proposal"].clone())
                    .map_err(|_| "interpretation_invalid")?;
                let evidence = ResolutionEvidence {
                    schema_version: 1, source_revision: command.expected_revision,
                    source_sha256: ResolutionEvidence::source_digest(&draft).map_err(|_| "draft_unconfirmed")?,
                    source_proposal_count: draft.proposals.len(), source_answer_count: draft.answers.len(),
                    backend: interpreted["model"]["backend"].as_str().ok_or("interpretation_invalid")?.into(),
                    model_id: interpreted["model"]["model_id"].as_str().ok_or("interpretation_invalid")?.into(),
                };
                let saving = DraftCommand::Resolve {id: command.id.clone(), expected_revision: command.expected_revision,
                    original_text: draft.original_text.clone(), proposal, evidence};
                let (kind, payload) = saving.submission().map_err(|_| "interpretation_invalid")?;
                report["saving_attempted"] = json!(true);
                report["outcome_unknown"] = json!(true);
                let saved = tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = tokio::time::timeout(Duration::from_secs(30), self.submissions.request_correlated(
                        kind, payload, Some(command.operation_id.clone()),
                    )) => match result {
                        Ok(Ok(data)) => data,
                        Ok(Err(error)) if matches!(error.downcast_ref::<crate::submission::SubmissionFailure>(), Some(crate::submission::SubmissionFailure::NotSent(_))) => {
                            report["outcome_unknown"] = json!(false);
                            return Err("resolution_not_sent");
                        }
                        Ok(Err(error)) if matches!(error.downcast_ref::<crate::submission::SubmissionFailure>(), Some(crate::submission::SubmissionFailure::Rejected(_))) => return Err("resolution_rejected"),
                        _ => return Err("resolution_unconfirmed"),
                    },
                };
                if !crate::request_drafts::confirmed_command(&saved, &self.node_id, &command.operation_id, &saving) {
                    return Err("resolution_unconfirmed");
                }
                let retained: DraftRecord = serde_json::from_value(saved["draft"].clone()).map_err(|_| "resolution_unconfirmed")?;
                if saved["request_context"] != source["request_context"]
                    || retained.original_text != draft.original_text || json!(retained.answers) != json!(draft.answers)
                    || retained.proposals.len() != draft.proposals.len() + 1
                    || json!(&retained.proposals[..draft.proposals.len()]) != json!(draft.proposals)
                {
                    return Err("resolution_unconfirmed");
                }
                report["coordinator_operation_id"] = json!(command.operation_id);
                report["result"] = saved;
                report["outcome_unknown"] = json!(false);
            }
            report["success"] = json!(true);
            report["coordinator_acknowledged"] = json!(true);
            report["proposal_saved"] = json!(true);
            if !crate::request_resolution::confirmed(&report, &self.node_id, command) {
                report["success"] = json!(false);
                report["coordinator_acknowledged"] = json!(false);
                report["proposal_saved"] = json!(false);
                report["result"] = Value::Null;
                report["outcome_unknown"] = report["saving_attempted"].clone();
                return Err("resolution_unconfirmed");
            }
            Ok(())
        }.await;
        if let Err(code) = result {
            report["error_code"] = json!(code);
        }
        annotate_rejection(&mut report);
        report
    }

    pub async fn review_request(&self, request: PreviewRequest) -> Value {
        let generation = self.preview.generation.subscribe();
        let preview = self.prepare_interpretation(request, true).await;
        self.review_prepared(preview, generation, false).await
    }
    pub async fn open_request(&self, request: PreviewRequest) -> Value {
        let generation = self.preview.generation.subscribe();
        let preview = self.prepare_interpretation(request, true).await;
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
            "interpretation_source":"supplied_proposal","interpretation_context":"coordinator_disclosed_network"});
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
            let _engine = binding.engine.upgrade().ok_or("runtime_unavailable")?;
            if generation.borrow().as_ref() != Some(&binding.runtime_id) {
                return Err("runtime_changed");
            }
            prepared["runtime_id"] = json!(binding.runtime_id);
            let proposal =
                serde_json::to_vec(&request.proposal).map_err(|_| "invalid_supplied_proposal")?;
            // Shape only here: the origin need not implement the suggested
            // feature. EEF independently checks its fresh disclosed vocabulary.
            let validated = crate::interpretation::validate(
                &request.text,
                &proposal,
                &request.proposal.suggested_capabilities,
            )
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
        self.attach_preview_runtime_with_policy(engine, runtime_id, Default::default())
    }

    pub fn attach_preview_runtime_with_policy(
        &self,
        engine: &Arc<NodeEngine>,
        runtime_id: String,
        input_policy: crate::request_input::InputPolicy,
    ) -> PreviewGuard {
        let runtime = self.preview.clone();
        {
            let mut binding = runtime.binding.lock().unwrap();
            *binding = Some(Binding {
                runtime_id: runtime_id.clone(),
                engine: Arc::downgrade(engine),
                input_policy,
            });
            runtime.generation.send_replace(Some(runtime_id.clone()));
        }
        PreviewGuard {
            runtime,
            runtime_id,
        }
    }

    pub(crate) fn input_workflow_runtime(
        &self,
    ) -> Option<(
        String,
        crate::request_input::InputPolicy,
        watch::Receiver<Option<String>>,
    )> {
        let binding = self.preview.binding.lock().unwrap();
        let value = binding.as_ref()?;
        value.engine.upgrade()?;
        Some((
            value.runtime_id.clone(),
            value.input_policy,
            self.preview.generation.subscribe(),
        ))
    }

    pub(crate) fn setup_interpreter_runtime(
        &self,
    ) -> Option<(String, Vec<(&'static str, String)>)> {
        let binding = self.preview.binding.lock().unwrap();
        let value = binding.as_ref()?;
        let engine = value.engine.upgrade()?;
        Some((value.runtime_id.clone(), engine.interpreter_candidates()))
    }

    /// Hold the existing runtime binding through a synchronous owner edit.
    /// Retirement/replacement cannot silently substitute a new engine.
    pub(crate) fn with_permission_runtime<T>(
        &self,
        expected: &str,
        inspect: impl FnOnce(&NodeEngine) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let binding = self.preview.binding.lock().unwrap();
        let bound = binding
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Node runtime unavailable"))?;
        if bound.runtime_id != expected {
            anyhow::bail!("Node permission runtime changed")
        }
        let engine = bound
            .engine
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Node runtime unavailable"))?;
        inspect(&engine)
    }

    pub(crate) fn input_workflow_permit(
        &self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, &'static str> {
        self.preview
            .input_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "input_workflow_busy")
    }

    pub async fn preview_request(&self, request: PreviewRequest) -> Value {
        self.prepare_interpretation(request, false).await
    }

    async fn network_interpretation_context(&self) -> Result<Vec<String>, &'static str> {
        let receipt = uuid::Uuid::new_v4().to_string();
        let data = tokio::time::timeout(
            Duration::from_secs(5),
            self.submissions.request_correlated(
                "requests.capabilities",
                json!({}),
                Some(receipt.clone()),
            ),
        )
        .await
        .map_err(|_| "coordinator_context_unavailable")?
        .map_err(|_| "coordinator_context_unavailable")?;
        let context: crate::interpretation::CapabilityContext =
            serde_json::from_value(data).map_err(|_| "coordinator_context_unconfirmed")?;
        if !context.confirms(&self.node_id, &receipt) {
            return Err("coordinator_context_unconfirmed");
        }
        Ok(context.capabilities)
    }

    async fn prepare_interpretation(&self, request: PreviewRequest, network: bool) -> Value {
        let mut runtime_id = None;
        let result: Result<Value, &'static str> = async {
            let budget = crate::inference_budget::InferenceBudget::new(request.inference_timeout_seconds)?;
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
            let capabilities = if network {
                tokio::select! {
                    biased;
                    _ = generation.changed() => return Err("runtime_changed"),
                    result = self.network_interpretation_context() => result?,
                }
            } else {
                let mut names = engine.capabilities();
                names.sort();
                names.dedup();
                names
            };
            let proposal = tokio::select! {
                biased;
                _ = generation.changed() => return Err("runtime_changed"),
                result = tokio::time::timeout(budget.duration(), engine.preview_interpretation(&request.text, &capabilities, budget)) => {
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
            "error":if code.starts_with("coordinator_context_") {
                "EEF interpretation context unavailable or unconfirmed. Check connection and coordinator discovery policy. No model inference, job or execution was requested; no local fallback or automatic retry."
            } else {
                "Preview unavailable or rejected. No job was submitted and no action was authorized; inspect model selection/status before retrying."
            }}),
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
        report["interpretation_context"] = json!(if network {
            "coordinator_disclosed_network"
        } else {
            "local_node"
        });
        annotate_rejection(&mut report);
        if let Some(hint) = interpreter_setup_hint(report["error_code"].as_str().unwrap_or("")) {
            report["setup_hint"] = json!(hint);
        }
        report
    }
}

/// Keep the established public error code; add only a typed, static category.
/// No output fragments, parser errors, model IDs or request contents are exposed.
fn annotate_rejection(report: &mut Value) {
    if report["success"] != false {
        return;
    }
    let Some(reason) = report["error_code"]
        .as_str()
        .and_then(crate::interpretation::RejectionReason::from_engine_code)
    else {
        return;
    };
    report["error_code"] = json!("interpretation_invalid");
    report["rejection_reason"] = json!(reason);
    report["error"] = json!(format!(
        "{} The rejected proposal was not saved or executed. No automatic retry was requested.",
        reason.explanation()
    ));
    report["setup_hint"] = json!(
        "Explicit commands do not require an interpreter. Inspect the selected model with eefn models show before deciding whether to change it; this diagnostic changed no settings."
    );
}

/// Readable adapter guidance, never a command, role selection or fallback.
fn interpreter_setup_hint(code: &str) -> Option<&'static str> {
    match code {
        "interpreter_unavailable" => Some(
            "Inspect selections with eefn models show. Choose an existing text-capable selection using eefn models interpreter --backend BACKEND --model MODEL_ID, then restart this node to apply it. This explicitly assigns the request_interpreter role; provider must activate the chosen backend. No model was chosen or downloaded automatically.",
        ),
        "interpreter_ambiguous" => Some(
            "More than one model has the request_interpreter role. Use eefn models interpreter --backend BACKEND --model MODEL_ID to keep that role on exactly one chosen selection, then restart this node. Other roles and capabilities are preserved. No model was chosen automatically.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejection_annotation_preserves_public_code_and_never_changes_authority_or_payload() {
        let mut failure = json!({"success":false,"error_code":"interpretation_rejected_clarification",
            "execution_authorized":false,"dispatched":false,"automatically_retried":false});
        annotate_rejection(&mut failure);
        assert_eq!(failure["error_code"], "interpretation_invalid");
        assert_eq!(failure["rejection_reason"], "clarification_required");
        assert!(failure["error"].as_str().unwrap().contains("inconsistent"));
        assert_eq!(failure["execution_authorized"], false);
        assert_eq!(failure["dispatched"], false);
        assert_eq!(failure["automatically_retried"], false);
        assert!(failure.get("result").is_none());
        for code in [
            "interpretation_invalid",
            "interpretation_timeout",
            "interpreter_backend_failed",
            "PRIVATE_MODEL_TEXT",
        ] {
            let mut unknown = json!({"success":false,"error_code":code});
            let original = unknown.clone();
            annotate_rejection(&mut unknown);
            assert_eq!(unknown, original);
        }
        let mut success = json!({"success":true,"error_code":"interpretation_rejected_schema"});
        let original = success.clone();
        annotate_rejection(&mut success);
        assert_eq!(success, original);
    }
    #[test]
    fn setup_guidance_does_not_imply_selection_fallback_or_retry() {
        let missing = interpreter_setup_hint("interpreter_unavailable").unwrap();
        assert!(missing.contains("request_interpreter") && missing.contains("restart"));
        let ambiguous = interpreter_setup_hint("interpreter_ambiguous").unwrap();
        assert!(
            ambiguous.contains("exactly one")
                && ambiguous.contains("No model was chosen automatically")
        );
        for code in [
            "",
            "interpretation_timeout",
            "interpreter_backend_failed",
            "runtime_changed",
        ] {
            assert_eq!(interpreter_setup_hint(code), None, "{code}");
        }
    }
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
    async fn supplied_review_rejects_identity_and_malformed_hints_and_does_not_replay_after_restart()
     {
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
        wrong.proposal.suggested_capabilities = vec!["invalid capability name".into()];
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
            inference_timeout_seconds: None,
        }
    }
    #[tokio::test]
    async fn online_context_is_correlated_strict_and_never_falls_back_to_local_inference() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, "runtime".into());
        let mut receiver = service.submissions.connect();
        for mode in [
            "valid",
            "origin",
            "receipt",
            "authority",
            "extra",
            "duplicate",
            "restart",
        ] {
            let local = service.clone();
            let pending = tokio::spawn(async move { local.review_request(request()).await });
            let command = receiver.recv().await.unwrap();
            assert_eq!(command.kind, "requests.capabilities");
            assert_eq!(command.payload, json!({}));
            let receipt = command.operation_id.as_deref().unwrap();
            let mut context = json!(crate::interpretation::CapabilityContext {
                schema_version: 1,
                report_type: "coordinator_interpretation_context".into(),
                request_context: crate::context::RequestContext::new(
                    receipt.into(),
                    "node".into(),
                    vec![]
                )
                .unwrap(),
                capabilities: vec!["http.request".into(), "system.info".into()],
                freshness_seconds: 30,
                execution_authorized: false,
                direct_access_authorized: false,
            });
            match mode {
                "origin" => context["request_context"]["origin_node"] = json!("other"),
                "receipt" => {
                    context["request_context"]["request_id"] =
                        json!(uuid::Uuid::new_v4().to_string())
                }
                "authority" => context["execution_authorized"] = json!(true),
                "extra" => context["private_routes"] = json!([]),
                "duplicate" => context["capabilities"] = json!(["http.request", "http.request"]),
                "restart" => {
                    let replacement = service.attach_preview_runtime(&engine, "new-runtime".into());
                    let report = pending.await.unwrap();
                    assert_eq!(report["error_code"], "runtime_changed");
                    drop(replacement);
                    break;
                }
                _ => {}
            }
            command.reply.send(Ok(context)).unwrap();
            let report = pending.await.unwrap();
            assert_eq!(
                report["error_code"],
                if mode == "valid" {
                    "interpreter_unavailable"
                } else {
                    "coordinator_context_unconfirmed"
                }
            );
            assert_eq!(report["execution_authorized"], false);
            assert_eq!(report["coordinator_acknowledged"], false);
            assert_eq!(
                report["interpretation_context"],
                "coordinator_disclosed_network"
            );
            assert!(receiver.try_recv().is_err());
        }
        drop(guard);
        assert!(!dir.path().join("config.json").exists());
    }
    #[tokio::test]
    async fn supplied_cross_node_hint_reaches_coordinator_without_local_feature_permission() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        assert!(!engine.capabilities().contains(&"http.request".into()));
        let _guard = service.attach_preview_runtime(&engine, "runtime".into());
        let mut receiver = service.submissions.connect();
        let mut request = supplied();
        request.proposal.suggested_capabilities = vec!["http.request".into()];
        let local = service.clone();
        let pending = tokio::spawn(async move { local.review_supplied_proposal(request).await });
        let command = receiver.recv().await.unwrap();
        assert_eq!(command.kind, "requests.review");
        assert_eq!(
            command.payload["proposal"]["suggested_capabilities"],
            json!(["http.request"])
        );
        command
            .reply
            .send(Err(crate::submission::SubmissionFailure::Rejected(
                "Undisclosed feature".into(),
            )
            .into()))
            .unwrap();
        let report = pending.await.unwrap();
        assert_eq!(report["error_code"], "review_rejected");
        assert_eq!(report["dispatched"], false);
        assert!(receiver.try_recv().is_err());
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
        assert_eq!(
            report["setup_hint"],
            interpreter_setup_hint("interpreter_unavailable").unwrap()
        );
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
