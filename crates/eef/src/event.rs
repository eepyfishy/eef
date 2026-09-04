use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub topic: String,
    pub data: Value,
    pub timestamp_ms: u64,
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    history: Arc<RwLock<VecDeque<Event>>>,
    history_limit: usize,
}

impl EventBus {
    pub fn new(history_limit: usize) -> Self {
        let (sender, _) = broadcast::channel(512);
        Self {
            sender,
            history: Arc::new(RwLock::new(VecDeque::with_capacity(history_limit))),
            history_limit,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    pub async fn publish(&self, topic: impl Into<String>, data: Value) {
        let event = Event {
            topic: topic.into(),
            data,
            timestamp_ms: eefn::protocol::now_ms(),
        };
        let mut history = self.history.write().await;
        history.push_back(event.clone());
        while history.len() > self.history_limit {
            history.pop_front();
        }
        drop(history);
        let _ = self.sender.send(event);
    }

    pub async fn history(&self, limit: usize) -> Vec<Event> {
        let history = self.history.read().await;
        history
            .iter()
            .skip(history.len().saturating_sub(limit))
            .cloned()
            .collect()
    }
}

pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    pattern
        .strip_suffix(".*")
        .map_or(pattern == topic, |prefix| {
            topic.starts_with(&format!("{prefix}."))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn history_is_bounded() {
        let bus = EventBus::new(2);
        bus.publish("one", json!({})).await;
        bus.publish("two", json!({})).await;
        bus.publish("three", json!({})).await;
        assert_eq!(
            bus.history(10)
                .await
                .iter()
                .map(|event| event.topic.as_str())
                .collect::<Vec<_>>(),
            ["two", "three"]
        );
        assert!(topic_matches("node.*", "node.connected"));
        assert!(!topic_matches("node.*", "model.loaded"));
    }
}
