//! Bounded serialized report envelopes; never execution permission or RAM proof.
use std::{io, sync::Arc};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

pub(super) struct Envelope {
    pub(super) message: Value,
    _bytes: OwnedSemaphorePermit,
}
impl Envelope {
    pub(super) fn into_parts(self) -> (Value, OwnedSemaphorePermit) {
        (self.message, self._bytes)
    }
}
pub(super) struct Queue {
    sender: mpsc::Sender<Envelope>,
    bytes: Arc<Semaphore>,
}
struct Size {
    bytes: usize,
}
impl io::Write for Size {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .filter(|size| *size <= crate::protocol::MAX_WIRE_LINE)
            .ok_or_else(|| io::Error::other("Report envelope exceeds wire bound"))?;
        Ok(buffer.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Queue {
    pub(super) fn new(count: usize, bytes: Arc<Semaphore>) -> (Self, mpsc::Receiver<Envelope>) {
        let (sender, receiver) = mpsc::channel(count);
        (Self { sender, bytes }, receiver)
    }
    pub(super) fn try_send(&self, message: Value) -> Result<()> {
        let mut size = Size { bytes: 0 };
        // Count without making another serialized payload allocation.
        serde_json::to_writer(&mut size, &message)?;
        let permit = self
            .bytes
            .clone()
            .try_acquire_many_owned(u32::try_from(size.bytes)?)
            .context("Report byte allowance exhausted; session must retire")?;
        self.sender
            .try_send(Envelope {
                message,
                _bytes: permit,
            })
            .map_err(|_| anyhow::anyhow!("Report queue unavailable/full; session must retire"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn count_limit_and_cancellation_release_byte_allowance() {
        let bytes = Arc::new(Semaphore::new(100));
        let (queue, receiver) = Queue::new(1, bytes.clone());
        let value = json!({"id":"one"});
        let size = serde_json::to_vec(&value).unwrap().len();
        queue.try_send(value.clone()).unwrap();
        assert_eq!(bytes.available_permits(), 100 - size);
        assert!(queue.try_send(value).is_err());
        assert_eq!(bytes.available_permits(), 100 - size);
        drop(receiver);
        assert_eq!(bytes.available_permits(), 100);
        assert!(queue.try_send(json!(null)).is_err());
        assert_eq!(bytes.available_permits(), 100);
    }
    #[tokio::test]
    async fn global_bytes_include_dequeued_report_until_processing_drops_it() {
        let bytes = Arc::new(Semaphore::new(8));
        let (first, mut receiver) = Queue::new(4, bytes.clone());
        let (second, other) = Queue::new(4, bytes.clone());
        first.try_send(json!(null)).unwrap();
        first.try_send(json!(null)).unwrap();
        let active = receiver.recv().await.unwrap();
        assert_eq!(bytes.available_permits(), 0);
        assert!(second.try_send(json!(null)).is_err());
        drop(active);
        assert_eq!(bytes.available_permits(), 4);
        second.try_send(json!(null)).unwrap();
        assert_eq!(bytes.available_permits(), 0);
        drop(other);
        assert_eq!(bytes.available_permits(), 4);
        drop(receiver);
        assert_eq!(bytes.available_permits(), 8);
    }
    #[test]
    fn size_matches_canonical_unicode_escaping_and_refuses_oversized_envelopes() {
        for value in [
            json!(null),
            json!({"x":"ไทย\n\"\\"}),
            json!([1, true, {"x":2}]),
        ] {
            let mut size = Size { bytes: 0 };
            serde_json::to_writer(&mut size, &value).unwrap();
            assert_eq!(size.bytes, serde_json::to_vec(&value).unwrap().len());
        }
        let mut size = Size {
            bytes: crate::protocol::MAX_WIRE_LINE,
        };
        assert!(io::Write::write(&mut size, b"x").is_err());
    }
    #[tokio::test]
    async fn queue_is_fifo_and_dropping_sender_preserves_already_queued_envelopes() {
        let bytes = Arc::new(Semaphore::new(100));
        let (queue, mut receiver) = Queue::new(2, bytes.clone());
        queue.try_send(json!(1)).unwrap();
        queue.try_send(json!(2)).unwrap();
        drop(queue);
        assert_eq!(receiver.recv().await.unwrap().message, json!(1));
        assert_eq!(receiver.recv().await.unwrap().message, json!(2));
        assert!(receiver.recv().await.is_none());
        assert_eq!(bytes.available_permits(), 100);
    }
}
