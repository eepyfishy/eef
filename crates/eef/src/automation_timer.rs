//! Bounded trigger coordination; workload execution remains in the grant driver.
use crate::{
    Runtime,
    jobs::automation_schedules::{State, Trigger},
    task::{PlanStatus, TaskEngine},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) struct TimerManager {
    pub(crate) active: Mutex<BTreeSet<String>>,
    pub(crate) closing: AtomicBool,
    pub(crate) max_active: usize,
    max_launches: usize,
    poll: Duration,
    timeout: Duration,
    clock_epoch: AtomicU64,
    clock_samples: AtomicU64,
    clock_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    journal_work: Mutex<bool>,
    journal_idle: Condvar,
}

// Owns the single queued/running periodic checkpoint even if its async waiter
// is cancelled. Shutdown drains it before interrupting schedules or closing DB.
struct JournalWork(Arc<TimerManager>);
impl Drop for JournalWork {
    fn drop(&mut self) {
        *self.0.journal_work.lock().expect("timer journal worker") = false;
        self.0.journal_idle.notify_all();
    }
}
impl TimerManager {
    pub(crate) fn new(config: &crate::config::Config) -> Result<Arc<Self>> {
        let number = |key: &str, default: u64| -> Result<u64> {
            match config.get(key) {
                None => Ok(default),
                Some(value) => value.as_u64().ok_or_else(|| {
                    anyhow::anyhow!("Automation setting {key} must be an unsigned integer")
                }),
            }
        };
        let poll = number("automations.poll_ms", 200)?;
        let active = number("automations.max_active", 8)?;
        let launches = number("automations.max_launches", 2)?;
        let timeout = number("automations.launch_timeout_ms", 30000)?;
        if !(50..=1000).contains(&poll)
            || !(1..=32).contains(&active)
            || !(1..=8).contains(&launches)
            || launches > active
            || !(1000..=60000).contains(&timeout)
        {
            bail!("Invalid bounded automation timer settings");
        }
        Ok(Arc::new(Self {
            active: Mutex::new(BTreeSet::new()),
            closing: AtomicBool::new(false),
            max_active: active as usize,
            max_launches: launches as usize,
            poll: Duration::from_millis(poll),
            timeout: Duration::from_millis(timeout),
            clock_epoch: AtomicU64::new(0),
            clock_samples: AtomicU64::new(0),
            clock_thread: Mutex::new(None),
            journal_work: Mutex::new(false),
            journal_idle: Condvar::new(),
        }))
    }
    pub(crate) fn close(&self, store: &crate::jobs::JobStore) -> Result<()> {
        self.closing.store(true, Ordering::SeqCst);
        if let Some(thread) = self.clock_thread.lock().expect("timer clock").take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Timer clock monitor failed; no suspension proof"))?;
        }
        let mut working = self.journal_work.lock().expect("timer journal worker");
        while *working {
            working = self
                .journal_idle
                .wait(working)
                .expect("timer journal worker");
        }
        drop(working);
        let ids = self.active.lock().expect("timer active").clone();
        for id in ids {
            store.schedule_interrupt_now(&id, "shutdown")?;
        }
        self.active.lock().expect("timer active").clear();
        Ok(())
    }
    fn begin_journal_work(self: &Arc<Self>) -> Result<Option<JournalWork>> {
        let mut working = self.journal_work.lock().expect("timer journal worker");
        if self.closing.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if *working {
            bail!("Periodic timer checkpoint already queued/running; no extra worker");
        }
        *working = true;
        Ok(Some(JournalWork(self.clone())))
    }
    async fn poll_schedule(
        self: &Arc<Self>,
        engine: Arc<TaskEngine>,
        id: String,
        runtime: String,
        discontinuity: bool,
        may_launch: bool,
        launching: bool,
    ) -> Result<Option<Trigger>> {
        let Some(work) = self.begin_journal_work()? else {
            return Ok(None);
        };
        let manager = self.clone();
        tokio::task::spawn_blocking(move || {
            let _owned = work;
            // A queued callback cannot admit work after shutdown begins.
            if manager.closing.load(Ordering::SeqCst) {
                return Ok(None);
            }
            let started = Instant::now();
            let step = manager.checkpoint(&engine, &id, &runtime, discontinuity, may_launch, launching);
            if started.elapsed() > Duration::from_millis(500) {
                tracing::warn!(schedule_id=%id,elapsed_ms=started.elapsed().as_millis(),"Slow timer journal checkpoint");
            }
            if let Err(error) = &step {
                tracing::warn!(schedule_id=%id,%error,"Timer checkpoint failed; no catch-up or retry");
                let _ = engine.store.schedule_interrupt_now(&id, "checkpoint_failed");
                manager.active.lock().expect("timer active").remove(&id);
            }
            step
        })
        .await
        .context("Periodic timer worker lost; inspect retained IDs, no retry")?
    }
    fn checkpoint(
        &self,
        engine: &TaskEngine,
        id: &str,
        runtime: &str,
        discontinuity: bool,
        may_launch: bool,
        launching: bool,
    ) -> Result<Option<Trigger>> {
        if !self.active.lock().expect("timer active").contains(id) {
            return Ok(None);
        }
        if discontinuity {
            engine
                .store
                .schedule_interrupt_now(id, "clock_discontinuity")?;
        }
        let schedule = engine.store.schedule_get(id)?;
        match schedule.state {
            State::Waiting if may_launch && !self.closing.load(Ordering::SeqCst) => {
                return engine.claim_schedule_due(id, runtime, &schedule.issuer_id);
            }
            State::InFlight if !launching => {
                let trigger = schedule
                    .active_trigger
                    .as_deref()
                    .context("Missing active trigger")?;
                let plan = engine.store.trigger_job(trigger)?;
                match plan.as_ref().map(|p| p.status) {
                    Some(PlanStatus::Completed | PlanStatus::Cancelled) => {
                        engine
                            .store
                            .schedule_settle_now(id, runtime, &schedule.issuer_id)?;
                    }
                    Some(PlanStatus::Running | PlanStatus::Stopping) => {}
                    _ => {
                        engine
                            .store
                            .trigger_interrupt_now(trigger, "job_unresolved")?;
                    }
                }
            }
            State::Waiting | State::InFlight => {}
            _ => {
                self.active.lock().expect("timer active").remove(id);
            }
        }
        Ok(None)
    }
    fn start_clock(self: &Arc<Self>) -> Result<()> {
        let mut handle = self.clock_thread.lock().expect("timer clock");
        if handle.is_some() {
            return Ok(());
        }
        let manager = self.clone();
        *handle = Some(
            std::thread::Builder::new()
                .name("eef-timer-clock".into())
                .spawn(move || {
                    let mut previous = (Instant::now(), eefn::protocol::now_ms());
                    while !manager.closing.load(Ordering::SeqCst) {
                        std::thread::sleep(manager.poll);
                        let now = (Instant::now(), eefn::protocol::now_ms());
                        let elapsed = now
                            .0
                            .duration_since(previous.0)
                            .as_millis()
                            .min(u128::from(u64::MAX)) as u64;
                        if clock_discontinuity(
                            previous.1,
                            now.1,
                            elapsed,
                            manager.poll.as_millis() as u64,
                        ) {
                            let _ = manager.clock_epoch.fetch_update(
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                                |n| Some(n.saturating_add(1)),
                            );
                            tracing::warn!(
                                before_wall_ms = previous.1,
                                after_wall_ms = now.1,
                                elapsed_ms = elapsed,
                                "Independent timer clock discontinuity"
                            );
                        }
                        manager.clock_samples.fetch_add(1, Ordering::Relaxed);
                        previous = now;
                    }
                })?,
        );
        Ok(())
    }
    pub(crate) fn start(
        self: &Arc<Self>,
        runtime: Arc<Runtime>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        self.start_clock()?;
        let manager = self.clone();
        Ok(tokio::spawn(async move {
            let mut pending = tokio::task::JoinSet::<(String, Result<serde_json::Value>)>::new();
            let mut launching = BTreeSet::<String>::new();
            let mut observed_clock_epoch = manager.clock_epoch.load(Ordering::SeqCst);
            loop {
                tokio::time::sleep(manager.poll).await;
                if manager.closing.load(Ordering::SeqCst) {
                    break;
                }
                while let Some(outcome) = pending.try_join_next() {
                    match outcome {
                        Ok((id, outcome)) => {
                            launching.remove(&id);
                            if let Err(error) = outcome {
                                tracing::warn!(schedule_id=%id,%error,"Automatic launch stopped for inspection");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error,"Automatic launch task lost; no continuation");
                            for id in &launching {
                                let _ = runtime
                                    .engine
                                    .store
                                    .schedule_interrupt_now(id, "launch_failed");
                            }
                            launching.clear();
                        }
                    }
                }
                // Journal/lock processing is not an OS clock/suspend sample.
                // The independent bounded monitor keeps the same thresholds.
                let epoch = manager.clock_epoch.load(Ordering::SeqCst);
                let discontinuity = epoch != observed_clock_epoch;
                observed_clock_epoch = epoch;
                let ids = manager.active.lock().expect("timer active").clone();
                for id in ids {
                    let step = manager
                        .poll_schedule(
                            runtime.engine.clone(),
                            id.clone(),
                            runtime.instance_id.clone(),
                            discontinuity,
                            launching.len() < manager.max_launches,
                            launching.contains(&id),
                        )
                        .await;
                    if manager.closing.load(Ordering::SeqCst) {
                        break;
                    }
                    match step {
                        Ok(Some(trigger))
                            if manager.active.lock().expect("timer active").contains(&id) =>
                        {
                            launching.insert(id.clone());
                            let worker = runtime.clone();
                            let timeout = manager.timeout;
                            pending.spawn(async move {
                                let result = match tokio::time::timeout(timeout,worker.launch_automation_trigger(&trigger.id)).await {
                                    Ok(result) => result,
                                    Err(_) => { let _=worker.engine.store.trigger_interrupt_now(&trigger.id,"launch_failed"); Err(anyhow::anyhow!("Trigger launch timed out; inspect exact retained IDs, no retry")) }
                                };
                                (id,result)
                            });
                        }
                        Ok(_) => {}
                        Err(error) => {
                            // The worker attempted its one error checkpoint. A
                            // lost/panicked worker is not permission to repeat it.
                            tracing::warn!(schedule_id=%id,%error,"Periodic checkpoint needs inspection; no retry");
                            manager.active.lock().expect("timer active").remove(&id);
                        }
                    }
                }
            }
            pending.shutdown().await;
        }))
    }
}
fn clock_discontinuity(before: u64, now: u64, elapsed: u64, poll: u64) -> bool {
    now < before
        || elapsed > 2000.max(poll.saturating_mul(10))
        || now.saturating_sub(before).abs_diff(elapsed) > 500
}
#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_for(check: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !check() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("bounded worker transition");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn periodic_checkpoint_keeps_async_executor_available_while_journal_is_held() {
        let store = crate::jobs::JobStore::in_memory().unwrap();
        let engine = TaskEngine::with_store(crate::event::EventBus::new(10), store.clone());
        let manager = TimerManager::new(&crate::config::Config::load(None).unwrap()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        manager.active.lock().unwrap().insert(id.clone());
        let (ready, held) = tokio::sync::oneshot::channel();
        let (release, hold) = std::sync::mpsc::channel();
        let locked = store.clone();
        let holder = std::thread::spawn(move || {
            locked.with_test_journal_lock(|_| {
                ready.send(()).unwrap();
                let _ = hold.recv_timeout(Duration::from_secs(5));
            })
        });
        held.await.unwrap();
        let worker = manager.clone();
        let checkpoint = tokio::spawn(async move {
            worker
                .poll_schedule(engine, id, "fixture-runtime".into(), false, true, false)
                .await
        });
        wait_for(|| *manager.journal_work.lock().unwrap()).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!checkpoint.is_finished());
        release.send(()).unwrap();
        holder.join().unwrap();
        assert!(
            checkpoint.await.unwrap().is_err(),
            "missing fixture schedule is not authority"
        );
        assert!(!*manager.journal_work.lock().unwrap());
        assert_eq!(store.with_test_journal_lock(|db| db.total_changes()), 0);
        manager.close(&store).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_drains_owned_checkpoint_and_refuses_extra_or_later_workers() {
        let store = crate::jobs::JobStore::in_memory().unwrap();
        let manager = TimerManager::new(&crate::config::Config::load(None).unwrap()).unwrap();
        let owned = manager.begin_journal_work().unwrap().unwrap();
        assert!(manager.begin_journal_work().is_err());
        let closing = manager.clone();
        let stopped = tokio::task::spawn_blocking(move || closing.close(&store));
        wait_for(|| manager.closing.load(Ordering::SeqCst)).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !stopped.is_finished(),
            "shutdown must not forget an owned callback"
        );
        assert!(manager.begin_journal_work().unwrap().is_none());
        drop(owned);
        stopped.await.unwrap().unwrap();
        assert!(!*manager.journal_work.lock().unwrap());
    }

    #[test]
    fn cancelled_waiter_keeps_queued_callback_owned_until_shutdown_observes_it() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let store = crate::jobs::JobStore::in_memory().unwrap();
            let engine = TaskEngine::with_store(crate::event::EventBus::new(10), store.clone());
            let manager = TimerManager::new(&crate::config::Config::load(None).unwrap()).unwrap();
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, held) = std::sync::mpsc::channel();
            let occupied = tokio::task::spawn_blocking(move || {
                started.send(()).unwrap();
                let _ = held.recv_timeout(Duration::from_secs(5));
            });
            ready.await.unwrap();
            let worker = manager.clone();
            let checkpoint = tokio::spawn(async move {
                worker
                    .poll_schedule(
                        engine,
                        uuid::Uuid::new_v4().to_string(),
                        "fixture-runtime".into(),
                        false,
                        true,
                        false,
                    )
                    .await
            });
            wait_for(|| *manager.journal_work.lock().unwrap()).await;
            checkpoint.abort();
            match checkpoint.await {
                Err(error) => assert!(error.is_cancelled()),
                Ok(_) => panic!("queued waiter should be cancelled"),
            }
            assert!(*manager.journal_work.lock().unwrap());
            let closing = manager.clone();
            let journal = store.clone();
            let stopped = std::thread::spawn(move || closing.close(&journal));
            wait_for(|| manager.closing.load(Ordering::SeqCst)).await;
            assert!(*manager.journal_work.lock().unwrap());
            release.send(()).unwrap();
            occupied.await.unwrap();
            stopped.join().unwrap().unwrap();
            assert!(!*manager.journal_work.lock().unwrap());
            assert_eq!(store.with_test_journal_lock(|db| db.total_changes()), 0);
            assert!(manager.begin_journal_work().unwrap().is_none());
        });
    }

    #[test]
    fn backward_wall_steps_forward_jumps_and_long_gaps_suspend_without_catchup() {
        assert!(!clock_discontinuity(1000, 1200, 200, 200));
        assert!(clock_discontinuity(1000, 900, 200, 200));
        assert!(clock_discontinuity(1000, 3000, 200, 200));
        assert!(clock_discontinuity(1000, 5000, 4000, 200));
        assert!(!clock_discontinuity(1000, 1500, 200, 200));
    }
    #[test]
    fn bounded_settings_reject_wrong_types_and_inconsistent_launch_capacity() {
        for text in [
            "poll_ms: 0",
            "poll_ms: bad",
            "max_active: 0",
            "max_launches: 9",
            "max_active: 1\n  max_launches: 2",
            "launch_timeout_ms: 999",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.yaml");
            std::fs::write(&path, format!("automations:\n  {text}\n")).unwrap();
            let config = crate::config::Config::load(Some(&path)).unwrap();
            assert!(TimerManager::new(&config).is_err(), "{text}");
        }
        let manager = TimerManager::new(&crate::config::Config::load(None).unwrap()).unwrap();
        assert!(manager.active.lock().unwrap().is_empty());
        assert!(!manager.closing.load(Ordering::SeqCst));
    }
    #[test]
    fn independent_clock_monitor_keeps_sampling_during_coordinator_processing_pause() {
        let manager = TimerManager::new(&crate::config::Config::load(None).unwrap()).unwrap();
        manager.start_clock().unwrap();
        // Stand-in for ordinary coordinator processing, not a monitor sleep.
        std::thread::sleep(Duration::from_millis(2300));
        assert!(manager.clock_samples.load(Ordering::Relaxed) >= 5);
        assert_eq!(manager.clock_epoch.load(Ordering::SeqCst), 0);
        manager
            .close(&crate::jobs::JobStore::in_memory().unwrap())
            .unwrap();
        assert!(manager.clock_thread.lock().unwrap().is_none());
    }
}
