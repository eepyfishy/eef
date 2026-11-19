//! Shared process ownership for metadata and private-image streaming transports.
//! The I/O callback is not a grant, renewal, or source admission boundary.
use super::{BridgeFailure, PythonRuntime};
use anyhow::{Context, Result, bail};
use std::{future::Future, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    process::{ChildStdin, ChildStdout, Command},
    time::{Instant, timeout, timeout_at},
};

pub(super) struct OwnedOutput<T> {
    pub(super) value: T,
    pub(super) owned_exit: Option<crate::process_lifetime::ObservedExit>,
}

impl PythonRuntime {
    pub(super) async fn run_owned_stream<T, F, C>(
        &self,
        command: &mut Command,
        deadline: Instant,
        reservation: Option<crate::capture_ownership::Reservation>,
        io: F,
    ) -> Result<OwnedOutput<T>>
    where
        T: Send,
        F: FnOnce(ChildStdin, ChildStdout) -> C + Send,
        C: Future<Output = Result<T>> + Send,
    {
        if reservation.is_some() && !cfg!(windows) {
            bail!("Owned capture exit observation is unsupported on this platform")
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (mut child, tree) = crate::process_lifetime::spawn_with_job_limits_before(
            command,
            self.limits.max_job_memory_mib,
            self.limits.max_job_cpu_percent,
            deadline,
        )
        .map_err(super::bridge_spawn_failure)?;
        let tree = Arc::new(tree);
        let mut capture = reservation.map(|reservation| reservation.bind(tree.clone()));
        let stdin = child.stdin.take().context("Stream stdin unavailable")?;
        let stdout = child.stdout.take().context("Stream stdout unavailable")?;
        let stderr = child.stderr.take().context("Stream stderr unavailable")?;
        #[allow(unused_mut)]
        let mut owned_exit = None;
        let operation = Box::pin(io(stdin, stdout));
        let outcome = timeout_at(deadline, async {
            let (value, _, status) = tokio::try_join!(
                operation,
                super::read_pipe(
                    stderr,
                    self.limits.max_stderr_bytes,
                    BridgeFailure::StderrLimit
                ),
                async {
                    #[cfg(windows)]
                    let status = if capture.is_some() {
                        tree.wait_root_with_coverage_until(&mut child, deadline)
                            .await
                            .map_err(super::bridge_wait_failure)?
                    } else {
                        child.wait().await.context("Stream child wait failed")?
                    };
                    #[cfg(not(windows))]
                    let status = child.wait().await.context("Stream child wait failed")?;
                    #[cfg(windows)]
                    {
                        if let Some(capture) = &mut capture {
                            let exit = tree.terminate_and_witness_until(deadline).await?;
                            capture.observed_exit(&exit)?;
                            owned_exit = Some(exit);
                        } else {
                            tree.terminate_and_observe_until(deadline).await?;
                        }
                    }
                    Ok::<_, anyhow::Error>(status)
                }
            )?;
            if !status.success() {
                return Err(BridgeFailure::Process.into());
            }
            Ok(value)
        })
        .await
        .map_err(|_| anyhow::Error::from(BridgeFailure::Deadline))
        .and_then(|v| v);
        // Keep the original cleanup boundary, including malformed output and
        // failed consumers. Cancellation leaves exit observation to the owned
        // capture cleanup when this runtime can still observe it.
        if outcome.is_err() {
            if let Some(capture) = &mut capture {
                let _ = capture.retire().await;
            }
        }
        drop(capture);
        drop(tree);
        if outcome.is_err() {
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(1), child.wait()).await;
        }
        outcome.map(|value| OwnedOutput { value, owned_exit })
    }
}
