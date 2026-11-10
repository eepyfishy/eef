//! Authenticated reference-only commands over the existing node submission path.
use crate::{
    Runtime,
    interpreter_allowance::{Participants, PolicyPin, Record},
    interpreter_exchange::Exchange,
    task::{Plan, PlanStatus},
};
use anyhow::{Context, Result, ensure};
use eefn::{
    context::RequestContext,
    interpreter_command::{Attempt, Command, Report, Stage, WireRequest},
    interpreter_task::{Candidate, Snapshot, Source, Task},
    request_preparation::PreparationSource,
};
use serde_json::{Value, json};

fn owns(plan: &Plan, origin: &str) -> bool {
    plan.goal
        .request_context
        .as_ref()
        .is_some_and(|c| c.origin_node() == origin)
}
fn preferences(command: &Command, record: &Record) -> Result<()> {
    let grant = &record.binding.grant;
    ensure!(
        command.preferences.matches(
            record.task()?.snapshot.request_context.origin_node(),
            &grant.executor_node,
            grant
                .backend
                .as_deref()
                .context("Missing interpreter backend")?,
            grant
                .model_id
                .as_deref()
                .context("Missing interpreter model")?
        ),
        "Interpreter preferences conflict with the retained attempt; no replacement"
    );
    Ok(())
}

