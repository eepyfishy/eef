//! Explicit owner reconciliation of already-retained semantic data; no execution.
use crate::{
    interpreter_allowance::{Phase, Record as AllowanceRecord},
    task::{Plan, PlanStatus},
};
use anyhow::{Context, Result, ensure};
use eefn::{
    interpreter_recovery::{Command, Record, RecoveryActor},
    interpreter_task::{Candidate, Source},
    request_preparation::PreparationSource,
};
use serde_json::json;

pub(crate) fn validate_history(plan: &Plan) -> Result<()> {
    let history = &plan.interpreter_recoveries;
    ensure!(
        history.len() <= eefn::interpreter_recovery::MAX_RECOVERIES,
        "Interpreter recovery history exceeds its bound"
    );
    let mut revision = 0;
    let mut attempts = std::collections::BTreeSet::new();
    let mut operations = std::collections::BTreeSet::new();
    for record in history {
        record.validate()?;
        ensure!(
            record.command.id == plan.plan_id
                && record.applied_revision <= plan.revision
                && record.applied_revision > revision
                && attempts.insert(&record.command.grant_id)
                && operations.insert(&record.command.operation_id)
                && plan
                    .goal
                    .request_context
                    .as_ref()
                    .is_none_or(|c| c.request_id() != record.command.operation_id)
                && !crate::request_execution::operation_used(plan, &record.command.operation_id),
            "Interpreter recovery history is inconsistent"
        );
        revision = record.applied_revision;
    }
    Ok(())
}

pub(crate) fn initial_claim(
    record: &AllowanceRecord,
) -> Result<eefn::request_interpretation::Claim> {
    let task = record.task()?;
    let Source::Initial { input } = &task.snapshot.source else {
        anyhow::bail!("Recovery does not describe initial input")
    };
    let grant = &record.binding.grant;
    Ok(eefn::request_interpretation::Claim {
        id: grant.job_id.clone(),
        expected_revision: task.snapshot.revision,
        source_sha256: input.digest(&task.snapshot.request_context)?,
        operation_id: task.snapshot.operation_id,
        origin_runtime_id: record.binding.origin_runtime_id.clone(),
        interpreter: Some(eefn::request_interpretation::Interpreter {
            node_id: grant.executor_node.clone(),
            backend: grant
                .backend
                .clone()
                .context("Missing interpreter backend")?,
            model_id: grant
                .model_id
                .clone()
                .context("Missing interpreter model")?,
            runtime_id: record.binding.provider_runtime_id.clone(),
        }),
    })
}

pub(crate) fn completed(record: &AllowanceRecord) -> Result<()> {
    record.validate()?;
    let report = record
        .report
        .as_ref()
        .context("Retained interpreter result unavailable")?;
    ensure!(
        record.phase == Phase::Final
            && !record.holds_slot()
            && record.stop.is_none()
            && report.success
            && report.state == eefn::grants::ReceiptState::Completed
            && report.confirms(&record.binding.grant),
        "Recovery requires an exact completed result without uncertain work or stop intent"
    );
    Ok(())
}
pub(crate) fn validate_capabilities(
    original: &AllowanceRecord,
    capabilities: &[String],
) -> Result<()> {
    completed(original)?;
    let task = original.task()?;
    let proposal = serde_json::to_vec(
        original
            .report
            .as_ref()
            .context("Missing result")?
            .data
            .pointer("/candidate/candidate/proposal")
            .context("Missing semantic proposal")?,
    )?;
    match &task.snapshot.source {
        Source::Initial { input } => {
            eefn::interpretation::validate(&input.original_text, &proposal, capabilities)?;
        }
        Source::Clarification { draft } => {
            eefn::interpretation::validate(&draft.original_text, &proposal, capabilities)?;
        }
        Source::Method { draft } => {
            eefn::request_preparation::validate(draft, capabilities, &proposal)?;
        }
    }
    Ok(())
}

