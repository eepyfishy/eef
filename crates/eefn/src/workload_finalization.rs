//! Bounded final uncertainty persistence after source/reporting retirement.
//! The worker owns only a consumed scope and journal, never a source or lease.
use super::*;
use std::sync::atomic::Ordering;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

const WAIT: Duration = Duration::from_secs(3);

pub(super) struct Finalization<T>(oneshot::Receiver<Result<T>>);
impl<T> Finalization<T> {
    pub(super) async fn wait(self) -> Result<T> {
        tokio::time::timeout(WAIT, self.0)
            .await
            .context("Final workload checkpoint still pending; final outcome is unconfirmed")?
            .context("Final workload checkpoint worker unavailable")?
    }
}
pub(super) fn start(
    ledger: &Arc<GrantLedger>,
    scope: stop::Scope,
    slot: OwnedSemaphorePermit,
) -> Result<Finalization<()>> {
    scope.validate()?;
    start_with(ledger, slot, move |journal| {
        journal.mark_workload_unknown(&scope)
    })
}
pub(super) fn start_with<T, F>(
    ledger: &Arc<GrantLedger>,
    slot: OwnedSemaphorePermit,
    save: F,
) -> Result<Finalization<T>>
where
    T: Send + 'static,
    F: FnOnce(&GrantLedger) -> Result<T> + Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let journal = ledger.clone();
    let spawned = ledger.submit_workload_job(Box::new(move || {
        // No Tokio runtime dependency: cancellation and runtime shutdown do not
        // make the caller wait for blocked SQLite. Capacity stays charged until
        // the actual save finishes, even when its async waiter has gone away.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| save(&journal)))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Final workload checkpoint worker panicked")));
        if result.is_err() {
            journal
                .final_reporting_failed
                .store(true, Ordering::Release);
        }
        drop(slot);
        let _ = sender.send(result);
    }));
    if let Err(error) = spawned {
        ledger.final_reporting_failed.store(true, Ordering::Release);
        return Err(error.context("Final workload checkpoint worker could not start"));
    }
    // No source/renewal handle enters the queue. One admission-reserved slot
    // bounds these saves even if their async waiter is cancelled.
    Ok(Finalization(receiver))
}
