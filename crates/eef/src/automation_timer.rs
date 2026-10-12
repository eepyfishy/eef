//! Bounded trigger coordination; workload execution remains in the grant driver.
use crate::{Runtime, jobs::automation_schedules::State, task::PlanStatus};
use anyhow::{Result, bail};
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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
        }))
    }
    pub(crate) fn close(&self, store: &crate::jobs::JobStore) -> Result<()> {
        self.closing.store(true, Ordering::SeqCst);
        let ids = self.active.lock().expect("timer active").clone();
        for id in ids {
            store.schedule_interrupt_now(&id, "shutdown")?;
        }
        self.active.lock().expect("timer active").clear();
        Ok(())
    }
    pub(crate) fn start(self: &Arc<Self>, runtime: Arc<Runtime>) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut pending = tokio::task::JoinSet::<(String, Result<serde_json::Value>)>::new();
            let mut launching = BTreeSet::<String>::new();
            let mut previous = (Instant::now(), eefn::protocol::now_ms());
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
                let now = (Instant::now(), eefn::protocol::now_ms());
                let elapsed = now
                    .0
                    .duration_since(previous.0)
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                let discontinuity = clock_discontinuity(
                    previous.1,
                    now.1,
                    elapsed,
                    manager.poll.as_millis() as u64,
                );
                previous = now;
                let ids = manager.active.lock().expect("timer active").clone();
                for id in ids {
                    let step:Result<()>=async {
                        if discontinuity {runtime.engine.store.schedule_interrupt_now(&id,"clock_discontinuity")?;}
                        let s=runtime.engine.store.schedule_get(&id)?;
                        match s.state {
                            State::Waiting=>{
                                if launching.len()>=manager.max_launches {return Ok(());}
                                if let Some(t)=runtime.engine.claim_schedule_due(&id,&runtime.instance_id,&s.issuer_id)? {
                                    launching.insert(id.clone());let worker=runtime.clone();let timeout=manager.timeout;let schedule_id=id.clone();
                                    pending.spawn(async move {
                                        let result=match tokio::time::timeout(timeout,worker.launch_automation_trigger(&t.id)).await {
                                            Ok(result)=>result,
                                            Err(_)=>{let _=worker.engine.store.trigger_interrupt_now(&t.id,"launch_failed");Err(anyhow::anyhow!("Trigger launch timed out; inspect exact retained IDs, no retry"))}
                                        };(schedule_id,result)
                                    });
                                }
                            }
                            State::InFlight if !launching.contains(&id)=>{
                                let trigger=s.active_trigger.as_deref().ok_or_else(||anyhow::anyhow!("Missing active trigger"))?;
                                let p=runtime.engine.store.trigger_job(trigger)?;
                                match p.as_ref().map(|p|p.status) {
                                    Some(PlanStatus::Completed|PlanStatus::Cancelled)=>{runtime.engine.store.schedule_settle_now(&id,&runtime.instance_id,&s.issuer_id)?;},
                                    Some(PlanStatus::Running|PlanStatus::Stopping)=>{},
                                    _=>{runtime.engine.store.trigger_interrupt_now(trigger,"job_unresolved")?;}
                                }
                            }
                            State::InFlight=>{},
                            _=>{manager.active.lock().expect("timer active").remove(&id);}
                        }Ok(())
                    }.await;
                    if let Err(error) = step {
                        tracing::warn!(schedule_id=%id,%error,"Timer checkpoint failed; no catch-up or retry");
                        let _ = runtime
                            .engine
                            .store
                            .schedule_interrupt_now(&id, "job_unresolved");
                        manager.active.lock().expect("timer active").remove(&id);
                    }
                }
            }
            pending.shutdown().await;
        })
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
}
