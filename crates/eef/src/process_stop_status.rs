//! One bounded read-only lookup of original stop targets. Never retransmit stop.
use crate::Runtime;
use anyhow::{Context, Result, bail};
use eefn::process_stop_status::{Availability, Command, GrantStatus, Report, Request};
use serde_json::Value;

impl Runtime {
    pub async fn process_stop_status_command(&self, request: Request) -> Result<Value> {
        request.validate()?;
        if request.expected_runtime_id != self.runtime_id() {
            bail!("Stop inspection runtime changed")
        }
        self.inspect_process_stop_status(request.command, None)
            .await
    }
    pub(crate) async fn inspect_process_stop_status(
        &self,
        command: Command,
        origin: Option<&str>,
    ) -> Result<Value> {
        command.validate()?;
        let plan = self.engine.store.get(&command.id)?;
        let context = plan
            .goal
            .request_context
            .as_ref()
            .context("Job lacks original node context")?;
        if origin.is_some_and(|node| node != context.origin_node()) {
            bail!("Job not found for this node")
        }
        let issued = plan
            .execution
            .as_ref()
            .map(|e| e.attempts.len())
            .unwrap_or(0);
        let requested = plan
            .execution
            .as_ref()
            .is_some_and(|e| e.stop_requested_at_ms.is_some());
        let targets = plan
            .execution
            .as_ref()
            .map(|e| {
                e.attempts
                    .iter()
                    .filter(|a| a.stop.is_some())
                    .map(|a| a.grant.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if targets.len() > 2 || (!requested && !targets.is_empty()) {
            bail!("Unconfirmed stop target history")
        }
        let inspect = |grant: eefn::grants::ExecutionGrant| async move {
            let scope = eefn::grants::stop::Scope::from_grant(&grant)?;
            if grant.capability != "process.exec" || grant.action != "run" {
                return Ok::<_, anyhow::Error>(GrantStatus {
                    scope,
                    availability: Availability::Unsupported,
                    intent_recorded: false,
                    observation: None,
                });
            }
            let read = async {
                let intent = self.node_server.inspect_grant_stop(&grant).await?;
                let recorded = intent.evidence.is_some();
                let observation = if recorded {
                    let result = self.node_server.inspect_grant_result(&grant).await?;
                    result.report.as_ref().and_then(|result| {
                        eefn::process_stop::confirmed_inspection(&grant, &intent, result)
                    })
                } else {
                    None
                };
                Ok::<_, anyhow::Error>(GrantStatus {
                    scope: scope.clone(),
                    availability: if observation.is_some() {
                        Availability::OwnedProcessTreeEmpty
                    } else {
                        Availability::Unconfirmed
                    },
                    intent_recorded: recorded,
                    observation,
                })
            };
            // A new read-only wait budget never extends execution authority.
            Ok(
                match tokio::time::timeout(std::time::Duration::from_secs(5), read).await {
                    Ok(Ok(status)) => status,
                    _ => GrantStatus {
                        scope,
                        availability: Availability::Unavailable,
                        intent_recorded: false,
                        observation: None,
                    },
                },
            )
        };
        let grants = match targets.as_slice() {
            [] => vec![],
            [one] => vec![inspect(one.clone()).await?],
            [one, two] => {
                let (a, b) = tokio::join!(inspect(one.clone()), inspect(two.clone()));
                vec![a?, b?]
            }
            _ => unreachable!(),
        };
        let report = Report {
            schema_version: 1,
            report_type: "job_process_stop_status".into(),
            success: true,
            runtime_id: self.runtime_id().into(),
            id: plan.plan_id,
            revision: plan.revision,
            origin_node: context.origin_node().into(),
            stop_requested: requested,
            issued_attempts: issued,
            not_inspected_issued_attempts: issued - grants.len(),
            grants,
            read_only: true,
            execution_requested: false,
            stop_signal_sent: false,
            automatically_retried: false,
            reconciled: false,
            external_effects_stopped: false,
            finished_actions_undone: false,
            note: eefn::process_stop_status::NOTE.into(),
        };
        report.validate()?;
        Ok(serde_json::to_value(report)?)
    }
}
