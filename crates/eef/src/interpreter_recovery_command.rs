//! Explicit origin commands over the existing authenticated submission channel.
use super::*;
use anyhow::ensure;
use eefn::interpreter_recovery_command::{Attempt, Command, Report, WireRequest};

impl Runtime {
    pub(crate) async fn origin_interpreter_recovery_command(
        &self,
        request: WireRequest,
        context: &eefn::context::RequestContext,
        gateway_session_id: &str,
    ) -> Result<Value> {
        request.command.validate()?;
        ensure!(
            request.schema_version == 1 && request.command.operation_id() == context.request_id(),
            "Recovery command origin differs"
        );
        let origin = self
            .node_server
            .interpreter_session(context.origin_node(), self.node_freshness_seconds())
            .await?;
        ensure!(
            origin.session_id() == gateway_session_id
                && origin.runtime_id() == request.origin_runtime_id,
            "Recovery command session/runtime changed"
        );
        // Scope-check before any attempt lookup, even for an absent/foreign ID.
        let source = self.engine.store.get(request.command.id())?;
        ensure!(
            source
                .goal
                .request_context
                .as_ref()
                .is_some_and(|c| c.origin_node() == context.origin_node()),
            "Interpreter source unavailable for this origin"
        );
        let (plan, attempts, truncated, recovery, reused) = match &request.command {
            Command::Inspect { .. } => {
                let mut records = self
                    .engine
                    .store
                    .interpreter_allowance_records()?
                    .0
                    .into_iter()
                    .filter(|r| {
                        r.binding.grant.job_id == source.plan_id
                            && r.binding.grant.request_context.origin_node()
                                == context.origin_node()
                    })
                    .collect::<Vec<_>>();
                records.sort_by(|a, b| {
                    b.binding
                        .reserved_at_ms
                        .cmp(&a.binding.reserved_at_ms)
                        .then_with(|| b.id().cmp(a.id()))
                });
                let truncated = records.len() > 48;
                let attempts = records
                    .into_iter()
                    .take(48)
                    .map(|r| {
                        let grant = &r.binding.grant;
                        Ok(Attempt {
                            attempt: eefn::interpreter_command::Attempt {
                                id: r.id().into(),
                                operation_id: r.task()?.snapshot.operation_id,
                                source_revision: r.task()?.snapshot.revision,
                                phase: json!(r.phase)
                                    .as_str()
                                    .context("Invalid attempt phase")?
                                    .into(),
                                provider: grant.executor_node.clone(),
                                backend: grant.backend.clone().context("Missing backend")?,
                                model_id: grant.model_id.clone().context("Missing model")?,
                                execution_limit_ms: Some(grant.limits.max_execution_ms),
                            },
                            stage: match r.stage()? {
                                "initial" => eefn::interpreter_command::Stage::Initial,
                                "clarification" => eefn::interpreter_command::Stage::Clarification,
                                _ => eefn::interpreter_command::Stage::Method,
                            },
                            completed_result: crate::interpreter_recovery::completed(&r).is_ok(),
                            source_applied: crate::interpreter_recovery::was_applied(&source, &r)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                (source, attempts, truncated, None, false)
            }
            Command::Recover { command } => {
                let (plan, reused) = self.recover_interpreter_result(command, &origin).await?;
                let receipt = plan
                    .interpreter_recoveries
                    .iter()
                    .find(|r| r.command == *command)
                    .context("Missing applied recovery receipt")?
                    .clone();
                (plan, vec![], false, Some(receipt), reused)
            }
        };
        // Also fence read-only results; a retired gateway cannot report as its replacement.
        self.node_server
            .with_interpreter_sessions(
                origin.node_id(),
                origin.node_id(),
                self.node_freshness_seconds(),
                |current, _| {
                    ensure!(
                        current.session_id() == origin.session_id()
                            && current.runtime_id() == origin.runtime_id(),
                        "Recovery origin retired before reply"
                    );
                    let report = json!(Report {
                        schema_version: 1,
                        report_type: "coordinator_interpreter_recovery".into(),
                        runtime_id: self.runtime_id().into(),
                        origin_node: context.origin_node().into(),
                        command: request.command.clone(),
                        revision: plan.revision,
                        status: json!(plan.status)
                            .as_str()
                            .context("Invalid source status")?
                            .into(),
                        attempts,
                        truncated,
                        recovery,
                        reused,
                        model_inference_requested: false,
                        execution_authorized: false,
                        dispatched: false,
                        automatically_retried: false
                    });
                    ensure!(
                        eefn::interpreter_recovery_command::confirmed(
                            &report,
                            &request.command,
                            context.origin_node()
                        ),
                        "Recovery response could not be confirmed"
                    );
                    Ok(report)
                },
            )
            .await
    }
}