pub(crate) fn eligible(
    plan: &Plan,
    command: &Command,
    actor: RecoveryActor,
    origin: Option<&str>,
    original: &AllowanceRecord,
    unavailable: bool,
) -> Result<bool> {
    command.validate()?;
    completed(original)?;
    validate_history(plan)?;
    let task = original.task()?;
    ensure!(
        plan.plan_id == command.id
            && original.id() == command.grant_id
            && task.snapshot.job_id == command.id
            && plan.goal.request_context.as_ref() == Some(&task.snapshot.request_context)
            && origin.is_none_or(|n| n == task.snapshot.request_context.origin_node())
            && actor
                == (if origin.is_some() {
                    RecoveryActor::OriginNode
                } else {
                    RecoveryActor::CoordinatorOwner
                }),
        "Interpreter recovery is unavailable for this origin"
    );
    if let Some(old) = plan
        .interpreter_recoveries
        .iter()
        .find(|r| r.command.operation_id == command.operation_id)
    {
        ensure!(
            old.command == *command && old.actor == actor,
            "Recovery receipt conflicts with retained history"
        );
        validate_binding(plan, old, original)?;
        return Ok(true);
    }
    ensure!(
        !plan
            .interpreter_recoveries
            .iter()
            .any(|r| r.command.grant_id == command.grant_id)
            && plan.interpreter_recoveries.len() < eefn::interpreter_recovery::MAX_RECOVERIES
            && command.operation_id != task.snapshot.operation_id
            && command.operation_id != task.snapshot.request_context.request_id(),
        "Interpreter result already recovered or recovery operation conflicts"
    );
    ensure!(
        !unavailable
            && plan.status == PlanStatus::Review
            && plan.revision == command.expected_revision
            && plan.planning.is_none()
            && plan.approval.is_none()
            && plan.execution.is_none()
            && plan.tasks.is_empty()
            && plan.results.is_empty(),
        "Recovery needs the exact idle unplanned source revision"
    );
    if let Some(draft) = &plan.request_review {
        ensure!(
            !draft
                .proposals
                .iter()
                .any(|p| p.operation_id == command.operation_id)
                && !draft
                    .answers
                    .iter()
                    .any(|a| a.operation_id == command.operation_id),
            "Recovery operation already belongs to source history"
        );
    }
    match &task.snapshot.source {
        Source::Initial { input } => {
            let expected = initial_claim(original)?;
            ensure!(
                crate::request_intent::validate(plan)? == *input
                    && plan
                        .input_interpretation
                        .as_ref()
                        .is_some_and(|r| r.state == eefn::request_interpretation::State::Unknown
                            && r.claim == expected),
                "Initial source is changed or is not explicitly uncertain"
            );
        }
        Source::Clarification { draft } | Source::Method { draft } => {
            ensure!(
                plan.revision == task.snapshot.revision
                    && plan
                        .request_review
                        .as_ref()
                        .is_some_and(|saved| json!(saved) == json!(draft)),
                "Recovery source changed after interpretation"
            );
        }
    }
    Ok(false)
}

pub(crate) fn receipt(
    command: &Command,
    actor: RecoveryActor,
    original: &AllowanceRecord,
    capabilities: &[String],
) -> Result<Record> {
    completed(original)?;
    let report = original.report.as_ref().unwrap();
    let task = original.task()?;
    let record = Record {
        schema_version: 1,
        command: command.clone(),
        actor,
        stage: match task.snapshot.source {
            Source::Initial { .. } => eefn::interpreter_command::Stage::Initial,
            Source::Clarification { .. } => eefn::interpreter_command::Stage::Clarification,
            Source::Method { .. } => eefn::interpreter_command::Stage::Method,
        },
        source_revision: task.snapshot.revision,
        applied_revision: command
            .expected_revision
            .checked_add(1)
            .context("Recovery revision overflow")?,
        decided_at_ms: eefn::protocol::now_ms().max(original.binding.reserved_at_ms),
        binding_sha256: original.binding_sha256.clone(),
        report_sha256: eefn::grants::definition_digest(&json!(report))?,
        proposal_sha256: eefn::grants::definition_digest(
            report
                .data
                .pointer("/candidate/candidate/proposal")
                .context("Missing semantic result")?,
        )?,
        capabilities_sha256: eefn::grants::definition_digest(&json!(capabilities))?,
    };
    record.validate()?;
    Ok(record)
}

