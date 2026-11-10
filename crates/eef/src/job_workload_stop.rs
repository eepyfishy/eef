//! Exact stop metadata on the existing bounded writer. No issuer, source,
//! runtime or network operation enters this worker.
use super::JobStore;
use crate::{execution_stop::Delivery, request_execution::AttemptState, task::PlanStatus};
use anyhow::{Context, Result, ensure};
use eefn::{grants::stop::Report, workload_leases::definition::Definition};
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};

pub(crate) enum StopCheckpoint {
    Begin,
    Receipt(Report),
}
impl StopCheckpoint {
    fn validate(&self, definition: &Definition) -> Result<()> {
        let scope = definition.scope()?;
        if let Self::Receipt(report) = self {
            ensure!(
                report.confirms_scope(&scope),
                "Workload stop receipt scope differs"
            );
        }
        Ok(())
    }
}
impl JobStore {
    pub(crate) async fn checkpoint_workload_stop_before(
        self: &Arc<Self>,
        definition: Definition,
        checkpoint: StopCheckpoint,
        deadline: Instant,
    ) -> Result<bool> {
        checkpoint.validate(&definition)?;
        ensure!(
            Instant::now() < deadline,
            "Workload stop deadline elapsed before admission"
        );
        self.require_workload_reporting()?;
        let slot = self
            .workload_writes
            .slots
            .clone()
            .try_acquire_owned()
            .context("Workload stop checkpoint capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let store = self.clone();
        let end = deadline.into_std();
        self.workload_writes.submit(Box::new(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<bool> {
                    ensure!(
                        !reply.is_closed() && std::time::Instant::now() < end,
                        "Workload stop checkpoint abandoned before storage"
                    );
                    store.require_workload_reporting()?;
                    #[cfg(test)]
                    store
                        .workload_writes
                        .stop_entered
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut changed = false;
                    store.edit(
                        &definition.job_id,
                        "Exact workload stop metadata; no execution or retry authority",
                        |plan| {
                            ensure!(
                                !reply.is_closed() && std::time::Instant::now() < end,
                                "Workload stop checkpoint abandoned while waiting for storage"
                            );
                            store.require_workload_reporting()?;
                            let execution = plan
                                .execution
                                .as_mut()
                                .context("Missing workload execution")?;
                            let requested = execution
                                .stop_requested_at_ms
                                .context("No explicit workload stop intent")?;
                            let w = execution
                                .workload
                                .as_mut()
                                .context("Missing workload issuance")?;
                            ensure!(
                                w.definition == definition
                                    && matches!(
                                        plan.status,
                                        PlanStatus::Stopping | PlanStatus::Interrupted
                                    )
                                    && matches!(
                                        w.state,
                                        AttemptState::Issued
                                            | AttemptState::Accepted
                                            | AttemptState::Unknown
                                    ),
                                "Workload stop cannot retarget or replace terminal history"
                            );
                            match checkpoint {
                                StopCheckpoint::Begin => {
                                    if w.stop.is_some() {
                                        return Ok(false);
                                    }
                                    w.stop = Some(Delivery {
                                        send_attempted_at_ms: eefn::protocol::now_ms()
                                            .max(requested),
                                        report: None,
                                        outcome_unknown: true,
                                    });
                                    execution.schema_version =
                                        if w.observations.is_empty() { 4 } else { 5 };
                                }
                                StopCheckpoint::Receipt(report) => {
                                    let saved = w
                                        .stop
                                        .as_mut()
                                        .context("Workload stop send checkpoint missing")?;
                                    if let Some(existing) = &saved.report {
                                        ensure!(
                                            *existing == report,
                                            "Workload stop receipt already differs"
                                        );
                                        return Ok(false);
                                    }
                                    saved.report = Some(report);
                                    saved.outcome_unknown = false;
                                }
                            }
                            changed = true;
                            Ok(true)
                        },
                    )?;
                    Ok(changed)
                }))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!("Workload stop checkpoint failed; no retry"))
                });
            // Actual storage retains both slot and journal until it has exited,
            // even if the observing future was cancelled or timed out.
            drop(slot);
            drop(store);
            let _ = reply.send(result);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Workload stop checkpoint deadline elapsed; delivery remains uncertain")?
            .context("Workload stop checkpoint worker unavailable")?;
        ensure!(
            Instant::now() < deadline,
            "Workload stop checkpoint observed after deadline"
        );
        result
    }
    #[cfg(test)]
    pub(crate) fn workload_stop_entered(&self) -> usize {
        self.workload_writes
            .stop_entered
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
