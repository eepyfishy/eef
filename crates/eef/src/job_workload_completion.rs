//! Historical completion uses the slot reserved before issuance. It owns no
//! live issuer, window or source and does not depend on a caller staying alive.
#[cfg(test)]
use super::Pending;
use super::{JobStore, Slot};
use crate::{
    request_execution::AttemptState,
    task::{Plan, PlanStatus, TaskStatus},
    workload_jobs::{Completion, mark_unknown},
};
use anyhow::{Context, Result, ensure};
use eefn::workload_leases::definition::Definition;
use std::sync::Arc;
use tokio::sync::oneshot;

impl JobStore {
    pub(crate) fn queue_workload_completion(
        self: &Arc<Self>,
        definition: Definition,
        completed: Completion,
        slot: Slot,
    ) -> Result<oneshot::Receiver<Result<Plan>>> {
        if !Arc::ptr_eq(&slot.owner, &self.finalizations.slots) {
            self.deny_workload_reporting();
            anyhow::bail!("Workload completion slot belongs to another journal")
        }
        let (reply, receiver) = oneshot::channel();
        #[cfg(test)]
        let pending = Pending::new(self.finalizations.pending.clone());
        let store = self.clone();
        let submitted = self.finalizations.submit(Box::new(move || {
            // The original window was closed before admission. Cancellation
            // cannot erase this historical report or release actual ownership.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                store.change(
                    &definition.job_id,
                    "Authenticated node source completion saved; no execution or replay authority",
                    |plan| {
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
                                && w.completion.is_none(),
                            "Workload changed before completion checkpoint"
                        );
                        completed.validate(
                            &w.definition,
                            w.acknowledgement
                                .as_ref()
                                .context("Completion lacks consumed ACK")?,
                            w.issued_at_ms,
                        )?;
                        #[cfg(test)]
                        store
                            .finalizations
                            .started
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        plan.updated_at_ms = plan.updated_at_ms.max(completed.recorded_at_ms);
                        w.completion = Some(completed);
                        w.state = AttemptState::Completed;
                        execution.schema_version = if w.observations.is_empty() { 3 } else { 5 };
                        plan.status = PlanStatus::Completed;
                        plan.tasks[0].status = TaskStatus::Completed;
                        Ok(())
                    },
                )
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Workload completion checkpoint failed")));
            if result.is_err() {
                // Exactly one uncertainty attempt on the same actual worker.
                // No caller, Drop or replacement token retries completion.
                let fallback = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mark_unknown(&store, &definition)
                }));
                if !matches!(fallback, Ok(Ok(()))) {
                    store.deny_workload_reporting();
                }
            }
            drop(slot);
            drop(store);
            #[cfg(test)]
            drop(pending);
            let _ = reply.send(result);
        }));
        if let Err(error) = submitted {
            self.deny_workload_reporting();
            return Err(error);
        }
        Ok(receiver)
    }
    #[cfg(test)]
    pub(crate) fn workload_completion_started(&self) -> usize {
        self.finalizations
            .started
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}
