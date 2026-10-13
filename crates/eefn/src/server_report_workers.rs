//! Bounded durable report callbacks owned beyond cancellation of their waiters.
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use tokio::sync::watch;

use crate::grants::{GrantObserver, GrantReport};

#[derive(Default)]
struct State {
    closing: bool,
    active: usize,
}
pub(super) struct ReportWorkers {
    state: Mutex<State>,
    active: watch::Sender<usize>,
    limit: usize,
}
// Returned to the async consumer after a successful save. Ownership also covers
// pending result delivery, so stop/restart cannot overtake a late confirmation.
pub(super) struct Work(Arc<ReportWorkers>);
impl Drop for Work {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().expect("grant report workers");
        state.active -= 1;
        self.0.active.send_replace(state.active);
    }
}
impl ReportWorkers {
    pub(super) fn new(limit: usize) -> Arc<Self> {
        assert!(limit > 0);
        Arc::new(Self {
            state: Mutex::new(State::default()),
            active: watch::channel(0).0,
            limit,
        })
    }
    fn begin(self: &Arc<Self>) -> Result<Work> {
        let mut state = self.state.lock().expect("grant report workers");
        if state.closing {
            bail!("Gateway stopping; report checkpoint not admitted");
        }
        if state.active >= self.limit {
            bail!("Grant report checkpoints busy; no extra callback or retry");
        }
        state.active += 1;
        self.active.send_replace(state.active);
        Ok(Work(self.clone()))
    }
    pub(super) fn is_closing(&self) -> bool {
        self.state.lock().expect("grant report workers").closing
    }
    #[cfg(test)]
    pub(super) fn active_count(&self) -> usize {
        self.state.lock().expect("grant report workers").active
    }
    pub(super) fn close(&self) {
        self.state.lock().expect("grant report workers").closing = true;
    }
    pub(super) fn open(&self) -> Result<()> {
        let mut state = self.state.lock().expect("grant report workers");
        if state.active != 0 {
            bail!("Report callbacks still owned; cannot restart gateway");
        }
        state.closing = false;
        Ok(())
    }
    pub(super) async fn drain(&self) {
        let mut active = self.active.subscribe();
        loop {
            if *active.borrow_and_update() == 0 {
                return;
            }
            active.changed().await.expect("owned worker counter");
        }
    }
    pub(super) async fn record(
        self: &Arc<Self>,
        observer: Arc<dyn GrantObserver>,
        report: GrantReport,
    ) -> Result<Work> {
        let work = self.begin()?;
        let manager = self.clone();
        tokio::task::spawn_blocking(move || {
            if manager.is_closing() {
                bail!("Gateway stopping; queued report checkpoint skipped");
            }
            observer.record(&report)?;
            if manager.is_closing() {
                bail!("Gateway stopped during checkpoint; inspect retained evidence");
            }
            Ok(work)
        })
        .await
        .context("Grant report callback lost; no retry or completion proof")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::ReceiptState;
    use std::time::Duration;

    struct Observer(std::sync::atomic::AtomicUsize);
    impl GrantObserver for Observer {
        fn record(&self, _: &GrantReport) -> Result<()> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }
    fn report() -> GrantReport {
        GrantReport::new(
            &crate::grants::tests::grant(),
            ReceiptState::Accepted,
            serde_json::Value::Null,
            None,
        )
    }
    #[tokio::test]
    async fn bounds_and_close_refuse_extra_work_and_restart_requires_drain() {
        let workers = ReportWorkers::new(2);
        let first = workers.begin().unwrap();
        let second = workers.begin().unwrap();
        assert!(workers.begin().is_err());
        assert_eq!(workers.active_count(), 2);
        workers.close();
        assert!(workers.begin().is_err());
        assert!(workers.open().is_err());
        drop(first);
        assert_eq!(workers.active_count(), 1);
        drop(second);
        workers.drain().await;
        workers.open().unwrap();
        let observer = Arc::new(Observer(0.into()));
        workers.record(observer.clone(), report()).await.unwrap();
        assert_eq!(observer.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(workers.active_count(), 0);
    }
    #[test]
    fn cancelled_queued_waiter_remains_owned_and_skips_save_after_close() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let workers = ReportWorkers::new(1);
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, held) = std::sync::mpsc::channel();
            let occupied = tokio::task::spawn_blocking(move || {
                started.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            ready.await.unwrap();
            let observer = Arc::new(Observer(0.into()));
            let manager = workers.clone();
            let saving = observer.clone();
            let waiter = tokio::spawn(async move { manager.record(saving, report()).await });
            tokio::time::timeout(Duration::from_secs(2), async {
                while workers.active_count() != 1 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            waiter.abort();
            match waiter.await {
                Err(error) => assert!(error.is_cancelled()),
                Ok(_) => panic!("queued waiter should be cancelled"),
            }
            assert_eq!(workers.active_count(), 1);
            assert!(workers.record(observer.clone(), report()).await.is_err());
            workers.close();
            assert!(workers.open().is_err());
            let draining = workers.clone();
            let drain = tokio::spawn(async move { draining.drain().await });
            tokio::task::yield_now().await;
            assert!(!drain.is_finished());
            release.send(()).unwrap();
            occupied.await.unwrap();
            tokio::time::timeout(Duration::from_secs(2), drain)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(workers.active_count(), 0);
            assert_eq!(observer.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        });
    }
    #[tokio::test]
    async fn successful_save_remains_owned_through_async_result_delivery() {
        let workers = ReportWorkers::new(1);
        let observer = Arc::new(Observer(0.into()));
        let receipt = workers.record(observer.clone(), report()).await.unwrap();
        assert_eq!(workers.active_count(), 1);
        workers.close();
        assert!(workers.open().is_err());
        let draining = workers.clone();
        let drain = tokio::spawn(async move { draining.drain().await });
        tokio::task::yield_now().await;
        assert!(!drain.is_finished());
        drop(receipt);
        drain.await.unwrap();
        assert_eq!(workers.active_count(), 0);
        assert_eq!(observer.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        workers.open().unwrap();
    }
    #[tokio::test]
    async fn panicked_observer_releases_ownership_without_confirmation_or_retry() {
        struct Panics;
        impl GrantObserver for Panics {
            fn record(&self, _: &GrantReport) -> Result<()> {
                panic!("isolated report callback fault");
            }
        }
        let workers = ReportWorkers::new(1);
        assert!(workers.record(Arc::new(Panics), report()).await.is_err());
        workers.close();
        workers.drain().await;
        assert_eq!(workers.active_count(), 0);
    }
}
