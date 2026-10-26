//! Private execution clock seam for deterministic ownership-order tests.
//! Admission, authenticated leases and production clock selection are unchanged.
use std::time::{Duration, Instant};

pub(super) trait ExecutionClock: Send + Sync {
    fn wall_ms(&self) -> u64;
    fn elapsed(&self, since: Instant) -> Duration;
    fn reached(&self, deadline: Instant) -> bool;
    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()> + Send;
}

pub(super) struct SystemClock;

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
