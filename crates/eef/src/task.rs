use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::adapters::AdapterManager;
use crate::capability::CapabilityRegistry;
use crate::event::EventBus;
use crate::model::{ChatOptions, LlmService, Modality};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub constraints: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    #[serde(default = "short_id")]
    pub task_id: String,
    pub capability: String,
    #[serde(default = "run")]
    pub action: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default = "task_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "retries")]
    pub retry_count: u32,
    #[serde(default = "retry_delay")]
    pub retry_delay_ms: u64,
    #[serde(default)]
    pub constraints: Value,
    #[serde(default = "pending_task")]
    pub status: TaskStatus,
    #[serde(default)]
    pub result: Option<TaskResult>,
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].into()
}
fn run() -> String {
    "run".into()
}
const fn task_timeout() -> u64 {
    30_000
}
const fn retries() -> u32 {
    2
}
const fn retry_delay() -> u64 {
    500
}
const fn pending_task() -> TaskStatus {
    TaskStatus::Pending
}
const fn pending_plan() -> PlanStatus {
    PlanStatus::Pending
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub error_type: String,
    pub duration_ms: u64,
    pub attempts: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default = "short_id")]
    pub plan_id: String,
    pub goal: Goal,
    pub tasks: Vec<Task>,
    #[serde(default = "pending_plan")]
    pub status: PlanStatus,
    #[serde(default)]
    pub results: BTreeMap<String, TaskResult>,
}

impl Plan {
    fn ready_indices(&self) -> Vec<usize> {
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| {
                task.status == TaskStatus::Pending
                    && task.dependencies.iter().all(|dependency| {
                        self.results
                            .get(dependency)
                            .is_some_and(|result| result.success)
                    })
            })
            .map(|(index, _)| index)
            .collect()
    }
    fn has_pending(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.status == TaskStatus::Pending)
    }
}

pub struct TaskPlanner;

impl TaskPlanner {
    pub fn plan(&self, goal: Goal) -> Result<Plan> {
        let template = if goal.template.is_empty() {
            infer_template(&goal.description)
        } else {
            goal.template.clone()
        };
        let steps: Vec<(&str, &str, &[&str])> = match template.as_str() {
            "launch_application" => vec![("launch_application", "launch", &["name", "args"])],
            "read_sensor" => vec![("sensor", "read", &["name"])],
            "write_file" => vec![("filesystem", "write", &["path", "content"])],
            "read_file" => vec![("filesystem", "read", &["path"])],
            "list_directory" => vec![("filesystem", "list", &["path"])],
            "type_text" => vec![("keyboard", "type", &["text"])],
            "press_key" => vec![("keyboard", "press", &["key"])],
            "capture_screen" => vec![("screen_capture", "capture", &[])],
            "observe_room" => vec![
                ("camera", "capture", &["node"]),
                ("vlm.analyze", "analyze", &["prompt"]),
            ],
            _ => bail!("no template '{template}'"),
        };
        let mut tasks: Vec<Task> = Vec::new();
        for (capability, action, keys) in steps {
            let params = goal
                .params
                .as_object()
                .map(|values| {
                    Value::Object(
                        keys.iter()
                            .filter_map(|key| {
                                values.get(*key).map(|value| ((*key).into(), value.clone()))
                            })
                            .collect(),
                    )
                })
                .unwrap_or_else(|| json!({}));
            tasks.push(Task {
                task_id: short_id(),
                capability: capability.into(),
                action: action.into(),
                params,
                dependencies: tasks.iter().map(|task| task.task_id.clone()).collect(),
                timeout_ms: task_timeout(),
                retry_count: retries(),
                retry_delay_ms: retry_delay(),
                constraints: goal.constraints.clone(),
                status: TaskStatus::Pending,
                result: None,
            });
        }
        Ok(Plan {
            plan_id: short_id(),
            goal,
            tasks,
            status: PlanStatus::Pending,
            results: BTreeMap::new(),
        })
    }
}

fn infer_template(description: &str) -> String {
    let text = description.to_lowercase();
    if regex::Regex::new(r"\b(read|show|cat)\b.*\bfile\b|\bfile\b.*\b(read|show|cat)\b")
        .unwrap()
        .is_match(&text)
    {
        "read_file"
    } else if regex::Regex::new(r"\b(write|create|save)\b.*\bfile\b")
        .unwrap()
        .is_match(&text)
    {
        "write_file"
    } else if text.contains("screenshot") || text.contains("screen") {
        "capture_screen"
    } else if text.contains("look") || text.contains("observe") || text.contains("watch") {
        "observe_room"
    } else if text.contains("temp") || text.contains("sensor") {
        "read_sensor"
    } else if text.contains("list") || text.contains(" ls ") {
        "list_directory"
    } else if text.contains("type") {
        "type_text"
    } else {
        "launch_application"
    }
    .into()
}

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(&self, task: &Task) -> Result<Value>;
}