pub(crate) fn validate_binding(
    plan: &Plan,
    recovery: &Record,
    original: &AllowanceRecord,
) -> Result<()> {
    recovery.validate()?;
    completed(original)?;
    let task = original.task()?;
    let report = original.report.as_ref().unwrap();
    ensure!(
        recovery.command.id == plan.plan_id
            && recovery.command.grant_id == original.id()
            && recovery.binding_sha256 == original.binding_sha256
            && recovery.report_sha256 == eefn::grants::definition_digest(&json!(report))?
            && recovery.proposal_sha256
                == eefn::grants::definition_digest(
                    report
                        .data
                        .pointer("/candidate/candidate/proposal")
                        .context("Missing semantic result")?
                )?
            && recovery.source_revision == task.snapshot.revision
            && recovery.decided_at_ms >= original.binding.reserved_at_ms
            && recovery.stage.as_str() == original.stage()?
            && plan.goal.request_context.as_ref() == Some(&task.snapshot.request_context)
            && was_applied(plan, original)?,
        "Recovery receipt differs from retained source or result"
    );
    Ok(())
}

/// Read only past source evidence. This cannot construct a live Completion or
/// apply a retained result, including one that was successful before a restart.
pub(crate) fn was_applied(plan: &Plan, record: &AllowanceRecord) -> Result<bool> {
    let Some(report) = record.report.as_ref().filter(|r| r.success) else {
        return Ok(false);
    };
    let task = record.task()?;
    let grant = &record.binding.grant;
    let backend = grant
        .backend
        .as_deref()
        .context("Missing interpreter backend")?;
    let model = grant
        .model_id
        .as_deref()
        .context("Missing interpreter model")?;
    let candidate = task.candidate(
        &serde_json::to_vec(
            report
                .data
                .pointer("/candidate/candidate/proposal")
                .context("Missing interpreter semantic proposal")?,
        )?,
        backend,
        model,
    )?;
    let Some(draft) = &plan.request_review else {
        return Ok(false);
    };
    let s = &task.snapshot;
    Ok(match candidate {
        Candidate::Initial(candidate) => plan.input_interpretation.as_ref().is_some_and(|r| {
            r.state == eefn::request_interpretation::State::Promoted
                && r.claim.operation_id == s.operation_id
                && r.claim.expected_revision == s.revision
                && r.claim.origin_runtime_id == record.binding.origin_runtime_id
                && r.claim.interpreter.as_ref().is_some_and(|i| {
                    i.node_id == grant.executor_node
                        && i.runtime_id == record.binding.provider_runtime_id
                        && i.backend == backend
                        && i.model_id == model
                })
                && draft
                    .proposals
                    .first()
                    .is_some_and(|p| p.proposal == candidate.proposal)
        }),
        Candidate::Clarification(candidate) => {
            let Source::Clarification { draft: source } = &s.source else {
                return Ok(false);
            };
            draft.proposals.iter().any(|p| {
                p.operation_id == s.operation_id
                    && p.proposal == candidate.proposal
                    && p.preparation.is_none()
                    && p.resolution.as_ref().is_some_and(|e| {
                        e.source_revision == s.revision
                            && e.backend == backend
                            && e.model_id == model
                            && e.confirms_source(source)
                    })
            })
        }
        Candidate::Method(candidate) => {
            let source = PreparationSource {
                evidence: candidate.evidence,
                request_context: s.request_context.clone(),
            };
            if let Some(input) = candidate.proposal.method {
                plan.planning.as_ref().is_some_and(|p| {
                    p.operation_id == s.operation_id
                        && p.source_revision == s.revision
                        && p.input == input
                        && p.preparation.as_ref() == Some(&source)
                })
            } else {
                draft.proposals.iter().any(|p| {
                    p.operation_id == s.operation_id
                        && p.preparation.as_ref() == Some(&source)
                        && p.proposal.clarification == candidate.proposal.clarification
                })
            }
        }
    })
}
