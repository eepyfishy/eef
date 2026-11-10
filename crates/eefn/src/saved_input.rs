//! Explicit runtime-owned interpretation of an existing saved input. No execution.
use crate::request_interpretation::{
    Checkpoint, Claim, ClaimCommand, Interpreter, PromotionCommand, Record, Request, State,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    id: String,
    revision: u64,
    status: String,
    input: crate::request_intent::Input,
    request_context: crate::context::RequestContext,
    source_sha256: String,
    record: Option<Record>,
}
impl Source {
    pub(crate) fn confirms_input(&self, operation: &str, text: &str) -> bool {
        self.request_context.request_id() == operation && self.input.original_text == text
    }
    fn validate(&self, id: &str, node: &str) -> Result<()> {
        ensure!(
            self.id == id
                && self.revision > 0
                && self.request_context.origin_node() == node
                && self.input.digest(&self.request_context)? == self.source_sha256,
            "Saved input source was not confirmed"
        );
        if let Some(record) = &self.record {
            record.validate_source(id, self.revision, &self.request_context)?;
            ensure!(
                record.input == self.input,
                "Saved input differs from attempt source"
            );
        } else {
            ensure!(
                matches!(self.status.as_str(), "review" | "cancelled"),
                "Saved input is not review-only"
            );
        }
        Ok(())
    }
    pub(crate) fn read(value: &Value, id: &str, node: &str) -> Result<Self> {
        let record = value
            .get("input_interpretation")
            .cloned()
            .map(serde_json::from_value::<Record>)
            .transpose()?;
        if record.is_none() {
            ensure!(
                value["review_only"] == true
                    && value["task_count"] == 0
                    && value["proposal_count"] == 0
                    && value["planning_state"] == "not_interpreted"
                    && value["can_resume"] == false,
                "Input is not an unexecuted raw source"
            );
        }
        let source = Self {
            id: value["id"].as_str().unwrap_or("").into(),
            revision: value["revision"].as_u64().unwrap_or(0),
            status: value["status"].as_str().unwrap_or("").into(),
            input: if let Some(record) = &record {
                record.input.clone()
            } else {
                serde_json::from_value(value["input_intent"].clone())?
            },
            request_context: serde_json::from_value(value["request_context"].clone())?,
            source_sha256: String::new(),
            record,
        };
        let source = Self {
            source_sha256: source.input.digest(&source.request_context)?,
            ..source
        };
        source.validate(id, node)?;
        Ok(source)
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Failure {
    RuntimeUnavailable,
    RuntimeChanged,
    Busy,
    LookupUnconfirmed,
    NotSavedInput,
    Cancelled,
    InterpreterUnavailable,
    InterpreterAmbiguous,
    ContextUnavailable,
    ClaimNotSent,
    ClaimRefused,
    ClaimUnconfirmed,
    ExistingAttempt,
    InferenceUnconfirmed,
    ModelMismatch,
    PromotionUnconfirmed,
    Timeout,
    NetworkInterpreterUnconfirmed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    request_id: String,
    operation_id: String,
    success: bool,
    error_code: Option<Failure>,
    source: Option<Source>,
    checkpoint: Option<Checkpoint>,
    claim_attempted: bool,
    claim_confirmed: bool,
    model_inference_requested: bool,
    promotion_attempted: bool,
    historical_snapshot: bool,
    outcome_unknown: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    #[serde(default)]
    network_interpreter_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network_interpreter: Option<Value>,
}
impl Report {
    fn new(request: &Request) -> Self {
        Self {
            schema_version: 1,
            report_type: "node_saved_input_interpretation".into(),
            node_id: request.expected_node_id.clone(),
            runtime_id: request.expected_runtime_id.clone(),
            request_id: request.command.id.clone(),
            operation_id: request.command.operation_id.clone(),
            success: false,
            error_code: Some(Failure::RuntimeUnavailable),
            source: None,
            checkpoint: None,
            claim_attempted: false,
            claim_confirmed: false,
            model_inference_requested: false,
            promotion_attempted: false,
            historical_snapshot: false,
            outcome_unknown: false,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
            network_interpreter_requested: false,
            network_interpreter: None,
        }
    }
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "node_saved_input_interpretation"
        || r.node_id != request.expected_node_id
        || r.runtime_id != request.expected_runtime_id
        || r.request_id != request.command.id
        || r.operation_id != request.command.operation_id
        || r.execution_authorized
        || r.dispatched
        || r.automatically_retried
    {
        return false;
    }
    if let Some(source) = &r.source {
        if source.validate(&r.request_id, &r.node_id).is_err() {
            return false;
        }
    }
    if let Some(checkpoint) = &r.checkpoint {
        if checkpoint
            .validate(&checkpoint.record.claim, &r.node_id)
            .is_err()
            || checkpoint.id != r.request_id
            || r.source.as_ref().is_none_or(|s| {
                s.input != checkpoint.record.input
                    || s.request_context != checkpoint.record.request_context
            })
        {
            return false;
        }
    }
    if r.network_interpreter_requested {
        return confirmed_network(&r, request);
    }
    if r.network_interpreter.is_some() {
        return false;
    }
    if r.historical_snapshot && r.source.as_ref().is_none_or(|s| s.record.is_none()) {
        return false;
    }
    if r.historical_snapshot
        && r.checkpoint.as_ref().is_some_and(|c| {
            r.source
                .as_ref()
                .and_then(|s| s.record.as_ref())
                .is_none_or(|old| old.claim != c.record.claim)
        })
    {
        return false;
    }
    if r.claim_confirmed && (!r.claim_attempted || r.checkpoint.is_none()) {
        return false;
    }
    if r.model_inference_requested
        && (!r.claim_attempted
            || !r.claim_confirmed
            || r.historical_snapshot
            || r.checkpoint.as_ref().is_none_or(|c| {
                c.record.claim.operation_id != r.operation_id
                    || c.record.claim.origin_runtime_id != r.runtime_id
                    || c.record
                        .claim
                        .interpreter
                        .as_ref()
                        .is_none_or(|p| p.node_id != r.node_id || p.runtime_id != r.runtime_id)
            }))
    {
        return false;
    }
    if r.promotion_attempted && !r.model_inference_requested {
        return false;
    }
    if r.success {
        r.error_code.is_none()
            && !r.outcome_unknown
            && r.checkpoint.as_ref().is_some_and(|c| {
                c.record.state == State::Promoted
                    && if r.historical_snapshot {
                        c.reused
                            && !r.claim_attempted
                            && !r.model_inference_requested
                            && !r.promotion_attempted
                    } else {
                        r.model_inference_requested && r.promotion_attempted && !c.reused
                    }
            })
    } else {
        r.error_code.is_some()
    }
}
fn confirmed_network(r: &Report, request: &Request) -> bool {
    if r.historical_snapshot
        || r.claim_attempted
        || r.claim_confirmed
        || r.model_inference_requested
        || r.promotion_attempted
    {
        return false;
    }
    let Some(source) = &r.source else {
        return false;
    };
    if source.record.is_some() || source.status != "review" {
        return false;
    }
    let Some(receipt) = &r.network_interpreter else {
        return !r.success && r.checkpoint.is_none() && r.error_code.is_some() && r.outcome_unknown;
    };
    let Ok(budget) =
        crate::inference_budget::InferenceBudget::new(request.command.inference_timeout_seconds)
    else {
        return false;
    };
    let command = crate::network_interpretation::request(
        &r.node_id,
        &r.runtime_id,
        &r.request_id,
        crate::interpreter_command::Stage::Initial,
        Some(source.revision),
        &r.operation_id,
        budget,
    );
    if !crate::interpreter_command::confirmed_node(receipt, &command) {
        return false;
    }
    if !r.success {
        return r.error_code.is_some()
            && r.checkpoint.is_none()
            && (r.outcome_unknown || receipt["outcome_unknown"] == false);
    }
    r.error_code.is_none()
        && !r.outcome_unknown
        && receipt["success"] == true
        && receipt["result"]["historical_snapshot"] == false
        && r.checkpoint.as_ref().is_some_and(|c| {
            !c.reused
                && c.record.state == State::Promoted
                && c.record.claim.origin_runtime_id == r.runtime_id
                && c.record.claim.operation_id == r.operation_id
                && c.record.claim.expected_revision == source.revision
                && json!(c) == receipt["result"]["initial_checkpoint"]
        })
}
pub fn unconfirmed(request: &Request) -> Value {
    let mut report = Report::new(request);
    report.error_code = Some(Failure::ClaimUnconfirmed);
    report.outcome_unknown = true;
    json!(report)
}

impl crate::NodeService {
    pub async fn interpret_saved_input(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Saved interpretation targets another node"
        );
        let permit = match self.input_workflow_permit() {
            Ok(permit) => permit,
            Err(_) => {
                let mut report = Report::new(&request);
                report.error_code = Some(Failure::Busy);
                return Ok(json!(report));
            }
        };
        self.interpret_saved_input_admitted(request, &permit).await
    }

    // Normal input already owns this capacity. Borrow its admission rather than
    // acquiring a second slot or bypassing the runtime/model/journal fences.
    pub(crate) async fn interpret_saved_input_admitted(
        &self,
        request: Request,
        admission: &tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Saved interpretation targets another node"
        );
        let mut report = Report::new(&request);
        let Some((runtime, engine, mut generation)) = self.saved_interpreter_runtime() else {
            return Ok(json!(report));
        };
        if runtime != request.expected_runtime_id {
            report.error_code = Some(Failure::RuntimeChanged);
            return Ok(json!(report));
        }
        let budget = crate::inference_budget::InferenceBudget::new(
            request.command.inference_timeout_seconds,
        )
        .expect("validated timing");
        let stages = generation.clone();
        let outcome = crate::request_input::within_runtime(&mut generation, &runtime, async {
            Ok(tokio::time::timeout(
                Duration::from_secs(budget.extend_seconds(75, 1)),
                self.saved_input_inner(&request, &engine, budget, &stages, &mut report, admission),
            )
            .await)
        })
        .await;
        let error = match outcome {
            Err(_) => Some(Failure::RuntimeChanged),
            Ok(Err(_)) => Some(Failure::Timeout),
            Ok(Ok(Err(error))) => Some(error),
            Ok(Ok(Ok(()))) => None,
        };
        if let Some(error) = error {
            report.success = false;
            report.error_code = Some(error);
            report.outcome_unknown |= report.model_inference_requested
                || report.promotion_attempted
                || (report.network_interpreter_requested && report.network_interpreter.is_none())
                || (report.claim_attempted
                    && !matches!(error, Failure::ClaimNotSent | Failure::ClaimRefused));
        }
        let value = json!(report);
        ensure!(
            confirmed(&value, &request),
            "Saved interpretation reply was not confirmed; inspect the request before retrying"
        );
        Ok(value)
    }
    async fn saved_input_inner(
        &self,
        request: &Request,
        engine: &crate::NodeEngine,
        budget: crate::inference_budget::InferenceBudget,
        generation: &tokio::sync::watch::Receiver<Option<String>>,
        report: &mut Report,
        admission: &tokio::sync::OwnedSemaphorePermit,
    ) -> std::result::Result<(), Failure> {
        let lookup = tokio::time::timeout(
            Duration::from_secs(10),
            self.job_command(crate::job_commands::JobRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                operation_id: None,
                command: crate::job_commands::JobCommand::Get {
                    id: request.command.id.clone(),
                },
            }),
        )
        .await
        .map_err(|_| Failure::LookupUnconfirmed)?
        .map_err(|_| Failure::LookupUnconfirmed)?;
        if lookup["success"] != true
            || lookup["acknowledged"] != true
            || lookup["outcome_unknown"] != false
        {
            return Err(Failure::LookupUnconfirmed);
        }
        let source = Source::read(&lookup["data"], &request.command.id, &self.node_id)
            .map_err(|_| Failure::NotSavedInput)?;
        report.source = Some(source.clone());
        if let Some(record) = &source.record {
            report.historical_snapshot = true;
            if record.state != State::Promoted {
                report.outcome_unknown = true;
                return Err(Failure::ExistingAttempt);
            }
            let inspected = self
                .draft_command(crate::request_drafts::DraftRequest {
                    schema_version: 1,
                    expected_node_id: self.node_id.clone(),
                    request_id: request.command.operation_id.clone(),
                    command: crate::request_drafts::DraftCommand::Get {
                        id: source.id.clone(),
                    },
                })
                .await
                .map_err(|_| Failure::LookupUnconfirmed)?;
            if inspected["success"] != true
                || !crate::request_drafts::confirmed_report(
                    &inspected["result"],
                    &self.node_id,
                    &request.command.operation_id,
                    Some(&source.id),
                )
            {
                return Err(Failure::LookupUnconfirmed);
            }
            let checkpoint = Checkpoint {
                schema_version: 1,
                report_type: "coordinator_input_interpretation".into(),
                success: true,
                id: source.id.clone(),
                revision: inspected["result"]["revision"]
                    .as_u64()
                    .ok_or(Failure::LookupUnconfirmed)?,
                status: inspected["result"]["status"]
                    .as_str()
                    .ok_or(Failure::LookupUnconfirmed)?
                    .into(),
                operation_id: record.claim.operation_id.clone(),
                record: record.clone(),
                reused: true,
                draft: Some(
                    serde_json::from_value(inspected["result"]["draft"].clone())
                        .map_err(|_| Failure::LookupUnconfirmed)?,
                ),
                inference_authorized: false,
                execution_authorized: false,
                dispatched: false,
            };
            checkpoint
                .validate(&record.claim, &self.node_id)
                .map_err(|_| Failure::LookupUnconfirmed)?;
            report.checkpoint = Some(checkpoint);
            report.success = true;
            report.error_code = None;
            return Ok(());
        }
        if source.status != "review" {
            return Err(Failure::Cancelled);
        }
        if self
            .input_workflow_runtime()
            .is_some_and(|(runtime, policy, _)| {
                runtime == request.expected_runtime_id && policy.network_interpreter
            })
        {
            return self
                .saved_network_input(request, &source, budget, report, admission)
                .await;
        }
        let candidates = engine.interpreter_candidates();
        if candidates.is_empty() {
            return Err(Failure::InterpreterUnavailable);
        }
        if candidates.len() != 1 {
            return Err(Failure::InterpreterAmbiguous);
        }
        let _inference_permit = self.saved_interpreter_permit().map_err(|_| Failure::Busy)?;
        let capabilities = self
            .network_interpretation_context()
            .await
            .map_err(|_| Failure::ContextUnavailable)?;
        let claim = Claim {
            id: source.id.clone(),
            expected_revision: source.revision,
            source_sha256: source.source_sha256.clone(),
            operation_id: request.command.operation_id.clone(),
            origin_runtime_id: request.expected_runtime_id.clone(),
            interpreter: Some(Interpreter {
                node_id: self.node_id.clone(),
                backend: candidates[0].0.into(),
                model_id: candidates[0].1.clone(),
                runtime_id: request.expected_runtime_id.clone(),
            }),
        };
        current_runtime(generation, &request.expected_runtime_id)?;
        report.claim_attempted = true;
        let checkpoint = self
            .saved_checkpoint(
                "requests.interpretation_claim",
                json!(ClaimCommand {
                    schema_version: 1,
                    claim: claim.clone()
                }),
                &claim,
            )
            .await?;
        report.claim_confirmed = true;
        report.checkpoint = Some(checkpoint.clone());
        if checkpoint.reused
            || checkpoint.record.state != State::Claimed
            || checkpoint.status != "review"
        {
            report.outcome_unknown = true;
            return Err(Failure::ExistingAttempt);
        }
        current_runtime(generation, &request.expected_runtime_id)?;
        report.model_inference_requested = true;
        let candidate = tokio::time::timeout(
            budget.duration(),
            engine.preview_interpretation(&source.input.original_text, &capabilities, budget),
        )
        .await
        .map_err(|_| Failure::InferenceUnconfirmed)?
        .map_err(|_| Failure::InferenceUnconfirmed)?;
        let interpreter = claim.interpreter.as_ref().unwrap();
        if candidate["model"]["backend"] != interpreter.backend
            || candidate["model"]["model_id"] != interpreter.model_id
        {
            return Err(Failure::ModelMismatch);
        }
        let proposal = serde_json::from_value(candidate["interpretation"]["proposal"].clone())
            .map_err(|_| Failure::InferenceUnconfirmed)?;
        let promotion = PromotionCommand {
            schema_version: 1,
            claim: claim.clone(),
            expected_revision: checkpoint.revision,
            proposal,
        };
        promotion
            .validate()
            .map_err(|_| Failure::PromotionUnconfirmed)?;
        current_runtime(generation, &request.expected_runtime_id)?;
        report.promotion_attempted = true;
        let promoted = self
            .saved_checkpoint("requests.interpretation_promote", json!(promotion), &claim)
            .await
            .map_err(|_| Failure::PromotionUnconfirmed)?;
        if promoted.record.state != State::Promoted || promoted.reused {
            return Err(Failure::PromotionUnconfirmed);
        }
        report.checkpoint = Some(promoted);
        report.success = true;
        report.error_code = None;
        Ok(())
    }
    async fn saved_network_input(
        &self,
        request: &Request,
        source: &Source,
        budget: crate::inference_budget::InferenceBudget,
        report: &mut Report,
        admission: &tokio::sync::OwnedSemaphorePermit,
    ) -> std::result::Result<(), Failure> {
        let command = crate::network_interpretation::request(
            &self.node_id,
            &request.expected_runtime_id,
            &source.id,
            crate::interpreter_command::Stage::Initial,
            Some(source.revision),
            &request.command.operation_id,
            budget,
        );
        report.network_interpreter_requested = true;
        report.outcome_unknown = true;
        let receipt = self
            .network_interpretation(command, Some(admission))
            .await
            .map_err(|_| Failure::NetworkInterpreterUnconfirmed)?;
        report.outcome_unknown = receipt["outcome_unknown"] == true;
        report.network_interpreter = Some(receipt.clone());
        if receipt["success"] != true {
            return Err(Failure::NetworkInterpreterUnconfirmed);
        }
        // A concurrent retained result is for inspection, never a fresh input
        // continuation. Repeating the outer receipt will inspect ordinary history.
        if receipt["result"]["historical_snapshot"] != false {
            return Err(Failure::ExistingAttempt);
        }
        report.outcome_unknown = true;
        let checkpoint: Checkpoint =
            serde_json::from_value(receipt["result"]["initial_checkpoint"].clone())
                .map_err(|_| Failure::NetworkInterpreterUnconfirmed)?;
        checkpoint
            .validate(&checkpoint.record.claim, &self.node_id)
            .map_err(|_| Failure::NetworkInterpreterUnconfirmed)?;
        if checkpoint.reused
            || checkpoint.record.state != State::Promoted
            || checkpoint.record.input != source.input
            || checkpoint.record.request_context != source.request_context
            || checkpoint.record.claim.expected_revision != source.revision
            || checkpoint.record.claim.source_sha256 != source.source_sha256
            || checkpoint.record.claim.origin_runtime_id != request.expected_runtime_id
            || checkpoint.record.claim.operation_id != request.command.operation_id
        {
            return Err(Failure::NetworkInterpreterUnconfirmed);
        }
        report.checkpoint = Some(checkpoint);
        report.outcome_unknown = false;
        report.success = true;
        report.error_code = None;
        Ok(())
    }
    async fn saved_checkpoint(
        &self,
        kind: &str,
        payload: Value,
        claim: &Claim,
    ) -> std::result::Result<Checkpoint, Failure> {
        let response = tokio::time::timeout(
            Duration::from_secs(15),
            self.submissions
                .request_correlated(kind, payload, Some(claim.operation_id.clone())),
        )
        .await
        .map_err(|_| Failure::ClaimUnconfirmed)?;
        let value = response.map_err(|error| {
            match error.downcast_ref::<crate::submission::SubmissionFailure>() {
                Some(crate::submission::SubmissionFailure::NotSent(_)) => Failure::ClaimNotSent,
                Some(crate::submission::SubmissionFailure::Rejected(_)) => Failure::ClaimRefused,
                None => Failure::ClaimUnconfirmed,
            }
        })?;
        let checkpoint: Checkpoint =
            serde_json::from_value(value).map_err(|_| Failure::ClaimUnconfirmed)?;
        checkpoint
            .validate(claim, &self.node_id)
            .map_err(|_| Failure::ClaimUnconfirmed)?;
        Ok(checkpoint)
    }
}

pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    if report.success {
        let checkpoint = report
            .checkpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing saved interpretation"))?;
        let mut output = format!(
            "{} for request {} ({}). Preparation, owner approval and execution remain separate.\n",
            if report.historical_snapshot {
                "Inspected retained interpretation"
            } else {
                "Interpreted saved input"
            },
            report.request_id,
            checkpoint.status
        );
        if checkpoint.status == "review" {
            if let Some(draft) = checkpoint
                .draft
                .as_ref()
                .filter(|d| d.state() == "clarification_required")
            {
                if let Some(question) = &draft.current().clarification {
                    output.push_str(&format!("Question: {question}\n"));
                }
            }
        }
        return Ok(output);
    }
    if matches!(
        report.error_code,
        Some(Failure::InterpreterUnavailable | Failure::InterpreterAmbiguous)
    ) {
        return Ok(format!(
            "Input {} remains saved. Choose exactly one request interpreter with `eefn models interpreter --backend BACKEND --model MODEL`, restart this node to apply it, then use `eefn requests interpret-saved {}`. No model call or tool execution was requested.\n",
            report.request_id, report.request_id
        ));
    }
    Ok(format!(
        "Interpretation of saved input {} was not confirmed. Inspect `eefn jobs get {}` before deciding what to do. {}No automatic retry or tool execution was requested.\n",
        report.request_id,
        report.request_id,
        if report.outcome_unknown {
            "An earlier attempt may have run; it will not be repeated automatically. "
        } else {
            ""
        }
    ))
}

fn current_runtime(
    generation: &tokio::sync::watch::Receiver<Option<String>>,
    expected: &str,
) -> std::result::Result<(), Failure> {
    if generation.has_changed().unwrap_or(true) || generation.borrow().as_deref() != Some(expected)
    {
        Err(Failure::RuntimeChanged)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "saved_input_tests.rs"]
mod tests;
