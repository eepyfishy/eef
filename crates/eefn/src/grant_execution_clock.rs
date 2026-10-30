//! Private execution clock seam for deterministic ownership-order tests.
//! Admission, authenticated leases and production clock selection are unchanged.
use std::time::{Duration, Instant};

pub(crate) trait ExecutionClock: Send + Sync {
    fn wall_ms(&self) -> u64;
    fn elapsed(&self, since: Instant) -> Duration;
    fn reached(&self, deadline: Instant) -> bool;
    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()> + Send;
}

pub(crate) struct SystemClock;

impl ExecutionClock for SystemClock {
    fn wall_ms(&self) -> u64 {
        crate::protocol::now_ms()
    }
    fn elapsed(&self, since: Instant) -> Duration {
        since.elapsed()
    }
    fn reached(&self, deadline: Instant) -> bool {
        Instant::now() >= deadline
    }
    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()> + Send {
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline))
    }
}

/// Only unit tests can select this clock. Authenticated admission still uses
/// the real lease challenge; execution advances the original admitted deadline.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ControlledClock {
    pub(crate) state: std::sync::Arc<std::sync::Mutex<(Instant, u64)>>,
    pub(crate) wake: std::sync::Arc<tokio::sync::Notify>,
}
#[cfg(test)]
impl ControlledClock {
    pub(crate) fn at(accepted: &super::AcceptedGrant) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new((
                accepted.accepted,
                accepted.accepted_wall,
            ))),
            wake: std::sync::Arc::new(tokio::sync::Notify::new()),
        }
    }
    pub(crate) fn advance_to(&self, deadline: Instant) {
        let mut state = self.state.lock().unwrap();
        let elapsed = deadline.duration_since(state.0);
        state.1 += u64::try_from(elapsed.as_millis()).unwrap();
        state.0 = deadline;
        drop(state);
        self.wake.notify_one();
    }
    pub(crate) fn jump_wall(&self) {
        self.state.lock().unwrap().1 += 1001;
    }
}
#[cfg(test)]
impl ExecutionClock for ControlledClock {
    fn wall_ms(&self) -> u64 {
        self.state.lock().unwrap().1
    }
    fn elapsed(&self, since: Instant) -> Duration {
        self.state.lock().unwrap().0.duration_since(since)
    }
    fn reached(&self, deadline: Instant) -> bool {
        self.state.lock().unwrap().0 >= deadline
    }
    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()> + Send {
        async move {
            loop {
                let notified = self.wake.notified();
                if self.reached(deadline) {
                    return;
                }
                notified.await;
            }
        }
    }
}
