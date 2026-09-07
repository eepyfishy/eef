use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use crate::event::{Event, EventBus};

pub struct WorldState {
    state: RwLock<State>,
    started: Instant,
}

#[derive(Default)]
struct State {
    applications: BTreeMap<String, Value>,
    nodes: BTreeMap<String, Value>,
    tasks: BTreeMap<String, Value>,
    sensors: BTreeMap<String, Value>,
    gpio: BTreeMap<String, Value>,
    services: BTreeMap<String, Value>,
    clipboard: Option<String>,
    screen_content: Option<String>,
    network_status: String,
}

impl WorldState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: RwLock::new(State {
                network_status: "unknown".into(),
                ..State::default()
            }),
            started: Instant::now(),
        })
    }

    pub fn start(self: &Arc<Self>, bus: &EventBus) -> JoinHandle<()> {
        let mut events = bus.subscribe();
        let world = self.clone();
        tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                world.apply(event).await
            }
        })
    }

    async fn apply(&self, event: Event) {
        let mut state = self.state.write().await;
        match event.topic.as_str() {
            "application.launched" => {
                if let Some(name) = event.data.get("name").and_then(Value::as_str) {
                    state.applications.insert(
                        name.into(),
                        json!({"name": name, "pid": event.data.get("pid"), "running": true}),
                    );
                }
            }
            "application.closed" => {
                if let Some(name) = event.data.get("name").and_then(Value::as_str) {
                    if let Some(app) = state.applications.get_mut(name) {
                        app["running"] = Value::Bool(false);
                    }
                }
            }
            "node.connected" => {
                if let Some(id) = event.data.get("node_id").and_then(Value::as_str) {
                    state.nodes.insert(id.into(), json!({
                    "node_id": id, "name": event.data.get("name").and_then(Value::as_str).unwrap_or(id),
                    "capabilities": event.data.get("capabilities").cloned().unwrap_or_else(|| json!([])), "connected": true,
                    "last_seen_ms":eefn::protocol::now_ms(),"specs":event.data.get("specs"),"models":event.data.get("models"),"metadata":event.data.get("metadata"),
                }));
                }
            }
            "node.updated" => {
                if let Some(id) = event.data.get("node_id").and_then(Value::as_str) {
                    if let Some(node) = state.nodes.get_mut(id) {
                        node["last_seen_ms"] = json!(eefn::protocol::now_ms());
                        if let Some(load) = event.data.get("load") {
                            node["load"] = load.clone();
                        }
                        if let Some(caps) = event.data.get("capabilities") {
                            node["capabilities"] = caps.clone();
                        }
                    }
                }
            }
            "node.disconnected" => {
                if let Some(id) = event.data.get("node_id").and_then(Value::as_str) {
                    if let Some(node) = state.nodes.get_mut(id) {
                        node["connected"] = Value::Bool(false);
                    }
                }
            }
            "task.started" => {
                if let Some(id) = event.data.get("task_id").and_then(Value::as_str) {
                    let mut task = event.data.clone();
                    task["id"] = Value::String(id.into());
                    task["status"] = Value::String("running".into());
                    state.tasks.insert(id.into(), task);
                }
            }
            "task.completed" => {
                if let Some(id) = event.data.get("task_id").and_then(Value::as_str) {
                    if let Some(task) = state.tasks.get_mut(id) {
                        task["status"] = Value::String(
                            if event
                                .data
                                .get("success")
                                .and_then(Value::as_bool)
                                .unwrap_or(true)
                            {
                                "completed"
                            } else {
                                "failed"
                            }
                            .into(),
                        );
                        task["progress"] = json!(1.0);
                    }
                }
            }
            "sensor.updated" => {
                if let Some(object) = event.data.as_object() {
                    for (key, value) in object {
                        if key != "node_id" {
                            state.sensors.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
            "gpio.changed" => {
                if let Some(pin) = event.data.get("pin") {
                    state.gpio.insert(
                        pin.to_string().trim_matches('"').into(),
                        event.data.get("value").cloned().unwrap_or(Value::Null),
                    );
                }
            }
            "service.connected" | "service.disconnected" => {
                if let Some(name) = event.data.get("name").and_then(Value::as_str) {
                    state.services.insert(
                        name.into(),
                        json!({"name": name, "connected": event.topic.ends_with("connected")}),
                    );
                }
            }
            "clipboard.updated" => {
                state.clipboard = event
                    .data
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }
            "network.changed" => {
                if let Some(value) = event.data.get("status").and_then(Value::as_str) {
                    state.network_status = value.into();
                }
            }
            _ => {}
        }
    }

    pub async fn summary(&self) -> Value {
        let state = self.state.read().await;
        json!({
            "running_applications": state.applications.values().filter(|app| app.get("running").and_then(Value::as_bool) == Some(true)).collect::<Vec<_>>(),
            "connected_nodes": state.nodes.values().filter(|node| node.get("connected").and_then(Value::as_bool) == Some(true)).collect::<Vec<_>>(),
            "devices":state.nodes.values().collect::<Vec<_>>(),
            "active_tasks": state.tasks.values().collect::<Vec<_>>(),
            "sensors": state.sensors,
            "gpio_states": state.gpio,
            "services": state.services,
            "clipboard": state.clipboard,
            "screen_content": state.screen_content,
            "network_status": state.network_status,
            "uptime": self.started.elapsed().as_secs_f64(),
        })
    }
}
