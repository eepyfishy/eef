//! Bounded native ACK persistence, separate from admission-reserved retirement.
use super::JobStore;
#[cfg(test)]
use super::Queue;
use crate::{
    request_execution::AttemptState,
    task::{Plan, PlanStatus},
};
use anyhow::{Context, Result, ensure};
use eefn::{grants::workload::Receipt, workload_leases::definition::Definition};
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};

impl JobStore {
    pub(crate) async fn checkpoint_workload_ack_before(
        self: &Arc<Self>,
        definition: Definition,
        ack: Receipt,
        deadline: Instant,
    ) -> Result<Plan> {
        definition.validate()?;
        ack.validate()?;
        ensure!(
            ack.definition == definition && ack.receipt.state == "accepted",
            "Wrong workload ACK checkpoint scope/state"
        );
        ensure!(
            Instant::now() < deadline,
            "Workload ACK deadline elapsed before admission"
        );
        self.require_workload_reporting()?;
        let slot = self
            .workload_writes
            .slots
            .clone()
            .try_acquire_owned()
            .context("Workload ACK checkpoint capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let store = self.clone();
        let end = deadline.into_std();
        self.workload_writes.submit(Box::new(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Plan> {
                    ensure!(
                        !reply.is_closed() && std::time::Instant::now() < end,
                        "Workload ACK abandoned before checkpoint"
                    );
                    store.require_workload_reporting()?;
                    store.change(
                        &definition.job_id,
                        "Exact workload consumption ACK saved before renewals",
                        |plan| {
                            // Recheck AFTER acquiring the actual journal, before editing.
                            // A commit already in progress can finish after observation;
                            // that historical fact never restores a cancelled issuer.
                            ensure!(
                                !reply.is_closed() && std::time::Instant::now() < end,
                                "Workload ACK abandoned while waiting for journal"
                            );
                            store.require_workload_reporting()?;
                            let w = plan
                                .execution
                                .as_mut()
                                .and_then(|e| e.workload.as_mut())
                                .context("Workload issuance missing")?;
                            ensure!(
                                plan.status == PlanStatus::Running
                                    && w.state == AttemptState::Issued
                                    && w.definition == definition
                                    && w.acknowledgement.is_none(),
                                "Workload state changed before ACK checkpoint"
                            );
                            #[cfg(test)]
                            store
                                .workload_writes
                                .started
                                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            w.acknowledgement = Some(ack);
                            w.state = AttemptState::Accepted;
                            Ok(())
                        },
                    )
                }))
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "Workload ACK checkpoint failed; no renewal authority"
                    ))
                });
            drop(slot);
            drop(store);
            let _ = reply.send(result);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Workload ACK checkpoint deadline elapsed; persistence unconfirmed")?
            .context("Workload ACK checkpoint worker unavailable")?;
        ensure!(
            Instant::now() < deadline,
            "Workload ACK checkpoint completed after its original deadline"
        );
        result
    }
    #[cfg(test)]
    pub(crate) fn test_workload_ack_limit(&mut self, limit: usize) {
        assert!(self.workload_writes.sender.lock().unwrap().is_none());
        assert_eq!(
            self.workload_writes.slots.available_permits(),
            self.workload_writes.capacity
        );
        self.workload_writes = Queue::for_workload_writes(limit, self.max_bytes);
    }
    #[cfg(test)]
    pub(crate) fn workload_ack_available(&self) -> usize {
        self.workload_writes.slots.available_permits()
    }
    #[cfg(test)]
    pub(crate) fn workload_ack_started(&self) -> usize {
        self.workload_writes
            .started
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
