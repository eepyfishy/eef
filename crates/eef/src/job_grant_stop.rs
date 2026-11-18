//! Historical one-shot stop metadata on the existing bounded native writer.
//! No network client, execution future, lease or Tokio runtime enters this worker.
use super::JobStore;
use crate::execution_stop::Delivery;
use anyhow::{Context, Result, ensure};
use eefn::grants::{ExecutionGrant, stop::Report};
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};

pub(crate) enum GrantStopCheckpoint {
    Begin,
    Receipt(Report),
}
impl JobStore {
    /// A successful new Begin is the only permission to attempt this stop send.
    /// Cancellation can leave a committed reservation, never replay authority.
    pub(crate) async fn checkpoint_grant_stop_before(
        self: &Arc<Self>,
        id: &str,
        grant: ExecutionGrant,
        checkpoint: GrantStopCheckpoint,
        deadline: Instant,
    ) -> Result<bool> {
        grant.validate()?;
        ensure!(id == grant.job_id, "Stop checkpoint targets another job");
        if let GrantStopCheckpoint::Receipt(report) = &checkpoint {
            ensure!(report.confirms(&grant), "Stop receipt scope changed");
        }
        ensure!(
            Instant::now() < deadline,
            "Stop checkpoint deadline elapsed before admission"
        );
        // Share existing bounded metadata-write capacity, not an execution slot.
        // A workload-reporting failure must not prohibit stopping one-shot work.
        let slot = self
            .workload_writes
            .slots
            .clone()
            .try_acquire_owned()
            .context("Stop checkpoint capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let store = self.clone();
        let end = deadline.into_std();
        self.workload_writes.submit(Box::new(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<bool> {
                    ensure!(
                        !reply.is_closed() && std::time::Instant::now() < end,
                        "Stop checkpoint abandoned before storage"
                    );
                    #[cfg(test)]
                    store
                        .workload_writes
                        .grant_stop_entered
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let mut changed = false;
                    store.edit(
                        &grant.job_id,
                        "Exact grant stop metadata; no execution or retry authority",
                        |plan| {
                            // Check again after acquiring the actual journal, not its waiter.
                            ensure!(
                                !reply.is_closed() && std::time::Instant::now() < end,
                                "Stop checkpoint abandoned while waiting for storage"
                            );
                            let execution = plan
                                .execution
                                .as_mut()
                                .context("Missing execution issuance")?;
                            let Some(requested) = execution.stop_requested_at_ms else {
                                ensure!(
                                    matches!(checkpoint, GrantStopCheckpoint::Begin),
                                    "Stop intent missing"
                                );
                                return Ok(false);
                            };
                            let attempt = execution
                                .attempts
                                .iter_mut()
                                .find(|a| a.grant.grant_id == grant.grant_id)
                                .context("Stop grant is not issued")?;
                            ensure!(
                                attempt.grant == grant,
                                "Stop cannot retarget changed issuance"
                            );
                            match checkpoint {
                                GrantStopCheckpoint::Begin => {
                                    if attempt.stop.is_some() {
                                        return Ok(false);
                                    }
                                    attempt.stop = Some(Delivery {
                                        send_attempted_at_ms: eefn::protocol::now_ms()
                                            .max(requested),
                                        report: None,
                                        outcome_unknown: true,
                                    });
                                }
                                GrantStopCheckpoint::Receipt(report) => {
                                    let saved = attempt
                                        .stop
                                        .as_mut()
                                        .context("Stop send checkpoint missing")?;
                                    if let Some(existing) = &saved.report {
                                        ensure!(
                                            *existing == report,
                                            "Stop receipt already differs"
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
                    Err(anyhow::anyhow!(
                        "Stop checkpoint failed; delivery remains uncertain"
                    ))
                });
            // Keep ownership until native storage actually finishes, even when
            // its observer timed out, disconnected or lost its Tokio runtime.
            drop(slot);
            drop(store);
            let _ = reply.send(result);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Stop checkpoint deadline elapsed; delivery remains uncertain")?
            .context("Stop checkpoint worker unavailable")?;
        ensure!(
            Instant::now() < deadline,
            "Stop checkpoint observed after its original deadline"
        );
        result
    }
    #[cfg(test)]
    pub(crate) fn grant_stop_entered(&self) -> usize {
        self.workload_writes
            .grant_stop_entered
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
