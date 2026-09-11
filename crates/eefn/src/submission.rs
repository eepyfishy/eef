//! Bounded local input to the running node connection. Never replay on reconnect.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

pub(crate) struct Submission {
    pub id: String,
    pub kind: String,
    pub payload: Value,
    pub reply: oneshot::Sender<Result<Value>>,
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
        if !matches!(
            kind,
            "message"
                | "network.peers"
                | "jobs.list"
                | "jobs.get"
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
            kind: kind.into(),
            payload,
            reply,
        };
        self.sender
            .lock()
            .expect("submission mailbox")
            .as_ref()
            .context("EEF is not connected. Wait for Connected, then send again.")?
            .try_send(command)
            .map_err(|_| {
                anyhow::anyhow!("Node connection is unavailable or busy. The message was not sent.")
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
