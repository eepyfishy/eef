//! One node-owned microphone reservation, held through observed Windows job exit.
//! This excludes our competing sources, not other applications or device drivers.
use anyhow::{Result, bail};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
pub(crate) struct Pool(Arc<Semaphore>);
impl Default for Pool {
    fn default() -> Self {
        Self(Arc::new(Semaphore::new(1)))
    }
}
impl Pool {
    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn reserve(&self) -> Result<Reservation> {
        let permit = self.0.clone().try_acquire_owned().map_err(|_| {
            anyhow::anyhow!("Microphone source is busy or its previous exit is unconfirmed")
        })?;
        Ok(Reservation(permit))
    }
}

// Unarmed reservations have started no process and release normally on Drop.
pub(crate) struct Reservation(OwnedSemaphorePermit);
struct Quarantined(Option<OwnedSemaphorePermit>);
impl Quarantined {
    fn release_after_observation(mut self) {
        drop(self.0.take());
    }
}
impl Drop for Quarantined {
    fn drop(&mut self) {
        // An aborted cleanup observer must not optimistically free this source.
        if let Some(permit) = self.0.take() {
            permit.forget();
        }
    }
}
pub(crate) struct OwnedCapture {
    tree: Arc<crate::process_lifetime::ProcessTree>,
    reservation: Option<Quarantined>,
}
impl Reservation {
    pub(crate) fn bind(self, tree: Arc<crate::process_lifetime::ProcessTree>) -> OwnedCapture {
        OwnedCapture {
            tree,
            reservation: Some(Quarantined(Some(self.0))),
        }
    }
}
impl OwnedCapture {
    /// Caller has just observed this exact job empty, not merely a root exit.
    #[cfg(windows)]
    pub(crate) fn observed_empty(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            reservation.release_after_observation();
        }
    }
    pub(crate) async fn retire(&mut self) -> Result<()> {
        if self.reservation.is_none() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            self.tree.request_termination()?;
            // Cleanup observes already-retired authority. This never extends a
            // source's execution deadline or permits further source work.
            self.tree
                .observe_empty_until(
                    tokio::time::Instant::now() + std::time::Duration::from_secs(3),
                )
                .await?;
            self.observed_empty();
            Ok(())
        }
        #[cfg(not(windows))]
        bail!("Owned microphone tree observation is unsupported on this platform")
    }
}
impl Drop for OwnedCapture {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.take() else {
            return;
        };
        #[cfg(windows)]
        {
            // Synchronous termination request precedes any final journal write.
            let _ = self.tree.request_termination();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let tree = self.tree.clone();
                runtime.spawn(async move {
                    if tree
                        .observe_empty_until(
                            tokio::time::Instant::now() + std::time::Duration::from_secs(3),
                        )
                        .await
                        .is_ok()
                    {
                        reservation.release_after_observation();
                    }
                    // Failure, task abort and runtime shutdown quarantine it.
                });
            }
            // Without a runtime the reservation is quarantined on Drop.
        }
        #[cfg(not(windows))]
        drop(reservation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_and_clones_share_one_reservation_until_unarmed_drop() {
        let pool = Pool::default();
        let alias = pool.clone();
        assert!(pool.same(&alias));
        assert!(!pool.same(&Pool::default()));
        let reservation = pool.reserve().unwrap();
        assert!(alias.reserve().is_err());
        drop(reservation);
        assert!(alias.reserve().is_ok());
    }
    #[test]
    fn abandoned_armed_observation_keeps_resource_quarantined() {
        let pool = Pool::default();
        let reservation = pool.reserve().unwrap();
        drop(Quarantined(Some(reservation.0)));
        assert!(pool.reserve().is_err());
    }
    #[test]
    fn positive_exit_observation_releases_exact_reservation() {
        let pool = Pool::default();
        let reservation = pool.reserve().unwrap();
        Quarantined(Some(reservation.0)).release_after_observation();
        assert!(pool.reserve().is_ok());
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn cancelled_capture_reservation_waits_for_held_root_and_descendant_exit() {
        use crate::process_lifetime::{
            spawn,
            tests::{Processes, args},
        };
        let dir = tempfile::tempdir().unwrap();
        let pool = Pool::default();
        let reservation = pool.reserve().unwrap();
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(args("loop", dir.path()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (mut child, tree) = spawn(&mut command).unwrap();
        let capture = reservation.bind(Arc::new(tree));
        let processes = Processes::observe(dir.path()).await;
        processes.alive();
        assert!(pool.reserve().is_err());
        drop(capture);
        // spawn() never polls its cleanup task synchronously. A kill request
        // alone must not release this permit, even if the processes exit fast.
        assert!(pool.reserve().is_err());
        let _next = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(next) = pool.reserve() {
                    assert!(
                        processes.exited_now(),
                        "Both held process handles must be signalled before reuse"
                    );
                    break next;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        child.wait().await.unwrap();
    }
}