pub struct Dispatcher {
    registry: CapabilityRegistry,
    adapters: AdapterManager,
    llm: Arc<LlmService>,
    node_server: eefn::NodeServer,
}

impl Dispatcher {
    pub fn new(
        registry: CapabilityRegistry,
        adapters: AdapterManager,
        llm: Arc<LlmService>,
        node_server: eefn::NodeServer,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            adapters,
            llm,
            node_server,
        })
    }

    pub async fn run_capability(
        &self,
        capability: &str,
        action: &str,
        params: Value,
        constraints: Value,
    ) -> Result<Value> {
        let task = Task {
            task_id: short_id(),
            capability: capability.into(),
            action: action.into(),
            params,
            dependencies: vec![],
            timeout_ms: task_timeout(),
            retry_count: 0,
            retry_delay_ms: 0,
            constraints,
            status: TaskStatus::Pending,
            result: None,
        };
        self.execute(&task).await
    }
}

#[async_trait]
impl TaskExecutor for Dispatcher {
    async fn execute(&self, task: &Task) -> Result<Value> {
        if matches!(task.capability.as_str(), "llm.infer" | "vlm.analyze") {
            let images = task
                .params
                .get("images")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let content = self.llm.chat(
                task.params.get("messages").cloned().unwrap_or_else(|| json!([{"role": "user", "content": task.params.get("prompt").and_then(Value::as_str).unwrap_or("")} ])),
                ChatOptions {
                    tier: task.params.get("tier").and_then(Value::as_str).unwrap_or("fast").into(),
                    model_id: task.params.get("model_id").or_else(|| task.params.get("model")).and_then(Value::as_str).map(str::to_owned),
                    modality: Some(if task.capability == "vlm.analyze" { Modality::Vlm } else { Modality::Text }),
                    constraints: task.constraints.clone(),
                    temperature: task.params.get("temperature").and_then(Value::as_f64).unwrap_or(0.7),
                    max_tokens: task.params.get("max_tokens").and_then(Value::as_u64).unwrap_or(1024),
                    images,
                    ..ChatOptions::default()
                },
            ).await?;
            return Ok(json!({"content": content}));
        }
        let provider = self
            .registry
            .find_best(&task.capability, &task.action, &task.constraints)
            .ok_or_else(|| anyhow::anyhow!("no provider available for '{}'", task.capability))?;
        self.registry.acquire(&provider);
        let result = async {
            if provider.node_id == "local" {
                self.adapters
                    .invoke(&task.capability, &task.action, task.params.clone())
                    .await
            } else {
                self.registry
                    .validate_params(&task.capability, &task.action, &task.params)?;
                let response = self
                    .node_server
                    .invoke_remote(
                        &provider.node_id,
                        &task.capability,
                        &task.action,
                        task.params.clone(),
                        Duration::from_millis(task.timeout_ms),
                    )
                    .await?;
                if response.get("success").and_then(Value::as_bool) == Some(true) {
                    Ok(response.get("data").cloned().unwrap_or(Value::Null))
                } else {
                    bail!(
                        "{}",
                        response
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("remote task failed")
                    )
                }
            }
        }
        .await;
        self.registry.release(&provider);
        result
    }
}

#[derive(Clone)]
pub struct TaskEngine {
    bus: EventBus,
    executor: Arc<RwLock<Option<Arc<dyn TaskExecutor>>>>,
    plans: Arc<RwLock<BTreeMap<String, Plan>>>,
    cancelled: Arc<RwLock<HashSet<String>>>,
    paused: Arc<RwLock<HashSet<String>>>,
    resume: Arc<Notify>,
}

