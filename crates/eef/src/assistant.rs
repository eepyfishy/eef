use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::event::{EventBus, topic_matches};
use crate::model::{ChatOptions, LlmService};
use crate::task::Dispatcher;
use crate::world::WorldState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponseRule {
    #[serde(default = "rule_id")]
    pub id: String,
    #[serde(default = "rule_name")]
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default = "keyword")]
    pub trigger: String,
    #[serde(default)]
    pub trigger_value: String,
    #[serde(default)]
    pub condition: Map<String, Value>,
    #[serde(default = "text_kind")]
    pub response_kind: String,
    #[serde(default)]
    pub response: Map<String, Value>,
    #[serde(default = "cooldown")]
    pub cooldown_seconds: f64,
}

fn rule_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].into()
}
fn rule_name() -> String {
    "rule".into()
}
fn keyword() -> String {
    "keyword".into()
}
fn text_kind() -> String {
    "text".into()
}
const fn enabled() -> bool {
    true
}
const fn cooldown() -> f64 {
    30.0
}

impl ResponseRule {
    pub fn text(name: &str, keyword: &str, response: &str, cooldown_seconds: f64) -> Self {
        Self {
            id: rule_id(),
            name: name.into(),
            enabled: true,
            trigger: "keyword".into(),
            trigger_value: keyword.into(),
            condition: Map::new(),
            response_kind: "text".into(),
            response: Map::from_iter([("text".into(), Value::String(response.into()))]),
            cooldown_seconds,
        }
    }
}

pub struct ResponseEngine {
    bus: EventBus,
    world: Arc<WorldState>,
    llm: Arc<LlmService>,
    dispatcher: Arc<Dispatcher>,
    rules: RwLock<Vec<ResponseRule>>,
    last_fired: Mutex<HashMap<String, Instant>>,
    firings: AtomicU64,
    event_loss_count: AtomicU64,
}

impl ResponseEngine {
    pub fn new(
        bus: EventBus,
        world: Arc<WorldState>,
        llm: Arc<LlmService>,
        dispatcher: Arc<Dispatcher>,
    ) -> Arc<Self> {
        Arc::new(Self {
            bus,
            world,
            llm,
            dispatcher,
            rules: RwLock::new(Vec::new()),
            last_fired: Mutex::new(HashMap::new()),
            firings: AtomicU64::new(0),
            event_loss_count: AtomicU64::new(0),
        })
    }

    pub fn add_rule(&self, rule: ResponseRule) -> Result<()> {
        if !matches!(rule.trigger.as_str(), "event" | "keyword" | "world") {
            bail!("unsupported rule trigger '{}'", rule.trigger)
        }
        if !matches!(rule.response_kind.as_str(), "text" | "llm" | "capability") {
            bail!("unsupported response kind '{}'", rule.response_kind)
        }
        let mut rules = self.rules.write().expect("rules lock");
        if rules.iter().any(|old| old.id == rule.id) {
            bail!("duplicate rule id '{}'", rule.id)
        }
        rules.push(rule);
        Ok(())
    }

    pub fn remove_rule(&self, id: &str) -> bool {
        let mut rules = self.rules.write().expect("rules lock");
        let before = rules.len();
        rules.retain(|rule| rule.id != id);
        before != rules.len()
    }

    pub fn toggle_rule(&self, id: &str) -> Option<bool> {
        let mut rules = self.rules.write().expect("rules lock");
        let rule = rules.iter_mut().find(|rule| rule.id == id)?;
        rule.enabled = !rule.enabled;
        Some(rule.enabled)
    }

    pub fn list_rules(&self) -> Vec<ResponseRule> {
        self.rules.read().expect("rules lock").clone()
    }
    pub fn firings(&self) -> u64 {
        self.firings.load(Ordering::Relaxed)
    }
    pub fn event_loss_count(&self) -> u64 {
        self.event_loss_count.load(Ordering::Relaxed)
    }

    pub fn start_event_listener(self: &Arc<Self>) -> JoinHandle<()> {
        self.event_listener(self.bus.subscribe())
    }

