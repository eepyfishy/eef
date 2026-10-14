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

/// Point-in-time local queue observation, never coordinator acceptance or a lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputChannelState {
    Unavailable,
    Ready,
    Busy,
}

impl SubmissionMailbox {
    pub fn state(&self) -> InputChannelState {
        let sender = self.sender.lock().expect("submission mailbox");
        match sender.as_ref() {
            None => InputChannelState::Unavailable,
            Some(sender) if sender.is_closed() => InputChannelState::Unavailable,
            Some(sender) if sender.capacity() == 0 => InputChannelState::Busy,
            Some(_) => InputChannelState::Ready,
        }
    }
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
                | "requests.capabilities"
                | "requests.planning"
                | "requests.preparation_checkpoint"
                | "requests.concrete"
                | "automations.command"
                | "requests.review"
                | "requests.open"
                | "requests.draft_get"
                | "requests.draft_answer"
                | "requests.draft_revise"
                | "requests.draft_resolve"
                | "requests.execute"
                | "requests.receipt"
                | "requests.result"
                | "requests.recover_result"
                | "requests.abandon"
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
    #[test]
    fn input_state_observes_ready_busy_and_closed_without_draining_or_replaying() {
        let mailbox = SubmissionMailbox::default();
        assert_eq!(mailbox.state(), InputChannelState::Unavailable);
        let mut receiver = mailbox.connect();
        assert_eq!(mailbox.state(), InputChannelState::Ready);
        for i in 0..8 {
            let (reply, _response) = oneshot::channel();
            assert!(
                mailbox
                    .sender
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .try_send(Submission {
                        id: i.to_string(),
                        operation_id: None,
                        kind: "jobs.list".into(),
                        payload: serde_json::json!({}),
                        reply,
                    })
                    .is_ok()
            );
        }
        assert_eq!(mailbox.state(), InputChannelState::Busy);
        assert_eq!(mailbox.state(), InputChannelState::Busy);
        assert_eq!(receiver.try_recv().unwrap().id, "0");
        assert_eq!(mailbox.state(), InputChannelState::Ready);
        drop(receiver);
        assert_eq!(mailbox.state(), InputChannelState::Unavailable);
        let mut replacement = mailbox.connect();
        assert_eq!(mailbox.state(), InputChannelState::Ready);
        assert!(
            replacement.try_recv().is_err(),
            "Old queued input is not replayed onto a new connection"
        );
    }
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
