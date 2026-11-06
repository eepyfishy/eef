//! Engine-owned workload lifetimes, including pending asynchronous issuance.
//! A registration is not approval, a lease, a source factory or durable history.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct State {
    active: AtomicBool,
    live: Mutex<bool>,
    cancelled: tokio::sync::watch::Sender<bool>,
}
impl State {
    pub(super) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub(super) fn cancel(&self) {
        if let Ok(mut live) = self.live.lock() {
            *live = false;
        }
        self.cancelled.send_replace(true);
    }
}
pub(crate) struct WorkloadJob {
    id: String,
    state: Arc<State>,
}
impl WorkloadJob {
    pub(crate) fn stop_receiver(&self) -> tokio::sync::watch::Receiver<bool> {
        self.state.cancelled.subscribe()
    }
    pub(crate) fn check(&self, id: &str) -> Result<()> {
        if self.id != id
            || !self.state.active()
            || !*self
                .state
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Workload control unavailable"))?
        {
            bail!("Workload engine ownership retired, stopped or changed")
        }
        Ok(())
    }
    /// One non-awaiting control delivery shares the exact stop linearization
    /// gate. No database wait or source operation belongs inside this closure.
    pub(crate) fn deliver(&self, id: &str, send: impl FnOnce() -> Result<()>) -> Result<()> {
        let live = self
            .state
            .live
            .lock()
            .map_err(|_| anyhow::anyhow!("Workload control unavailable"))?;
        if self.id != id || !self.state.active() || !*live {
            bail!("Workload delivery denied after stop or shutdown")
        }
        send()
    }
}

#[cfg(test)]
#[path = "task_workload_tests.rs"]
mod tests;
impl Drop for WorkloadJob {
    fn drop(&mut self) {
        self.state.cancel();
        // Original Issued/CommittedIssue owns the separately reserved journal
        // finalization. Dropping registration must never wait on that storage.
        self.state.active.store(false, Ordering::Release);
    }
}
impl ActiveJobs {
    pub(super) fn contains(&self, id: &str) -> bool {
        self.ids.contains(id) || self.workloads.get(id).is_some_and(|state| state.active())
    }
    pub(super) fn empty(&self) -> bool {
        self.ids.is_empty() && self.workloads.values().all(|state| !state.active())
    }
}
impl TaskEngine {
    pub(crate) fn claim_workload(&self, id: &str) -> Result<WorkloadJob> {
        eefn::request_drafts::validate_id(id)?;
        let mut active = self.active.lock().expect("active jobs");
        if active.closing || active.contains(id) {
            bail!("Coordinator is stopping or this job already owns a workload lifetime")
        }
        active.workloads.retain(|_, state| state.active());
        if active.workloads.len() >= self.store.workload_lifetime_capacity() {
            bail!("Workload engine lifetime capacity unavailable; no queued retry")
        }
        let state = Arc::new(State {
            active: AtomicBool::new(true),
            live: Mutex::new(true),
            cancelled: tokio::sync::watch::channel(false).0,
        });
        active.workloads.insert(id.into(), state.clone());
        Ok(WorkloadJob {
            id: id.into(),
            state,
        })
    }
}