    fn event_listener(
        self: &Arc<Self>,
        mut events: tokio::sync::broadcast::Receiver<crate::event::Event>,
    ) -> JoinHandle<()> {
        let engine = self.clone();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) if event.topic != "assistant.response" => {
                        engine.on_event(&event.topic, &event.data).await;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        let _ = engine.event_loss_count.fetch_update(
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                            |value| Some(value.saturating_add(count)),
                        );
                        tracing::warn!(count, "assistant listener missed events; no event replay");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    pub async fn on_keyword(&self, input: &str) {
        self.on_keyword_with_context(input, None).await;
    }

    pub async fn on_keyword_with_context(
        &self,
        input: &str,
        context: Option<&eefn::context::RequestContext>,
    ) {
        let lowered = input.to_lowercase();
        let rules = self.enabled("keyword");
        for rule in rules {
            if lowered.contains(&rule.trigger_value.to_lowercase()) {
                self.maybe_fire(rule, json!({"text": input,"request_context":context}))
                    .await;
            }
        }
    }

    pub async fn on_event(&self, topic: &str, data: &Value) {
        for rule in self.enabled("event") {
            if rule.trigger_value == "*" || topic_matches(&rule.trigger_value, topic) {
                self.maybe_fire(rule, json!({"event": topic, "data": data}))
                    .await;
            }
        }
    }

    pub async fn run_cycle(&self) {
        let summary = self.world.summary().await;
        // Missing observation is not permission to act on an apparently unchanged
        // world. Explicit commands and event/keyword rules remain separate paths.
        if summary["state_may_be_stale"] == true {
            return;
        }
        for rule in self.enabled("world") {
            self.maybe_fire(rule, summary.clone()).await;
        }
    }

    fn enabled(&self, trigger: &str) -> Vec<ResponseRule> {
        self.rules
            .read()
            .expect("rules lock")
            .iter()
            .filter(|rule| rule.enabled && rule.trigger == trigger)
            .cloned()
            .collect()
    }

    async fn maybe_fire(&self, rule: ResponseRule, context: Value) {
        if !conditions_match(&rule.condition, &context) {
            return;
        }
        {
            let mut last = self.last_fired.lock().expect("cooldown lock");
            if last.get(&rule.id).is_some_and(|time| {
                time.elapsed() < Duration::from_secs_f64(rule.cooldown_seconds.max(0.0))
            }) {
                return;
            }
            last.insert(rule.id.clone(), Instant::now());
        }
        match self.fire(&rule, context).await {
            Ok(()) => { self.firings.fetch_add(1, Ordering::Relaxed); }
            Err(error) => self.bus.publish("assistant.rule_failed", json!({"rule_id": rule.id, "error": error.to_string(), "error_type": "RuleExecutionError"})).await,
        }
    }

    async fn fire(&self, rule: &ResponseRule, context: Value) -> Result<()> {
        let origin: Option<eefn::context::RequestContext> = context
            .get("request_context")
            .or_else(|| context.pointer("/data/request_context"))
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?;
        let payload = match rule.response_kind.as_str() {
            "text" => {
                json!({"content": rule.response.get("text").and_then(Value::as_str).unwrap_or(""), "rule_id": rule.id})
            }
            "llm" => {
                let prompt = rule
                    .response
                    .get("prompt")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let content = self
                    .llm
                    .chat(
                        json!([{"role": "user", "content": prompt}]),
                        ChatOptions {
                            request_context: origin.clone(),
                            tier: rule
                                .response
                                .get("tier")
                                .and_then(Value::as_str)
                                .unwrap_or("fast")
                                .into(),
                            ..ChatOptions::default()
                        },
                    )
                    .await?;
                json!({"content": content, "rule_id": rule.id})
            }
            "capability" => {
                let capability = rule
                    .response
                    .get("capability")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("capability response needs 'capability'"))?;
                let action = rule
                    .response
                    .get("action")
                    .and_then(Value::as_str)
                    .unwrap_or("run");
                let params = rule
                    .response
                    .get("params")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let data = self
                    .dispatcher
                    .run_capability_with_context(capability, action, params, json!({}), origin)
                    .await?;
                json!({"content": format!("[capability] {capability}"), "data": data, "rule_id": rule.id})
            }
            kind => bail!("unsupported response kind '{kind}'"),
        };
        self.bus
            .publish("assistant.response", merge_context(payload, context))
            .await;
        Ok(())
    }
}

fn dot_get<'a>(context: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.strip_prefix("world.").unwrap_or(path);
    path.split('.')
        .try_fold(context, |value, part| value.get(part))
}

fn conditions_match(conditions: &Map<String, Value>, context: &Value) -> bool {
    conditions
        .iter()
        .all(|(path, expected)| dot_get(context, path) == Some(expected))
}

fn merge_context(mut payload: Value, context: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert("context".into(), context);
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn event_listener_survives_lag_without_replay_and_world_rules_suspend_on_stale_state() {
        let bus = EventBus::new(8);
        let world = WorldState::new();
        let world_receiver = bus.subscribe();
        let server = eefn::NodeServer::new("isolated-test-secret", "127.0.0.1", 0).unwrap();
        let registry = crate::capability::CapabilityRegistry::default();
        let llm = LlmService::legacy_for_test(
            crate::model::ModelRegistry::default(),
            Default::default(),
            server.clone(),
            bus.clone(),
        );
        let dispatcher = Dispatcher::legacy_for_test(
            registry.clone(),
            crate::adapters::AdapterManager::new(registry, json!({})),
            llm.clone(),
            server,
        );
        let engine = ResponseEngine::new(bus.clone(), world.clone(), llm, dispatcher);
        let mut rule = ResponseRule::text("fixture", "unused", "response", 0.0);
        rule.trigger = "event".into();
        rule.trigger_value = "fixture.fire".into();
        engine.add_rule(rule).unwrap();
        let mut rule = ResponseRule::text("world fixture", "unused", "world response", 0.0);
        rule.trigger = "world".into();
        engine.add_rule(rule).unwrap();
        let receiver = bus.subscribe();
        bus.publish("fixture.fire", json!({"lost":true})).await;
        for _ in 0..600 {
            bus.publish("noise", json!({})).await;
        }
        let world_task = world.event_listener(world_receiver);
        let task = engine.event_listener(receiver);
        bus.publish("fixture.fire", json!({"lost":false})).await;
        let resumed = tokio::time::timeout(Duration::from_secs(1), async {
            while engine.firings() != 1 || world.event_loss_count().await == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await;
        engine.run_cycle().await;
        let after_cycle = engine.firings();
        let finished = task.is_finished();
        task.abort();
        world_task.abort();
        let _ = task.await;
        let _ = world_task.await;
        assert!(
            resumed.is_ok(),
            "listener failed to resume or replayed a lost trigger"
        );
        assert_eq!(after_cycle, 1, "world rule ran after observation loss");
        assert!(!finished);
        assert!(engine.event_loss_count() > 0);
    }

    #[test]
    fn dotted_world_conditions_match() {
        let conditions = Map::from_iter([("world.network_status".into(), json!("online"))]);
        assert!(conditions_match(
            &conditions,
            &json!({"network_status": "online"})
        ));
        assert!(!conditions_match(
            &conditions,
            &json!({"network_status": "offline"})
        ));
    }
}
