//! Admission-reserved historical checkpoints in the existing JobStore. Native
//! work holds metadata and journal ownership, never an issuer, lease or source.
use super::JobStore;
use anyhow::{Context, Result, ensure};
use eefn::workload_leases::definition::Definition;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[path = "job_workload_acks.rs"]
mod workload_acks;
#[path = "job_workload_completion.rs"]
mod workload_completion;
#[path = "job_workload_events.rs"]
mod workload_events;
#[path = "job_workload_stop.rs"]
mod workload_stop;
pub(crate) use workload_stop::StopCheckpoint;
#[path = "job_workload_issuance.rs"]
mod workload_issuance;
pub(crate) use workload_issuance::CommittedIssue;
#[path = "job_interpreter_retirement.rs"]
mod interpreter_retirement;

const MAX_FINALIZATIONS: usize = 64;
type Job = Box<dyn FnOnce() + Send + 'static>;

pub(crate) struct Slot {
    _permit: OwnedSemaphorePermit,
    owner: Arc<Semaphore>,
}
pub(super) struct Queue {
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    slots: Arc<Semaphore>,
    capacity: usize,
    failed: AtomicBool,
    #[cfg(test)]
    pending: Arc<PendingState>,
    #[cfg(test)]
    started: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    issuance_started: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    event_entered: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    stop_entered: std::sync::atomic::AtomicUsize,
}
impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkloadCheckpointQueue")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}
impl Default for Queue {
    fn default() -> Self {
        Self::new(
            MAX_FINALIZATIONS,
            MAX_FINALIZATIONS * crate::workload_jobs::COMPLETION_RESERVE,
        )
    }
}
impl Queue {
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }
    pub(super) fn for_workload_writes(max_count: usize, max_bytes: usize) -> Self {
        Self::new(max_count.min(4), max_bytes)
    }
    pub(super) fn new(max_count: usize, max_bytes: usize) -> Self {
        // Each issued job already reserves completion headroom in the same
        // owner's quota. Never allocate a channel from an unbounded config value.
        let capacity = max_count
            .min(max_bytes / crate::workload_jobs::COMPLETION_RESERVE)
            .min(MAX_FINALIZATIONS);
        Self {
            sender: Mutex::new(None),
            slots: Arc::new(Semaphore::new(capacity)),
            capacity,
            failed: AtomicBool::new(false),
            #[cfg(test)]
            pending: Default::default(),
            #[cfg(test)]
            started: Default::default(),
            #[cfg(test)]
            issuance_started: Default::default(),
            #[cfg(test)]
            event_entered: Default::default(),
            #[cfg(test)]
            stop_entered: Default::default(),
        }
    }
    fn submit(&self, job: Job) -> Result<()> {
        let sender = {
            let mut saved = self
                .sender
                .lock()
                .map_err(|_| anyhow::anyhow!("Workload checkpoint worker unavailable"))?;
            if saved.is_none() {
                let (sender, receiver) = mpsc::sync_channel::<Job>(self.capacity);
                // The idle receiver owns no store, runtime, lease or file lock.
                std::thread::Builder::new()
                    .name("eef-workload-journal".into())
                    .spawn(move || {
                        while let Ok(job) = receiver.recv() {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .context("Workload checkpoint worker could not start")?;
                *saved = Some(sender);
            }
            saved.as_ref().unwrap().clone()
        };
        sender
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("Reserved workload checkpoint queue unavailable"))
    }
}
impl JobStore {
    pub(crate) fn require_workload_reporting(&self) -> Result<()> {
        ensure!(
            !self.finalizations.failed.load(Ordering::Acquire),
            "Workload checkpoint failed; new authority denied until owner recovery"
        );
        Ok(())
    }
    pub(crate) fn reserve_workload_finalization(&self) -> Result<Slot> {
        self.require_workload_reporting()?;
        let owner = self.finalizations.slots.clone();
        let permit = owner
            .clone()
            .try_acquire_owned()
            .context("Workload reporting capacity unavailable before issuance")?;
        Ok(Slot {
            _permit: permit,
            owner,
        })
    }
    pub(crate) fn deny_workload_reporting(&self) {
        self.finalizations.failed.store(true, Ordering::Release);
    }
    pub(crate) fn queue_workload_unknown(
        self: &Arc<Self>,
        definition: Definition,
        slot: Slot,
    ) -> Result<()> {
        if !Arc::ptr_eq(&slot.owner, &self.finalizations.slots) {
            self.deny_workload_reporting();
            anyhow::bail!("Workload checkpoint slot belongs to another journal")
        }
        #[cfg(test)]
        let pending = Pending::new(self.finalizations.pending.clone());
        let store = self.clone();
        let submitted = self.finalizations.submit(Box::new(move || {
            // Retiring the original issuer happens BEFORE this queue. These
            // bytes describe historical uncertainty and cannot renew or run it.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::workload_jobs::mark_unknown(&store, &definition)
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Workload checkpoint panicked")));
            if result.is_err() {
                store.deny_workload_reporting();
            }
            // A cancelled caller or runtime never frees actual queued capacity.
            drop(slot);
            drop(store);
            #[cfg(test)]
            drop(pending);
        }));
        if submitted.is_err() {
            self.deny_workload_reporting();
        }
        submitted
    }
    #[cfg(test)]
    pub(crate) fn wait_workload_finalizations(&self) -> Result<()> {
        let state = &self.finalizations.pending;
        let count = state.count.lock().unwrap();
        let (count, _) = state
            .changed
            .wait_timeout_while(count, std::time::Duration::from_secs(3), |n| *n != 0)
            .unwrap();
        ensure!(*count == 0, "Actual workload checkpoint has not finished");
        Ok(())
    }
    #[cfg(test)]
    pub(crate) async fn wait_workload_finalizations_async(&self) -> Result<()> {
        self.finalizations.pending.wait_async().await
    }
    #[cfg(test)]
    pub(crate) fn test_workload_finalization_limit(&mut self, limit: usize) {
        assert!(self.finalizations.sender.lock().unwrap().is_none());
        assert_eq!(
            self.finalizations.slots.available_permits(),
            self.finalizations.capacity
        );
        self.finalizations = Queue::new(limit, self.max_bytes);
    }
    #[cfg(test)]
    pub(crate) fn workload_finalization_available(&self) -> usize {
        self.finalizations.slots.available_permits()
    }
}

#[cfg(test)]
#[derive(Default)]
struct PendingState {
    count: Mutex<usize>,
    changed: std::sync::Condvar,
    async_changed: tokio::sync::Notify,
}
#[cfg(test)]
impl PendingState {
    async fn wait_async(&self) -> Result<()> {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let changed = self.async_changed.notified();
                tokio::pin!(changed);
                // Register before inspecting the count: the native writer may
                // finish between that inspection and the first await.
                changed.as_mut().enable();
                if *self.count.lock().unwrap() == 0 {
                    return;
                }
                changed.await;
            }
        })
        .await
        .context("Actual workload checkpoint has not finished")
    }
}
#[cfg(test)]
struct Pending(Arc<PendingState>);
#[cfg(test)]
impl Pending {
    fn new(state: Arc<PendingState>) -> Self {
        *state.count.lock().unwrap() += 1;
        Self(state)
    }
}
#[cfg(test)]
impl Drop for Pending {
    fn drop(&mut self) {
        *self.0.count.lock().unwrap() -= 1;
        self.0.changed.notify_all();
        self.0.async_changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn async_checkpoint_wait_tracks_all_writes_and_wakes_all_waiters() {
        let state = Arc::new(PendingState::default());
        let first = Pending::new(state.clone());
        let second = Pending::new(state.clone());
        let a = tokio::spawn({
            let state = state.clone();
            async move { state.wait_async().await }
        });
        let b = tokio::spawn({
            let state = state.clone();
            async move { state.wait_async().await }
        });
        tokio::task::yield_now().await;
        drop(first);
        tokio::task::yield_now().await;
        assert!(!a.is_finished());
        assert!(!b.is_finished());
        drop(second);
        a.await.unwrap().unwrap();
        b.await.unwrap().unwrap();
        // Completion before registration must also be observable immediately.
        state.wait_async().await.unwrap();
    }
}
