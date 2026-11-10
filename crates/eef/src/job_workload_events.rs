//! Bounded authenticated event metadata in the existing job journal. The
//! original caller keeps control authority; this worker only saves history.
use super::JobStore;
use crate::{request_execution::AttemptState, task::PlanStatus};
use anyhow::{Context, Result, ensure};
use eefn::{workload_leases::definition::Definition, workload_network::Event};
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};

impl JobStore {
    pub(crate) fn queue_workload_event(
        self: &Arc<Self>,
        definition: Definition,
        event: Event,
        deadline: Instant,
    ) -> Result<oneshot::Receiver<Result<()>>> {
        definition.validate()?;
        event.validate(&definition)?;
        ensure!(
            Instant::now() < deadline
                && serde_json::to_vec(&event)?.len() <= definition.events.max_event_bytes,
            "Workload event deadline or byte bound exceeded"
        );
        self.require_workload_reporting()?;
        let slot = self
            .workload_writes
            .slots
            .clone()
            .try_acquire_owned()
            .context("Workload event checkpoint capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let store = self.clone();
        let end = deadline.into_std();
        self.workload_writes.submit(Box::new(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                    ensure!(
                        !reply.is_closed() && std::time::Instant::now() < end,
                        "Workload event observer retired"
                    );
                    #[cfg(test)]
                    store
                        .workload_writes
                        .event_entered
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    store.edit(
                        &definition.job_id,
                        "Authenticated workload metadata saved; no lease or execution authority",
                        |plan| {
                            ensure!(
                                !reply.is_closed() && std::time::Instant::now() < end,
                                "Workload event abandoned during storage wait"
                            );
                            store.require_workload_reporting()?;
                            let execution = plan
                                .execution
                                .as_mut()
                                .context("Missing workload execution")?;
                            let w = execution
                                .workload
                                .as_mut()
                                .context("Missing workload issuance")?;
                            ensure!(
                                plan.status == PlanStatus::Running
                                    && w.state == AttemptState::Accepted
                                    && w.definition == definition
                                    && execution.stop_requested_at_ms.is_none(),
                                "Workload event belongs to retired or different issuance"
                            );
                            event.follows(&definition, w.observations.last())?;
                            w.observations.push(event);
                            execution.schema_version = 5;
                            Ok(true)
                        },
                    )?;
                    Ok(())
                }))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "Workload event checkpoint failed; no retry"
                    ))
                });
            drop(slot);
            drop(store);
            let _ = reply.send(result);
        }))?;
        Ok(receiver)
    }
    #[cfg(test)]
    pub(crate) fn workload_event_entered(&self) -> usize {
        self.workload_writes
            .event_entered
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn queue_workload_event_test_barrier(
        &self,
        ready: oneshot::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> Result<()> {
        self.workload_writes.submit(Box::new(move || {
            let _ = ready.send(());
            // Sender drop also releases this synthetic worker on test failure.
            let _ = release.recv_timeout(std::time::Duration::from_secs(10));
        }))
    }
}
