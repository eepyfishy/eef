use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::assistant::ResponseEngine;
use crate::event::EventBus;
use crate::memory::{IdentityMemory, MutableMemory};
use crate::model::{ChatOptions, LlmService};
use crate::task::{Goal, PlanStatus, TaskEngine, TaskPlanner};
use crate::world::WorldState;

pub struct Brain {
    bus: EventBus,
    world: Arc<WorldState>,
    identity: IdentityMemory,
    memory: Arc<MutableMemory>,
    engine: Arc<TaskEngine>,
    llm: Arc<LlmService>,
    assistant: Arc<ResponseEngine>,
    tick: Duration,
    running: AtomicBool,
    loop_task: Mutex<Option<JoinHandle<()>>>,
    intents_handled: AtomicU64,
}

enum Intent {
    Chat,
    Remember(String),
    Goal(Goal),
}

impl Brain {
    pub fn new(
        bus: EventBus,
        world: Arc<WorldState>,
        identity: IdentityMemory,
        memory: Arc<MutableMemory>,
        engine: Arc<TaskEngine>,
        llm: Arc<LlmService>,
        assistant: Arc<ResponseEngine>,
        tick: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            bus,
            world,
            identity,
            memory,
            engine,
            llm,
            assistant,
            tick,
            running: AtomicBool::new(false),
            loop_task: Mutex::new(None),
            intents_handled: AtomicU64::new(0),
        })
    }

    pub async fn start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let brain = self.clone();
        *self.loop_task.lock().await = Some(tokio::spawn(async move { brain.run_loop().await }));
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(task) = self.loop_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
    }

    pub fn alive(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    pub fn intents_handled(&self) -> u64 {
        self.intents_handled.load(Ordering::Relaxed)
    }

    async fn run_loop(&self) {
        info!(tick_seconds = self.tick.as_secs_f64(), "brain loop started");
        let mut last_reflection = Instant::now() - Duration::from_secs(60);
        while self.running.load(Ordering::Relaxed) {
            self.assistant.run_cycle().await;
            if last_reflection.elapsed() >= Duration::from_secs(60) {
                let summary = self.world.summary().await;
                let active = summary
                    .get("active_tasks")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                let nodes = summary
                    .get("connected_nodes")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                self.bus
                    .publish(
                        "brain.reflection",
                        json!({"active_tasks": active, "connected_nodes": nodes}),
                    )
                    .await;
                last_reflection = Instant::now();
            }
            tokio::time::sleep(self.tick).await;
        }
    }

    pub async fn handle_message(&self, text: &str) -> Result<String> {
        self.handle_message_with_context(text, None).await
    }

    pub async fn handle_message_with_context(
        &self,
        text: &str,
        context: Option<eefn::context::RequestContext>,
    ) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            bail!("message must not be empty")
        }
        let sequence = self.intents_handled.fetch_add(1, Ordering::Relaxed) + 1;
        self.memory
            .add_conversation_with_context("user", text, context.as_ref())?;
        self.bus
            .publish(
                "assistant.user_message",
                json!({"content": text,"request_context":context}),
            )
            .await;
        self.assistant
            .on_keyword_with_context(text, context.as_ref())
            .await;
        let reply = match understand(text) {
            Intent::Remember(fact) => {
                self.memory.add_fact(
                    &format!("note_{sequence}"),
                    &Value::String(fact.clone()),
                    1.0,
                    "user",
                )?;
                format!("Remembered: {fact}")
            }
            Intent::Goal(mut goal) => {
                goal.request_context = context.clone();
                self.run_goal(goal).await
            }
            Intent::Chat => self.chat_reply(text, context.clone()).await,
        };
        self.memory
            .add_conversation_with_context("assistant", &reply, context.as_ref())?;
        self.bus
            .publish(
                "assistant.response",
                json!({"content": reply,"request_context":context}),
            )
            .await;
        Ok(reply)
    }

    async fn run_goal(&self, goal: Goal) -> String {
        let description = goal.description.clone();
        let template = goal.template.clone();
        let plan = match TaskPlanner.plan(goal) {
            Ok(plan) => plan,
            Err(error) => return format!("I couldn't plan that: {error}"),
        };
        self.bus
            .publish(
                "goal.created",
                json!({"description": description, "template": template, "plan_id": plan.plan_id,"request_context":plan.goal.request_context}),
            )
            .await;
        match self.engine.run_plan(plan).await {
            Ok(plan) if plan.status == PlanStatus::Completed => {
                let _ = self
                    .memory
                    .add_note(&format!("goal completed: {description}"));
                "Done.".into()
            }
            Ok(plan) => {
                let errors = plan
                    .results
                    .values()
                    .filter(|result| !result.success)
                    .map(|result| result.error.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                format!(
                    "Failed. {}",
                    if errors.is_empty() {
                        "one or more tasks failed"
                    } else {
                        &errors
                    }
                )
            }
            Err(error) => format!("Failed. {error}"),
        }
    }

    async fn chat_reply(
        &self,
        text: &str,
        context: Option<eefn::context::RequestContext>,
    ) -> String {
        let system_prompt = format!(
            "You are {}, a continuous personal AI agent. {} Keep replies short and useful.",
            self.identity.name(),
            self.identity
                .behavior_rules()
                .into_iter()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        );
        match tokio::time::timeout(
            Duration::from_secs(30),
            self.llm.chat(
                json!([{"role": "user", "content": text}]),
                ChatOptions {
                    request_context: context,
                    system_prompt,
                    ..ChatOptions::default()
                },
            ),
        )
        .await
        {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => {
                warn!(%error, "chat model failed");
                "I'm here. The configured model is unavailable right now.".into()
            }
            Err(_) => "I'm still warming up — ask me again in a moment.".into(),
        }
    }
}

fn understand(text: &str) -> Intent {
    let lowered = text.to_lowercase();
    if let Some(index) = lowered.find("remember") {
        let tail = text[index + "remember".len()..]
            .trim()
            .strip_prefix("that ")
            .unwrap_or(text[index + "remember".len()..].trim());
        return Intent::Remember(tail.to_owned());
    }
    for prefix in ["open ", "launch "] {
        if lowered.starts_with(prefix) {
            let rest = text[prefix.len()..].trim();
            let mut parts = rest.split_whitespace();
            let name = parts.next().unwrap_or("");
            if !name.is_empty() {
                return Intent::Goal(Goal {
                    request_context: None,
                    description: text.into(),
                    template: "launch_application".into(),
                    params: json!({"name": name, "args": parts.collect::<Vec<_>>()}),
                    constraints: json!({}),
                });
            }
        }
    }
    if let Some(path) = lowered
        .strip_prefix("read file ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Intent::Goal(Goal {
            request_context: None,
            description: text.into(),
            template: "read_file".into(),
            params: json!({"path": path}),
            constraints: json!({}),
        });
    }
    if let Some(path) = lowered
        .strip_prefix("list directory ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Intent::Goal(Goal {
            request_context: None,
            description: text.into(),
            template: "list_directory".into(),
            params: json!({"path": path}),
            constraints: json!({}),
        });
    }
    if lowered.contains("screenshot") || lowered == "capture screen" {
        return Intent::Goal(Goal {
            request_context: None,
            description: text.into(),
            template: "capture_screen".into(),
            params: json!({}),
            constraints: json!({}),
        });
    }
    Intent::Chat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_target_is_not_hardcoded() {
        let Intent::Goal(goal) = understand("open notepad") else {
            panic!("expected goal")
        };
        assert_eq!(goal.params["name"], "notepad");
    }
}