/// Read only past source evidence. This cannot construct a live Completion or
/// apply a retained result, including one that was successful before a restart.
fn was_applied(plan: &Plan, record: &Record) -> Result<bool> {
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

impl Runtime {
    fn interpreter_command_report(
        &self,
        command: &Command,
        context: &RequestContext,
        record: &Record,
        plan: &Plan,
        historical: bool,
        requested: bool,
    ) -> Result<Value> {
        ensure!(
            owns(plan, context.origin_node()),
            "Interpreter source unavailable for this origin"
        );
        preferences(command, record)?;
        let applied = was_applied(plan, record)?;
        let grant = &record.binding.grant;
        let report = Report {
            schema_version: 1,
            report_type: "coordinator_interpreter_command".into(),
            runtime_id: self.runtime_id().into(),
            origin_node: context.origin_node().into(),
            command: command.clone(),
            attempt: Attempt {
                id: record.id().into(),
                operation_id: record.task()?.snapshot.operation_id,
                source_revision: record.task()?.snapshot.revision,
                phase: json!(record.phase)
                    .as_str()
                    .context("Invalid interpreter phase")?
                    .into(),
                provider: grant.executor_node.clone(),
                backend: grant
                    .backend
                    .clone()
                    .context("Missing interpreter backend")?,
                model_id: grant
                    .model_id
                    .clone()
                    .context("Missing interpreter model")?,
            },
            success: applied,
            source_applied: applied,
            historical_snapshot: historical,
            model_inference_requested: requested,
            outcome_unknown: matches!(
                record.phase,
                crate::interpreter_allowance::Phase::DeliveryIntent
                    | crate::interpreter_allowance::Phase::Accepted
                    | crate::interpreter_allowance::Phase::Interrupted
            ) || record
                .report
                .as_ref()
                .is_some_and(|r| r.state == eefn::grants::ReceiptState::Unknown),
            view: if plan.request_review.is_some() {
                // This is a fresh inspection, not a reused planning write.
                // Attempt history is recorded by historical_snapshot above.
                Some(self.node_planning_view(plan, context, false)?)
            } else {
                None
            },
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
        };
        let value = json!(report);
        ensure!(
            eefn::interpreter_command::confirmed(&value, context.origin_node(), command),
            "Interpreter command reply could not be confirmed"
        );
        Ok(value)
    }

    pub(crate) async fn origin_interpreter_command(
        &self,
        request: WireRequest,
        context: &RequestContext,
        gateway_session_id: &str,
    ) -> Result<Value> {
        request.validate(context.origin_node())?;
        ensure!(
            request.command.operation_id == context.request_id(),
            "Interpreter operation differs from authenticated origin"
        );
        // A queued command from a retired connection cannot initiate work on its
        // replacement, even when the stable node ID is unchanged.
        let origin = self
            .node_server
            .interpreter_session(context.origin_node(), self.discovery.freshness_seconds)
            .await?;
        ensure!(
            origin.session_id() == gateway_session_id
                && origin.runtime_id() == request.origin_runtime_id,
            "Interpreter origin session/runtime changed before admission"
        );
        let command = &request.command;
        let saved = self.engine.store.get(&command.id)?;
        ensure!(
            owns(&saved, context.origin_node()),
            "Interpreter source unavailable for this origin"
        );
        for record in self.engine.store.interpreter_allowance_records()?.0 {
            let task = record.task()?;
            if task.snapshot.request_context.origin_node() != context.origin_node()
                || record.stage()? != command.stage.as_str()
            {
                continue;
            }
            let same_operation = task.snapshot.operation_id == command.operation_id;
            let same_source = task.snapshot.job_id == command.id
                && (command.expected_revision == Some(task.snapshot.revision)
                    || command.stage == Stage::Initial && command.expected_revision.is_none());
            if same_operation || same_source {
                ensure!(
                    task.snapshot.job_id == command.id
                        && command
                            .expected_revision
                            .is_none_or(|r| r == task.snapshot.revision),
                    "Interpreter receipt conflicts with retained source"
                );
                return self
                    .interpreter_command_report(command, context, &record, &saved, true, false);
            }
        }
        ensure!(
            saved.status == PlanStatus::Review
                && saved.planning.is_none()
                && saved.approval.is_none()
                && saved.execution.is_none()
                && command
                    .expected_revision
                    .is_none_or(|r| r == saved.revision),
            "Interpreter source is stale, stopped or already executable"
        );
        let source = match command.stage {
            Stage::Initial => {
                ensure!(
                    saved.input_interpretation.is_none(),
                    "Input already has an interpretation attempt; inspect it before continuing"
                );
                Source::Initial {
                    input: crate::request_intent::validate(&saved)?,
                }
            }
            Stage::Clarification => Source::Clarification {
                draft: Box::new(
                    saved
                        .request_review
                        .clone()
                        .context("Missing reviewed source")?,
                ),
            },
            Stage::Method => Source::Method {
                draft: Box::new(
                    saved
                        .request_review
                        .clone()
                        .context("Missing reviewed source")?,
                ),
            },
        };
        let options = crate::interpreter_routing::Options {
            origin_node: context.origin_node().into(),
            local_only: command.preferences.local_only,
            node_id: command.preferences.provider.clone(),
            backend: command.preferences.backend.clone(),
            model_id: command.preferences.model_id.clone(),
        };
        let preview = crate::interpreter_routing::resolve(
            self,
            &self.discovery,
            crate::interpreter_routing::Request {
                schema_version: 1,
                expected_runtime_id: self.runtime_id().into(),
                command: options.clone(),
            },
        )
        .await?;
        ensure!(
            crate::interpreter_routing::confirmed(&preview, &options, self.runtime_id()),
            "Interpreter route observation differs"
        );
        let route: crate::interpreter_routing::Report = serde_json::from_value(preview)?;
        ensure!(
            route.origin.session_id == gateway_session_id
                && route.origin.runtime_id == request.origin_runtime_id,
            "Interpreter origin changed during selection"
        );
        let selected = route
            .selected
            .context("No authorized interpreter is available; input remains saved")?;
        let policy = self
            .engine
            .store
            .interpreter_policy_receipt(&route.policy_id)?
            .context("Interpreter policy unavailable")?;
        let pin = PolicyPin::from(&policy);
        ensure!(
            pin.generation == route.policy_generation && pin.sha256 == route.policy_sha256,
            "Interpreter policy changed during selection"
        );
        let eefn::interpreter_policy::Command::Approve { limits, .. } = &policy.creation else {
            unreachable!()
        };
        let capabilities = self
            .disclosed_interpretation_capabilities(context.origin_node())
            .await?;
        let max_output_tokens = source.max_output_tokens().min(limits.max_output_tokens);
        let task = Task::new(
            Snapshot {
                schema_version: 1,
                job_id: saved.plan_id,
                revision: saved.revision,
                operation_id: command.operation_id.clone(),
                request_context: saved
                    .goal
                    .request_context
                    .context("Missing immutable origin")?,
                allowed_capabilities: capabilities,
                source,
            },
            max_output_tokens,
        )?;
        let provider = self
            .node_server
            .interpreter_session(
                &selected.connection.node_id,
                self.discovery.freshness_seconds,
            )
            .await?;
        ensure!(
            provider.session_id() == selected.connection.session_id
                && provider.runtime_id() == selected.connection.runtime_id
                && provider.permission_revision() == Some(selected.permission_revision)
                && json!(provider.limits()) == json!(selected.grant_limits),
            "Selected interpreter changed; no substitution"
        );
        let participants = Participants {
            origin,
            provider,
            backend: selected.backend,
            model_id: selected.model_id,
        };
        let outcome = self.exchange_interpreter(task, pin, &participants).await?;
        let (record, plan, historical, requested) = match outcome {
            Exchange::Historical(record) => {
                (*record, self.engine.store.get(&command.id)?, true, false)
            }
            Exchange::Failed(record) => (*record, self.engine.store.get(&command.id)?, false, true),
            Exchange::Completed(completed) => {
                let record = completed.record().clone();
                let plan = self.apply_interpreter_completion(completed).await?;
                (record, plan, false, true)
            }
        };
        self.interpreter_command_report(command, context, &record, &plan, historical, requested)
    }
}