impl TaskEngine {
    pub fn new(bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            bus,
            executor: Arc::new(RwLock::new(None)),
            plans: Arc::new(RwLock::new(BTreeMap::new())),
            cancelled: Arc::new(RwLock::new(HashSet::new())),
            paused: Arc::new(RwLock::new(HashSet::new())),
            resume: Arc::new(Notify::new()),
        })
    }
    pub fn set_executor(&self, executor: Arc<dyn TaskExecutor>) {
        *self.executor.write().expect("executor lock") = Some(executor);
    }
    pub fn has_executor(&self) -> bool {
        self.executor.read().expect("executor lock").is_some()
    }
    pub fn list_plans(&self) -> Vec<Plan> {
        self.plans
            .read()
            .expect("plans lock")
            .values()
            .cloned()
            .collect()
    }
    pub fn cancel(&self, id: &str) {
        self.cancelled
            .write()
            .expect("cancel lock")
            .insert(id.into());
        self.resume.notify_waiters();
    }
    pub fn pause(&self, id: &str) {
        self.paused.write().expect("pause lock").insert(id.into());
    }
    pub fn resume(&self, id: &str) {
        self.paused.write().expect("pause lock").remove(id);
        self.resume.notify_waiters();
    }

    pub async fn run_plan(&self, mut plan: Plan) -> Result<Plan> {
        let executor = self
            .executor
            .read()
            .expect("executor lock")
            .clone()
            .context("TaskEngine has no executor")?;
        plan.status = PlanStatus::Running;
        self.plans
            .write()
            .expect("plans lock")
            .insert(plan.plan_id.clone(), plan.clone());
        loop {
            if self
                .cancelled
                .read()
                .expect("cancel lock")
                .contains(&plan.plan_id)
            {
                plan.status = PlanStatus::Cancelled;
                for task in &mut plan.tasks {
                    if matches!(task.status, TaskStatus::Pending | TaskStatus::Running) {
                        task.status = TaskStatus::Cancelled;
                    }
                }
                break;
            }
            while self
                .paused
                .read()
                .expect("pause lock")
                .contains(&plan.plan_id)
            {
                plan.status = PlanStatus::Paused;
                self.plans
                    .write()
                    .expect("plans lock")
                    .insert(plan.plan_id.clone(), plan.clone());
                self.resume.notified().await;
            }
            plan.status = PlanStatus::Running;
            let ready = plan.ready_indices();
            if ready.is_empty() {
                if plan.has_pending() {
                    for task in &mut plan.tasks {
                        if task.status == TaskStatus::Pending {
                            task.status = TaskStatus::Skipped;
                        }
                    }
                }
                break;
            }
            let jobs = ready
                .iter()
                .map(|index| self.run_task(plan.tasks[*index].clone(), executor.clone()));
            let results = join_all(jobs).await;
            for (index, result) in ready.into_iter().zip(results) {
                plan.tasks[index].status = if result.success {
                    TaskStatus::Completed
                } else {
                    TaskStatus::Failed
                };
                plan.tasks[index].result = Some(result.clone());
                plan.results.insert(result.task_id.clone(), result);
            }
            self.plans
                .write()
                .expect("plans lock")
                .insert(plan.plan_id.clone(), plan.clone());
        }
        if plan.status != PlanStatus::Cancelled {
            plan.status = if plan
                .tasks
                .iter()
                .any(|task| task.status == TaskStatus::Failed)
            {
                PlanStatus::Failed
            } else {
                PlanStatus::Completed
            };
        }
        self.plans
            .write()
            .expect("plans lock")
            .insert(plan.plan_id.clone(), plan.clone());
        Ok(plan)
    }

    async fn run_task(&self, mut task: Task, executor: Arc<dyn TaskExecutor>) -> TaskResult {
        self.bus.publish("task.started", json!({"task_id": task.task_id, "capability": task.capability, "action": task.action})).await;
        task.status = TaskStatus::Running;
        let started = Instant::now();
        for attempt in 1..=task.retry_count + 1 {
            let failure = match timeout(
                Duration::from_millis(task.timeout_ms),
                executor.execute(&task),
            )
            .await
            {
                Ok(Ok(data)) => {
                    let result = TaskResult {
                        task_id: task.task_id.clone(),
                        success: true,
                        data,
                        error: String::new(),
                        error_type: String::new(),
                        duration_ms: started.elapsed().as_millis() as u64,
                        attempts: attempt,
                    };
                    self.bus.publish("task.completed", json!({"task_id": task.task_id, "success": true, "duration_ms": result.duration_ms})).await;
                    return result;
                }
                Ok(Err(error)) => (error.to_string(), "ExecutionError"),
                Err(_) => ("task timed out".into(), "TimeoutError"),
            };
            if attempt > task.retry_count {
                let result = TaskResult {
                    task_id: task.task_id.clone(),
                    success: false,
                    data: Value::Null,
                    error: failure.0,
                    error_type: failure.1.into(),
                    duration_ms: started.elapsed().as_millis() as u64,
                    attempts: attempt,
                };
                self.bus.publish("task.completed", json!({"task_id": task.task_id, "success": false, "error": result.error, "error_type": result.error_type})).await;
                return result;
            }
            sleep(Duration::from_millis(task.retry_delay_ms)).await;
        }
        unreachable!()
    }
}

trait ContextOption<T> {
    fn context(self, message: &str) -> Result<T>;
}
impl<T> ContextOption<T> for Option<T> {
    fn context(self, message: &str) -> Result<T> {
        self.ok_or_else(|| anyhow::anyhow!(message.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    #[async_trait]
    impl TaskExecutor for Echo {
        async fn execute(&self, task: &Task) -> Result<Value> {
            Ok(task.params.clone())
        }
    }

    #[tokio::test]
    async fn plans_really_execute() {
        let engine = TaskEngine::new(EventBus::new(20));
        engine.set_executor(Arc::new(Echo));
        let plan = TaskPlanner
            .plan(Goal {
                description: "write file".into(),
                template: "write_file".into(),
                params: json!({"path": "a", "content": "b"}),
                constraints: json!({}),
            })
            .unwrap();
        let plan = engine.run_plan(plan).await.unwrap();
        assert_eq!(plan.status, PlanStatus::Completed);
        assert_eq!(plan.results.len(), 1);
    }
}
