//! Bounded local input to the running node connection. Never replay on reconnect.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub(crate) struct Submission {
    pub id: String,
    pub operation_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub reply: oneshot::Sender<Result<Value>>,
}

/// Known outcomes only; transport/timeout errors deliberately remain uncertain.
#[derive(Debug, thiserror::Error)]
pub enum SubmissionFailure {
    #[error("{0}")]
    NotSent(String),
    #[error("{0}")]
    Rejected(String),
}

#[derive(Default)]
pub struct SubmissionMailbox {
    sender: Mutex<Option<mpsc::Sender<Submission>>>,
}

impl SubmissionMailbox {
    pub(crate) fn connect(&self) -> mpsc::Receiver<Submission> {
        let (sender, receiver) = mpsc::channel(8);
        *self.sender.lock().expect("submission mailbox") = Some(sender);
        receiver
    }

    pub async fn submit(self: &Arc<Self>, text: &str) -> Result<Value> {
        if text.trim().is_empty() || text.len() > 32 * 1024 {
            bail!("message must contain between 1 and 32768 bytes")
        }
        self.request("message", serde_json::json!({"text":text}))
            .await
    }

    pub async fn request(self: &Arc<Self>, kind: &str, payload: Value) -> Result<Value> {
        self.request_correlated(kind, payload, None).await
    }

    /// Correlation is separate from the fresh transport ID: reusing a receipt
    /// must never route a late reply to a different waiter. This is not deduplication.
    pub(crate) async fn request_correlated(
        self: &Arc<Self>,
        kind: &str,
        payload: Value,
        operation_id: Option<String>,
    ) -> Result<Value> {
        if let Some(id) = &operation_id {
            crate::job_commands::validate_operation_id(id)?;
        }
        if !matches!(
            kind,
            "message"
                | "network.peers"
                | "jobs.list"
                | "jobs.find"
                | "jobs.get"
                | "jobs.output"
                | "jobs.create"
                | "jobs.pause"
                | "jobs.resume"
                | "jobs.stop"
                | "jobs.remove"
        ) || serde_json::to_vec(&payload)?.len() > 64 * 1024
        {
            bail!("Unsupported or oversized node request")
        }
        let (reply, response) = oneshot::channel();
        let command = Submission {
            id: uuid::Uuid::new_v4().simple().to_string(),
            operation_id,
            kind: kind.into(),
            payload,
            reply,
        };
        self.sender
            .lock()
            .expect("submission mailbox")
            .as_ref()
            .ok_or_else(|| {
                SubmissionFailure::NotSent(
                    "EEF is not connected. Wait for Connected, then send again.".into(),
                )
            })?
            .try_send(command)
            .map_err(|_| {
                SubmissionFailure::NotSent(
                    "Node connection is unavailable or busy. The message was not sent.".into(),
                )
            })?;
        tokio::time::timeout(Duration::from_secs(120), response).await
            .context("No reply within two minutes. Work may still be running; the message was not automatically retried.")?
            .context("Connection ended before a reply. Work may already have run; the message was not automatically retried.")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reused_correlation_cannot_replace_waiters_or_match_late_transport_replies() {
        let mailbox = Arc::new(SubmissionMailbox::default());
        let mut receiver = mailbox.connect();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let start = || {
            let mailbox = mailbox.clone();
            let id = operation_id.clone();
            tokio::spawn(async move {
                mailbox
                    .request_correlated("jobs.list", serde_json::json!({}), Some(id))
                    .await
            })
        };
        let first = start();
        let a = receiver.recv().await.unwrap();
        let second = start();
        let b = receiver.recv().await.unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.operation_id.as_deref(), Some(operation_id.as_str()));
        assert_eq!(a.operation_id, b.operation_id);
        b.reply.send(Ok(serde_json::json!("second"))).unwrap();
        a.reply.send(Ok(serde_json::json!("first"))).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), "first");
        assert_eq!(second.await.unwrap().unwrap(), "second");
    }

    #[tokio::test]
    async fn disconnected_input_is_not_queued_for_later() {
        let mailbox = Arc::new(SubmissionMailbox::default());
        assert!(mailbox.submit("hello").await.is_err());
        let mut receiver = mailbox.connect();
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        assert!(mailbox.submit("hello").await.is_err());
    }
    #[tokio::test]
    async fn connection_loss_does_not_replay_submitted_work() {
        let mailbox = Arc::new(SubmissionMailbox::default());
        let mut receiver = mailbox.connect();
        let local = mailbox.clone();
        let task = tokio::spawn(async move { local.submit("hello").await });
        let command = receiver.recv().await.unwrap();
        assert_eq!(command.kind, "message");
        assert_eq!(command.payload["text"], "hello");
        drop(command);
        drop(receiver);
        assert!(
            task.await
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("not automatically retried")
        );
        assert!(mailbox.connect().try_recv().is_err());
    }
}
