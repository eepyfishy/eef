//! Bounded writes on one existing encrypted connection. No replay or new framing.
use crate::{NodeCrypto, protocol::write_message};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::time::Duration;
use tokio::{
    io::AsyncWrite,
    sync::{Mutex, watch},
    time::timeout,
};

pub(crate) const WRITE_BUDGET: Duration = Duration::from_secs(5);

/// Arm only after acquiring the writer. Cancellation must mark the session
/// unusable before releasing its lock: a partial encrypted line is not reusable.
pub(crate) struct WriteGuard {
    signal: watch::Sender<bool>,
    completed: bool,
}
impl WriteGuard {
    pub fn new(signal: watch::Sender<bool>) -> Self {
        Self {
            signal,
            completed: false,
        }
    }
    pub fn complete(&mut self) {
        self.completed = true;
    }
}
impl Drop for WriteGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.signal.send_replace(true);
        }
    }
}

pub(crate) async fn send<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
) -> Result<()> {
    send_with_budget(crypto, writer, failed, message, WRITE_BUDGET).await
}

async fn send_with_budget<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
    budget: Duration,
) -> Result<()> {
    if *failed.borrow() {
        bail!("framed session output is retired; no frame was sent")
    }
    let result = timeout(budget, async {
        let mut output = writer.lock().await;
        // Another write may fail while this one waits. Never append to its prefix.
        if *failed.borrow() {
            bail!("framed session output is retired; no frame was sent")
        }
        // Declaration order ensures the guard poisons before the lock is dropped.
        let mut guard = WriteGuard::new(failed.clone());
        let result = write_message(crypto, &mut *output, message).await;
        if result.is_ok() {
            guard.complete();
        }
        result
    })
    .await
    .context("framed session write timed out; delivery is unknown")
    .and_then(|result| result);
    if result.is_err() {
        // Queue-wait timeout may not have acquired the lock, but still retires
        // the connection. No hidden retry or claim of rollback/not-started.
        failed.send_replace(true);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, BufReader};

    #[tokio::test]
    async fn completed_frames_preserve_protocol_and_keep_session_writable() {
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, input) = tokio::io::duplex(4096);
        let writer = Mutex::new(output);
        let (signal, receiver) = watch::channel(false);
        let mut input = BufReader::new(input);
        for value in [
            json!({"type":"heartbeat"}),
            json!({"type":"response","success":true}),
        ] {
            send_with_budget(&crypto, &writer, &signal, &value, Duration::from_secs(1))
                .await
                .unwrap();
            assert!(!*receiver.borrow());
            assert_eq!(
                crate::protocol::read_message(&crypto, &mut input)
                    .await
                    .unwrap(),
                Some(value)
            );
        }
    }

    #[tokio::test]
    async fn partial_write_timeout_poisons_before_any_later_frame_can_append() {
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, mut input) = tokio::io::duplex(32);
        let writer = Mutex::new(output);
        let (signal, receiver) = watch::channel(false);
        let value = json!({"data":"x".repeat(4096)});
        assert!(
            send_with_budget(&crypto, &writer, &signal, &value, Duration::from_millis(30))
                .await
                .is_err()
        );
        assert!(*receiver.borrow());
        assert!(
            send(&crypto, &writer, &signal, &json!({"later":true}))
                .await
                .is_err()
        );
        drop(writer);
        let mut bytes = vec![];
        input.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes.len(), 32);
        assert!(!bytes.contains(&b'\n'));
    }

    #[tokio::test]
    async fn queued_write_timeout_and_io_failure_retire_without_receivers() {
        for queue in [false, true] {
            let crypto = NodeCrypto::new("test-only").unwrap();
            let (output, input) = tokio::io::duplex(4096);
            drop(input);
            let writer = Mutex::new(output);
            let (signal, receiver) = watch::channel(false);
            drop(receiver);
            let held = if queue {
                Some(writer.lock().await)
            } else {
                None
            };
            assert!(
                send_with_budget(
                    &crypto,
                    &writer,
                    &signal,
                    &json!({"data":"test"}),
                    Duration::from_millis(30)
                )
                .await
                .is_err()
            );
            assert!(*signal.borrow());
            drop(held);
            assert!(
                send(&crypto, &writer, &signal, &json!({"later":true}))
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("retired")
            );
        }
    }

    #[tokio::test]
    async fn cancelling_partial_write_signals_retirement_and_never_completes_the_prefix() {
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, mut input) = tokio::io::duplex(32);
        let writer = Arc::new(Mutex::new(output));
        let (signal, receiver) = watch::channel(false);
        let child_writer = writer.clone();
        let child_signal = signal.clone();
        let child_crypto = crypto.clone();
        let child = tokio::spawn(async move {
            send_with_budget(
                &child_crypto,
                &child_writer,
                &child_signal,
                &json!({"data":"x".repeat(4096)}),
                Duration::from_secs(30),
            )
            .await
        });
        let mut first = [0; 32];
        input.read_exact(&mut first).await.unwrap();
        child.abort();
        assert!(child.await.unwrap_err().is_cancelled());
        assert!(*receiver.borrow());
        assert!(
            send(&crypto, &writer, &signal, &json!({"later":true}))
                .await
                .is_err()
        );
        drop(writer);
        let mut remaining = vec![];
        input.read_to_end(&mut remaining).await.unwrap();
        assert!(!first.contains(&b'\n') && !remaining.contains(&b'\n'));
        assert!(remaining.len() <= 32);
    }
}
