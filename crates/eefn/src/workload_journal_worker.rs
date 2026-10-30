//! One lazy native journal worker. Its bounded queue owns metadata only, never
//! a source or lease. Runtime shutdown need not join a stalled SQLite operation.
use super::GrantLedger;
use anyhow::{Context, Result, bail};
use std::sync::mpsc::{self, SyncSender};

pub(super) type Job = Box<dyn FnOnce() + Send + 'static>;
pub(super) type Sender = SyncSender<Job>;

impl GrantLedger {
    pub(crate) fn submit_workload_job(&self, job: Job) -> Result<()> {
        let sender = {
            let mut saved = self
                .workload_worker
                .lock()
                .map_err(|_| anyhow::anyhow!("Workload journal worker unavailable"))?;
            if saved.is_none() {
                // At most one event save and one admission-reserved final save
                // exist per handle, including the currently executing job.
                let (sender, receiver) = mpsc::sync_channel::<Job>(2);
                std::thread::Builder::new()
                    .name("eefn-journal".into())
                    .spawn(move || {
                        while let Ok(job) = receiver.recv() {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .context("Workload journal worker could not start")?;
                *saved = Some(sender);
            }
            saved.as_ref().unwrap().clone()
        };
        match sender.try_send(job) {
            Ok(()) => Ok(()),
            Err(_) => bail!("Workload journal worker queue unavailable; no automatic retry"),
        }
    }
}
