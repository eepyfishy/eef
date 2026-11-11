//! Apply a live service completion through ordinary review/compiler transitions.
//! No public DTO, historical receipt or model output can create this continuation.
use super::*;
#[path = "interpreter_recovery_runtime.rs"]
mod recovery;
use crate::{
    interpreter_allowance::Permit,
    task::{Plan, PlanStatus},
};
use eefn::{
    interpreter_task::{Candidate, Source},
    request_preparation::{PreparationCheckpoint, PreparationSource},
    request_resolution::ResolutionEvidence,
};

enum Mutation {
    Initial(eefn::interpretation::Proposal),
    Clarification(eefn::interpretation::Proposal, ResolutionEvidence),
    Question(PreparationCheckpoint),
    Method(Plan),
}
impl Mutation {
    fn apply(self, plan: &mut Plan, reserved: &Reserved, task: &Task) -> Result<bool> {
        let snapshot = &task.snapshot;
        match self {
            Self::Initial(proposal) => {
                let claim = reserved
                    .initial_claim()?
                    .context("Missing interpreter source claim")?;
                let expected_revision = plan.revision;
                crate::request_interpretation::apply(
                    plan,
                    crate::request_interpretation::Transition::Promote {
                        claim,
                        expected_revision,
                        proposal,
                    },
                    Some(snapshot.request_context.origin_node()),
                    &snapshot.allowed_capabilities,
                )
            }
            other => other.apply_review(plan, task),
        }
    }
    fn apply_review(self, plan: &mut Plan, task: &Task) -> Result<bool> {
        let snapshot = &task.snapshot;
        match self {
            Self::Initial(_) => {
                anyhow::bail!("Initial input needs its explicit ownership transition")
            }
            Self::Clarification(proposal, evidence) => {
                let Source::Clarification { draft: source } = &snapshot.source else {
                    anyhow::bail!("Interpreter source stage differs")
                };
                let draft = plan
                    .request_review
                    .as_mut()
                    .context("Missing reviewed source")?;
                crate::request_drafts::append_revision(
                    draft,
                    plan.status,
                    plan.revision,
                    snapshot.revision,
                    &snapshot.operation_id,
                    &source.original_text,
                    &proposal,
                    Some(evidence),
                    &snapshot.allowed_capabilities,
                )
            }
            Self::Question(checkpoint) => crate::request_preparation::apply_question(
                plan,
                &checkpoint,
                snapshot.request_context.origin_node(),
                &snapshot.allowed_capabilities,
            ),
            Self::Method(compiled) => {
                ensure!(
                    compiled.status == PlanStatus::AwaitingApproval
                        && compiled.approval.is_none()
                        && compiled.execution.is_none(),
                    "Interpreter cannot approve or execute a method"
                );
                crate::request_planning::validate_retained(&compiled)?;
                crate::request_planning::apply_compilation(plan, compiled);
                Ok(true)
            }
        }
    }
}

impl Runtime {
    async fn prepare_interpreter_mutation(&self, record: &Record) -> Result<(Task, Mutation)> {
        record.validate()?;
        let task = record.task()?;
        let grant = &record.binding.grant;
        let report = record
            .report
            .as_ref()
            .context("Missing interpreter result")?;
        ensure!(
            report.success && report.confirms(grant),
            "Unconfirmed interpreter result"
        );
        // Rebuild the typed candidate, never deserialize peer authority markers.
        let proposal = report
            .data
            .pointer("/candidate/candidate/proposal")
            .context("Missing interpreter semantic proposal")?;
        let backend = grant
            .backend
            .as_deref()
            .context("Missing interpreter backend")?;
        let model = grant
            .model_id
            .as_deref()
            .context("Missing interpreter model")?;
        let candidate = task.candidate(&serde_json::to_vec(proposal)?, backend, model)?;
        let snapshot = &task.snapshot;
        let mutation = match candidate {
            Candidate::Initial(candidate) => Mutation::Initial(candidate.proposal),
            Candidate::Clarification(candidate) => {
                let Source::Clarification { draft } = &snapshot.source else {
                    anyhow::bail!("Interpreter source stage differs")
                };
                Mutation::Clarification(
                    candidate.proposal,
                    ResolutionEvidence {
                        schema_version: 1,
                        source_revision: snapshot.revision,
                        source_sha256: ResolutionEvidence::source_digest(draft)?,
                        source_proposal_count: draft.proposals.len(),
                        source_answer_count: draft.answers.len(),
                        backend: backend.into(),
                        model_id: model.into(),
                    },
                )
            }
            Candidate::Method(candidate) => {
                let checkpoint = PreparationCheckpoint {
                    schema_version: 1,
                    operation_id: snapshot.operation_id.clone(),
                    source: PreparationSource {
                        evidence: candidate.evidence,
                        request_context: snapshot.request_context.clone(),
                    },
                    proposal: candidate.proposal,
                };
                checkpoint.validate()?;
                if let Some(input) = &checkpoint.proposal.method {
                    let saved = self.engine.store.get(&snapshot.job_id)?;
                    Mutation::Method(
                        self.prepare_compilation(
                            &saved,
                            input,
                            &snapshot.operation_id,
                            Some(snapshot.request_context.origin_node()),
                            Some(&checkpoint.source),
                        )
                        .await?,
                    )
                } else {
                    Mutation::Question(checkpoint)
                }
            }
        };
        Ok((task, mutation))
    }
    /// Consume once, before its original deadline. The actual source, latest
    /// policy, applied/saved authentication and both live registrations remain
    /// fenced through the same journal write. Failure retires source ownership
    /// through Completion's existing drop path; it never retries inference.
    pub(crate) async fn apply_interpreter_completion(&self, completed: Completion) -> Result<Plan> {
        completed.continuation.check()?;
        let record = completed.record();
        let (task, mutation) = self.prepare_interpreter_mutation(record).await?;
        let snapshot = &task.snapshot;
        let grant = &record.binding.grant;
        let backend = grant
            .backend
            .as_deref()
            .context("Missing interpreter backend")?;
        let model = grant
            .model_id
            .as_deref()
            .context("Missing interpreter model")?;
        let origin = snapshot.request_context.origin_node();
        let provider = grant.executor_node.as_str();
        let (capabilities, freshness) = self
            .interpreter_participant_context(origin, provider)
            .await?;
        let pin = PolicyPin {
            id: record.binding.policy_id.clone(),
            generation: record.binding.policy_generation,
            sha256: record.binding.policy_sha256.clone(),
        };
        self.node_server
            .with_interpreter_sessions(origin, provider, freshness, |origin, provider| {
                let permit = Permit::from_participants(
                    &Participants {
                        origin,
                        provider,
                        backend: backend.into(),
                        model_id: model.into(),
                    },
                    capabilities,
                )?;
                permit.validate(&task)?;
                self.config.edit_for_restart(|saved| {
                    let settings = eefn::authentication::AuthenticationPolicy::parse(
                        saved.pointer("/node/authentication"),
                    )?;
                    let issuer = crate::interpreter_policy::current_issuer(
                        self.node_server.authentication_policy(),
                        &settings,
                    )?;
                    ensure!(
                        issuer.as_deref() == Some(permit.issuer.as_str()),
                        "Interpreter authentication changed before source checkpoint"
                    );
                    let plan = self.engine.commit_interpreter_result(
                        completed.continuation.registration()?,
                        &completed.continuation.reserved,
                        &permit,
                        self.runtime_id(),
                        &pin,
                        record,
                        |plan| mutation.apply(plan, &completed.continuation.reserved, &task),
                    )?;
                    Ok((plan, false))
                })
            })
            .await
    }
}
