//! Bounded native journal callbacks with no async-runtime shutdown dependency.
use super::GrantLedger;
use anyhow::{Context, Result, bail};
use std::sync::{
    Mutex,
    mpsc::{self, SyncSender},
};

pub(super) type Job = Box<dyn FnOnce() + Send + 'static>;

pub(super) struct JournalWorker {
    name: &'static str,
    capacity: usize,
    sender: Mutex<Option<SyncSender<Job>>>,
}

impl JournalWorker {
    pub(super) fn new(name: &'static str, capacity: usize) -> Self {
        Self {
            name,
            capacity,
            sender: Mutex::new(None),
        }
    }

    pub(super) fn submit(&self, job: Job) -> Result<()> {
        let sender = {
            let mut saved = self
                .sender
                .lock()
                .map_err(|_| anyhow::anyhow!("Native journal worker unavailable"))?;
            if let Some(sender) = saved.as_ref() {
                sender.clone()
            } else {
                let (sender, receiver) = mpsc::sync_channel::<Job>(self.capacity);
                std::thread::Builder::new()
                    .name(self.name.into())
                    .spawn(move || {
                        while let Ok(job) = receiver.recv() {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .context("Native journal worker could not start")?;
                *saved = Some(sender.clone());
                sender
            }
        };
        if sender.try_send(job).is_err() {
            bail!("Native journal worker queue unavailable; no automatic retry")
        }
        Ok(())
    }
}

impl GrantLedger {
    pub(crate) fn submit_workload_job(&self, job: Job) -> Result<()> {
        // The event and admission-reserved final slots bound actual callbacks.
        self.workload_worker.submit(job)
    }
}
