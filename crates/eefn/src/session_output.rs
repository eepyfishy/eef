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

/// Terminal reply only: execution ends, but bounded reply ownership survives
/// encoding, queue wait, flush, failure and cancellation.
pub(crate) async fn send_completed<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
    admission: crate::request_limits::Admission,
) -> Result<()> {
    send_reply(
        crypto,
        writer,
        failed,
        message,
        Some(admission.into_reply()),
    )
    .await
}

/// An admitted terminal reply owns its reservation; unadmitted error replies
/// use None only on the already bounded synchronous receive path.
pub(crate) async fn send_reply<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
    reply: Option<crate::request_limits::ReplyPermit>,
) -> Result<()> {
    send_owned_with_budget(crypto, writer, failed, message, WRITE_BUDGET, reply).await
}

async fn send_with_budget<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
    budget: Duration,
) -> Result<()> {
    send_owned_with_budget(crypto, writer, failed, message, budget, None).await
}

async fn send_owned_with_budget<W: AsyncWrite + Unpin>(
    crypto: &NodeCrypto,
    writer: &Mutex<W>,
    failed: &watch::Sender<bool>,
    message: &Value,
    budget: Duration,
    mut reply: Option<crate::request_limits::ReplyPermit>,
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
            // Release successful output ownership before the next writer can
            // take the lock. Do not leave completed reservations after handoff.
            drop(reply.take());
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

    struct GatedFlush {
        output: tokio::io::DuplexStream,
        release: tokio::sync::oneshot::Receiver<()>,
    }
    impl AsyncWrite for GatedFlush {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            bytes: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.output).poll_write(cx, bytes)
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if std::future::Future::poll(std::pin::Pin::new(&mut self.release), cx).is_pending() {
                return std::task::Poll::Pending;
            }
            std::pin::Pin::new(&mut self.output).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.output).poll_shutdown(cx)
        }
    }

    #[tokio::test]
    async fn completed_reply_does_not_keep_execution_busy_after_peer_receives_it() {
        let budget =
            crate::request_limits::RequestBudget::new(&crate::request_limits::RequestLimits {
                work: 1,
                control: 1,
            })
            .unwrap();
        let admission = budget.admit("node.configure", "model_command").unwrap();
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, input) = tokio::io::duplex(4096);
        let (release, gate) = tokio::sync::oneshot::channel();
        let writer = Arc::new(Mutex::new(GatedFlush {
            output,
            release: gate,
        }));
        let (failed, _) = watch::channel(false);
        let response = json!({"type":"response","id":"first","success":true});
        let task = tokio::spawn({
            let crypto = crypto.clone();
            let writer = writer.clone();
            let response = response.clone();
            async move { send_completed(&crypto, &writer, &failed, &response, admission).await }
        });
        let mut input = BufReader::new(input);
        let received = tokio::time::timeout(
            Duration::from_secs(1),
            crate::protocol::read_message(&crypto, &mut input),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert_eq!(received, response);
        assert!(
            !task.is_finished(),
            "the complete frame is visible while flush is pending"
        );
        let next = budget.admit("node.configure", "model_command");
        release.send(()).unwrap();
        task.await.unwrap().unwrap();
        assert!(
            next.is_some(),
            "a completed reply must not make the peer's next sequential command falsely busy"
        );
    }

    #[tokio::test]
    async fn pending_completed_replies_stay_bounded_and_cancel_without_starving_controls() {
        let budget =
            crate::request_limits::RequestBudget::new(&crate::request_limits::RequestLimits {
                work: 1,
                control: 1,
            })
            .unwrap();
        let admission = budget.admit("node.configure", "model_command").unwrap();
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, input) = tokio::io::duplex(4096);
        let (_release, gate) = tokio::sync::oneshot::channel();
        let writer = Arc::new(Mutex::new(GatedFlush {
            output,
            release: gate,
        }));
        let (failed, retired) = watch::channel(false);
        let task = tokio::spawn({
            let crypto = crypto.clone();
            let writer = writer.clone();
            async move {
                send_completed(
                    &crypto,
                    &writer,
                    &failed,
                    &json!({"type":"response","id":"first"}),
                    admission,
                )
                .await
            }
        });
        let mut input = BufReader::new(input);
        tokio::time::timeout(
            Duration::from_secs(1),
            crate::protocol::read_message(&crypto, &mut input),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let second = budget.admit("node.configure", "model_command").unwrap();
        assert!(
            budget.admit("node.configure", "model_command").is_none(),
            "execution cap remains one"
        );
        let second_reply = second.into_reply();
        assert!(
            budget.admit("node.configure", "model_command").is_none(),
            "pending reply capacity is bounded before execution"
        );
        assert!(
            budget.admit_receipt().is_some(),
            "work replies do not consume reserved controls"
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(
            *retired.borrow(),
            "an unconfirmed flush retires framing before reply ownership is released"
        );
        assert!(budget.admit("node.configure", "model_command").is_some());
        drop(second_reply);
    }

    #[tokio::test]
    async fn completed_reply_io_failure_returns_capacity_but_retires_the_session() {
        let budget =
            crate::request_limits::RequestBudget::new(&crate::request_limits::RequestLimits {
                work: 1,
                control: 1,
            })
            .unwrap();
        let admission = budget.admit("node.configure", "model_command").unwrap();
        let crypto = NodeCrypto::new("test-only").unwrap();
        let (output, input) = tokio::io::duplex(32);
        drop(input);
        let writer = Mutex::new(output);
        let (failed, retired) = watch::channel(false);
        assert!(
            send_completed(
                &crypto,
                &writer,
                &failed,
                &json!({"type":"response"}),
                admission
            )
            .await
            .is_err()
        );
        assert!(*retired.borrow());
        assert!(budget.admit("node.configure", "model_command").is_some());
        assert!(budget.admit_receipt().is_some());
        assert!(
            send(&crypto, &writer, &failed, &json!({"later":true}))
                .await
                .is_err()
        );
    }

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
