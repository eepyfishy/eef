//! Native issuance saves metadata only. The original caller alone owns its
//! monotonic window; an unclaimed committed checkpoint retires to uncertainty.
#[cfg(test)]
use super::Pending;
use super::{JobStore, Slot};
use crate::{
    jobs::Assignment,
    request_execution::{AttemptState, ExecutionRecord},
    task::{PlanStatus, TaskStatus},
    workload_jobs::{Execution, mark_unknown, validate_approval},
};
use anyhow::{Context, Result, ensure};
use eefn::{request_execution::ExecuteCommand, workload_leases::definition::Definition};
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};

/// Historical ownership, never execution authority. No window, source, runtime,
/// lease or reconstructible issuer is sent through the native reply channel.
pub(crate) struct CommittedIssue {
    store: Arc<JobStore>,
    definition: Definition,
    slot: Option<Slot>,
}
impl CommittedIssue {
    pub(crate) fn take_slot(&mut self) -> Slot {
        self.slot.take().expect("Committed issuance claimed once")
    }
}
impl Drop for CommittedIssue {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            let _ = self
                .store
                .queue_workload_unknown(self.definition.clone(), slot);
        }
    }
}

impl JobStore {
    pub(crate) async fn checkpoint_workload_issue_before(
        self: &Arc<Self>,
        command: ExecuteCommand,
        definition: Definition,
        issued_at_ms: u64,
        final_slot: Slot,
        deadline: Instant,
    ) -> Result<CommittedIssue> {
        ensure!(
            Arc::ptr_eq(&final_slot.owner, &self.finalizations.slots),
            "Workload issuance slot belongs to another journal"
        );
        command.validate()?;
        definition.validate()?;
        ensure!(
            command.id == definition.job_id,
            "Workload issuance job changed"
        );
        ensure!(
            Instant::now() < deadline,
            "Workload issuance deadline elapsed before admission"
        );
        self.require_workload_reporting()?;
        // Share the existing ACK write budget; retirement keeps its separately
        // reserved capacity even while this native writer is blocked in SQLite.
        let slot = self
            .workload_writes
            .slots
            .clone()
            .try_acquire_owned()
            .context("Workload write capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        #[cfg(test)]
        let pending = Pending::new(self.workload_writes.pending.clone());
        let store = self.clone();
        let end = deadline.into_std();
        self.workload_writes.submit(Box::new(move || {
            let mut attempted = false;
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                    ensure!(
                        !reply.is_closed() && std::time::Instant::now() < end,
                        "Workload issuance abandoned before checkpoint"
                    );
                    store.require_workload_reporting()?;
                    store.change(
                        &definition.job_id,
                        "Exact workload definition issued durably before any network offer",
                        |plan| {
                            // Recheck consent and exact source AFTER journal acquisition.
                            ensure!(
                                !reply.is_closed() && std::time::Instant::now() < end,
                                "Workload issuance abandoned while waiting for journal"
                            );
                            store.require_workload_reporting()?;
                            validate_approval(plan)?;
                            let approval = plan
                                .approval
                                .as_ref()
                                .context("Workload approval missing")?;
                            ensure!(
                                plan.status == PlanStatus::Approved
                                    && plan.execution.is_none()
                                    && plan.revision == command.expected_revision
                                    && approval.approval_sha256 == command.approval_sha256
                                    && approval.issuer_id == definition.issuer_id
                                    && approval.plan_sha256 == definition.plan_sha256
                                    && plan.goal.request_context.as_ref()
                                        == Some(&definition.request_context)
                                    && plan.tasks[0].task_id == definition.task_id
                                    && approval.workload.as_ref() == Some(&definition.template())
                                    && approval.active_at(plan, eefn::protocol::now_ms())
                                    && issued_at_ms
                                        .checked_add(definition.limits.max_session_ms)
                                        .is_some_and(|end| end <= approval.expires_at_ms),
                                "Concurrent workload issuance/source/approval change"
                            );
                            attempted = true;
                            #[cfg(test)]
                            store
                                .workload_writes
                                .issuance_started
                                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            plan.execution = Some(ExecutionRecord {
                                schema_version: 2,
                                command: command.clone(),
                                started_at_ms: issued_at_ms,
                                attempts: vec![],
                                abandonment: None,
                                peer: None,
                                stop_requested_at_ms: None,
                                workload: Some(Execution {
                                    definition: definition.clone(),
                                    issued_at_ms,
                                    state: AttemptState::Issued,
                                    acknowledgement: None,
                                    completion: None,
                                    stop: None,
                                    observations: vec![],
                                }),
                            });
                            plan.status = PlanStatus::Running;
                            let task = &mut plan.tasks[0];
                            task.status = TaskStatus::Running;
                            task.attempts = 1;
                            task.assignments.push(Assignment {
                                attempt: 1,
                                node_id: definition.executor_node.clone(),
                                resource_id: Some(definition.template().resource_id().into()),
                                model_id: None,
                                backend: None,
                                assigned_at_ms: issued_at_ms,
                            });
                            Ok(())
                        },
                    )?;
                    Ok(())
                }));
            let result = match outcome {
                Ok(Ok(())) => Ok(CommittedIssue {
                    store: store.clone(),
                    definition: definition.clone(),
                    slot: Some(final_slot),
                }),
                failed => {
                    // A refused edit has no issued row. A possibly committed
                    // exact issuance needs one uncertainty attempt on this worker.
                    // Never overwrite a different assignment or terminal history.
                    if attempted {
                        let recovered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || -> Result<()> {
                                let saved = store.get(&definition.job_id)?;
                                if let Some(w) =
                                    saved.execution.as_ref().and_then(|e| e.workload.as_ref())
                                {
                                    if w.definition == definition
                                        && matches!(
                                            w.state,
                                            AttemptState::Issued | AttemptState::Accepted
                                        )
                                    {
                                        mark_unknown(&store, &definition)?;
                                    }
                                }
                                Ok(())
                            },
                        ));
                        if !matches!(recovered, Ok(Ok(()))) {
                            store.deny_workload_reporting();
                        }
                    }
                    let error = match failed {
                        Ok(Err(error)) => error,
                        Err(_) => {
                            store.deny_workload_reporting();
                            anyhow::anyhow!(
                                "Workload issuance checkpoint panicked; authority denied"
                            )
                        }
                        Ok(Ok(())) => unreachable!(),
                    };
                    drop(final_slot);
                    Err(error)
                }
            };
            // Actual work owns capacity/store after waiter cancellation. A lost
            // successful reply drops its packet and queues historical Unknown.
            drop(slot);
            drop(store);
            let _ = reply.send(result);
            #[cfg(test)]
            drop(pending);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Workload issuance deadline elapsed; no execution authority returned")?
            .context("Workload issuance worker unavailable")?;
        ensure!(
            Instant::now() < deadline,
            "Workload issuance completed after its original deadline"
        );
        result
    }
    #[cfg(test)]
    pub(crate) fn workload_issuance_started(&self) -> usize {
        self.workload_writes
            .issuance_started
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn wait_workload_issuance(&self) -> Result<()> {
        let state = &self.workload_writes.pending;
        let count = state.count.lock().unwrap();
        let (count, _) = state
            .changed
            .wait_timeout_while(count, std::time::Duration::from_secs(3), |n| *n != 0)
            .unwrap();
        ensure!(*count == 0, "Actual workload issuance has not finished");
        Ok(())
    }
}
