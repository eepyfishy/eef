//! Read-only access to the existing coordinator journal. A bounded native worker
//! keeps SQLite waits off async runtime threads, without owning an idle store.
use super::{JobStore, Plan};
use anyhow::{Context, Result, bail};
use std::sync::{Arc, Mutex, mpsc};
use tokio::sync::{Semaphore, oneshot};
use tokio::time::Instant;

const CAPACITY: usize = 4;
type Job = Box<dyn FnOnce() + Send + 'static>;

pub(super) struct Queue {
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    slots: Arc<Semaphore>,
    #[cfg(test)]
    started: std::sync::atomic::AtomicUsize,
}
impl Default for Queue {
    fn default() -> Self {
        Self {
            sender: Mutex::new(None),
            slots: Arc::new(Semaphore::new(CAPACITY)),
            #[cfg(test)]
            started: Default::default(),
        }
    }
}
impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JournalReadQueue").finish_non_exhaustive()
    }
}
impl Queue {
    fn submit(&self, job: Job) -> Result<()> {
        let sender = {
            let mut saved = self
                .sender
                .lock()
                .map_err(|_| anyhow::anyhow!("Journal read worker unavailable"))?;
            if saved.is_none() {
                let (sender, receiver) = mpsc::sync_channel::<Job>(CAPACITY);
                // The idle receiver owns no JobStore, database, runtime or file
                // lock. Only bounded admitted reads retain the exact journal.
                std::thread::Builder::new()
                    .name("eef-journal-read".into())
                    .spawn(move || {
                        while let Ok(job) = receiver.recv() {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .context("Journal read worker could not start")?;
                *saved = Some(sender);
            }
            saved.as_ref().unwrap().clone()
        };
        sender
            .try_send(job)
            .map_err(|_| anyhow::anyhow!("Journal read queue unavailable; no retry requested"))
    }
}

impl JobStore {
    /// Historical read only. The actual reader retains its slot and journal
    /// even if its async waiter times out or disconnects. No source or lease is
    /// passed to the worker; cancellation never creates replacement work.
    pub(crate) async fn get_before(self: &Arc<Self>, id: &str, deadline: Instant) -> Result<Plan> {
        if Instant::now() >= deadline {
            bail!("Journal read deadline elapsed before admission")
        }
        let slot = self
            .reads
            .slots
            .clone()
            .try_acquire_owned()
            .context("Journal read capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let store = self.clone();
        let id = id.to_owned();
        let original_end = deadline.into_std();
        self.reads.submit(Box::new(move || {
            if reply.is_closed() || std::time::Instant::now() >= original_end {
                // A queued abandoned/expired read cannot start a later query.
                return;
            }
            #[cfg(test)]
            store
                .reads
                .started
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.get(&id)))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Journal read failed; evidence retained")));
            // Do not free capacity when only the network waiter goes away.
            drop(slot);
            let _ = reply.send(result);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Journal read deadline elapsed; evidence unavailable")?
            .context("Journal read worker unavailable; no fallback requested")?;
        if Instant::now() >= deadline {
            bail!("Journal read completed after its original deadline")
        }
        result
    }
}

#[cfg(test)]
#[path = "job_reads_tests.rs"]
mod tests;
