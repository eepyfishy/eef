use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use async_trait::async_trait;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    Interrupted,
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
    Pausing,
    Stopping,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    #[serde(default)]
    pub request_context: Option<eefn::context::RequestContext>,
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
    #[serde(default)]
    pub request_context: Option<eefn::context::RequestContext>,
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
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub assignments: Vec<crate::jobs::Assignment>,
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()
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
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub history: Vec<crate::jobs::JobEvent>,
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
            "generate_text" => vec![(
                "llm.infer",
                "run",
                &[
                    "prompt",
                    "messages",
                    "model_id",
                    "tier",
                    "max_tokens",
                    "temperature",
                ],
            )],
            "analyze_image" => vec![(
                "vlm.analyze",
                "run",
                &[
                    "prompt",
                    "messages",
                    "images",
                    "model_id",
                    "tier",
                    "max_tokens",
                    "temperature",
                ],
            )],
            "launch_application" => {
                vec![("application.control", "launch", &["application", "args"])]
            }
            "read_sensor" => vec![("sensor", "read", &["name"])],
            "write_file" => vec![("filesystem", "write", &["path", "content"])],
            "read_file" => vec![("filesystem", "read", &["path"])],
            "list_directory" => vec![("filesystem", "list", &["path"])],
            "type_text" => vec![("input.control", "type", &["text", "interval"])],
            "press_key" => vec![("input.control", "press", &["key"])],
            "capture_screen" => vec![("screen.capture", "capture", &[])],
            "capture_camera" => vec![(
                "camera.capture",
                "capture",
                &["device", "width", "height", "quality"],
            )],
            "record_audio" => vec![(
                "audio.capture",
                "record",
                &["device", "duration_seconds", "sample_rate", "channels"],
            )],
            "play_audio" => vec![("audio.play", "play", &["data_base64"])],
            "speak_text" => vec![("tts.speak", "speak", &["text", "voice", "rate", "volume"])],
            "transcribe_audio" => vec![(
                "stt.transcribe",
                "transcribe",
                &["audio_base64", "audio_path", "filename"],
            )],
            "execute_process" => vec![(
                "process.exec",
                "run",
                &["program", "args", "cwd", "timeout_seconds"],
            )],
            "http_request" => vec![(
                "http.request",
                "send",
                &["url", "method", "headers", "body", "timeout_seconds"],
            )],
            "wake_computer" => vec![("network.wol", "wake", &["mac", "broadcast", "port"])],
            "observe_room" => vec![
                ("camera.capture", "capture", &["device"]),
                ("vlm.analyze", "analyze", &["prompt"]),
            ],
            _ => bail!("no template '{template}'"),
        };
        let mut tasks: Vec<Task> = Vec::new();
        for (capability, action, keys) in steps {
            let mut params = goal
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
            if !matches!(capability, "llm.infer" | "vlm.analyze") {
                if let Some(resource) = goal.params.get("resource_id") {
                    params["resource_id"] = resource.clone();
                }
            }
            tasks.push(Task {
                request_context: goal.request_context.clone(),
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
                attempts: 0,
                assignments: vec![],
            });
        }
        Ok(Plan {
            plan_id: short_id(),
            goal,
            tasks,
            status: PlanStatus::Pending,
            results: BTreeMap::new(),
            revision: 0,
            updated_at_ms: 0,
            history: vec![],
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
    async fn execute_journaled(
        &self,
        task: &Task,
        _journal: crate::jobs::AttemptJournal,
    ) -> Result<Value> {
        self.execute(task).await
    }
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
        self.run_capability_with_context(capability, action, params, constraints, None)
            .await
    }

    pub async fn run_capability_with_context(
        &self,
        capability: &str,
        action: &str,
        params: Value,
        constraints: Value,
        context: Option<eefn::context::RequestContext>,
    ) -> Result<Value> {
        let task = Task {
            request_context: context,
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
            attempts: 0,
            assignments: vec![],
        };
        self.execute(&task).await
    }
}

#[async_trait]
impl TaskExecutor for Dispatcher {
    async fn execute(&self, task: &Task) -> Result<Value> {
        self.dispatch(task, None).await
    }
    async fn execute_journaled(
        &self,
        task: &Task,
        journal: crate::jobs::AttemptJournal,
    ) -> Result<Value> {
        self.dispatch(task, Some(journal)).await
    }
}
impl Dispatcher {
    async fn dispatch(
        &self,
        task: &Task,
        journal: Option<crate::jobs::AttemptJournal>,
    ) -> Result<Value> {
        let mut constraints = task.constraints.clone();
        if let Some(resource) = task.params.get("resource_id") {
            if !constraints.is_object() {
                constraints = json!({});
            }
            if constraints
                .get("resource_id")
                .is_some_and(|value| value != resource)
            {
                bail!("conflicting resource selection")
            }
            constraints["resource_id"] = resource.clone();
        }
        if matches!(task.capability.as_str(), "llm.infer" | "vlm.analyze") {
            if constraints.get("resource_id").is_some() {
                bail!("choose a model and node for inference, not a physical resource ID")
            }
            let images = task
                .params
                .get("images")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let content = self.llm.chat(
                task.params.get("messages").cloned().unwrap_or_else(|| json!([{"role": "user", "content": task.params.get("prompt").and_then(Value::as_str).unwrap_or("")} ])),
                ChatOptions {
                    journal: journal.clone(),
                    request_context: task.request_context.clone(),
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
            .find_best_with_context(
                &task.capability,
                &task.action,
                &constraints,
                task.request_context.as_ref(),
            )
            .ok_or_else(|| anyhow::anyhow!("no provider available for '{}'", task.capability))?;
        if let Some(journal) = &journal {
            journal.assign(
                &provider.node_id,
                provider.properties["selected_resource_id"].as_str(),
                None,
            )?;
        }
        self.registry.acquire(&provider);
        // Release capacity even when timeout/shutdown drops the invocation future.
        struct Capacity<'a>(
            &'a CapabilityRegistry,
            &'a crate::capability::CapabilityProvider,
        );
        impl Drop for Capacity<'_> {
            fn drop(&mut self) {
                self.0.release(self.1);
            }
        }
        let _capacity = Capacity(&self.registry, &provider);
        let result = async {
            if provider.node_id == "local" {
                self.adapters
                    .invoke(&task.capability, &task.action, task.params.clone())
                    .await
            } else {
                self.registry
                    .validate_params(&task.capability, &task.action, &task.params)?;
                let mut params = task.params.clone();
                if let Some(resource) = provider.properties.get("selected_resource_id") {
                    if !params.is_object() {
                        params = json!({});
                    }
                    params["resource_id"] = resource.clone();
                }
                let response = self
                    .node_server
                    .invoke_remote_with_context(
                        &provider.node_id,
                        &task.capability,
                        &task.action,
                        params,
                        Duration::from_millis(task.timeout_ms),
                        task.request_context.as_ref(),
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
        result
    }
}

#[derive(Clone)]
pub struct TaskEngine {
    bus: EventBus,
    executor: Arc<RwLock<Option<Arc<dyn TaskExecutor>>>>,
    pub store: Arc<crate::jobs::JobStore>,
    active: Arc<std::sync::Mutex<HashSet<String>>>,
    stopping: tokio::sync::watch::Sender<bool>,
}

struct ActiveJob {
    engine: TaskEngine,
    id: String,
}
impl Drop for ActiveJob {
    fn drop(&mut self) {
        if let Err(error) = self.engine.store.interrupt(&self.id) {
            tracing::error!(job_id=%self.id,%error,"Could not checkpoint interrupted job");
        }
        self.engine
            .active
            .lock()
            .expect("active jobs")
            .remove(&self.id);
    }
}

pub fn retry_safe(task: &Task) -> bool {
    matches!(
        (task.capability.as_str(), task.action.as_str()),
        ("system.ping" | "system.info", _)
            | ("filesystem", "read" | "list")
            | ("llm.infer" | "vlm.analyze", _)
    ) || task.constraints.get("retry_safe").and_then(Value::as_bool) == Some(true)
}

impl TaskEngine {
    pub fn new(bus: EventBus) -> Arc<Self> {
        Self::with_store(
            bus,
            crate::jobs::JobStore::in_memory().expect("in-memory jobs"),
        )
    }
    pub fn with_store(bus: EventBus, store: Arc<crate::jobs::JobStore>) -> Arc<Self> {
        Arc::new(Self {
            bus,
            store,
            executor: Arc::new(RwLock::new(None)),
            active: Arc::new(std::sync::Mutex::new(HashSet::new())),
            stopping: tokio::sync::watch::channel(false).0,
        })
    }
    pub fn set_executor(&self, executor: Arc<dyn TaskExecutor>) {
        *self.executor.write().expect("executor lock") = Some(executor);
    }
    pub fn has_executor(&self) -> bool {
        self.executor.read().expect("executor lock").is_some()
    }
    pub fn list_plans(&self) -> Result<Vec<Plan>> {
        self.store.list()
    }
    fn claim(&self, id: &str) -> Result<ActiveJob> {
        let mut active = self.active.lock().expect("active jobs");
        if *self.stopping.borrow() {
            bail!("Coordinator is stopping; no new job execution is accepted")
        }
        if !active.insert(id.into()) {
            bail!("Job is already running")
        }
        Ok(ActiveJob {
            engine: self.clone(),
            id: id.into(),
        })
    }
    pub fn pause(&self, id: &str) -> Result<Plan> {
        self.store
            .change(id, "Pause requested; current batch may finish", |plan| {
                plan.status = match plan.status {
                    PlanStatus::Running => PlanStatus::Pausing,
                    PlanStatus::Pending | PlanStatus::Paused => PlanStatus::Paused,
                    _ => bail!("This job cannot be paused in its current state"),
                };
                Ok(())
            })
    }
    pub fn cancel(&self, id: &str) -> Result<Plan> {
        let active = self.active.lock().expect("active jobs").contains(id);
        self.store.change(
            id,
            "Stop requested; completed actions are not undone",
            |plan| {
                if matches!(
                    plan.status,
                    PlanStatus::Completed | PlanStatus::Failed | PlanStatus::Cancelled
                ) {
                    bail!("Job is already finished")
                }
                plan.status = if active {
                    PlanStatus::Stopping
                } else {
                    PlanStatus::Cancelled
                };
                if !active {
                    for task in &mut plan.tasks {
                        if task.status == TaskStatus::Pending {
                            task.status = TaskStatus::Cancelled;
                        }
                    }
                }
                Ok(())
            },
        )
    }
    pub fn resume_job(self: &Arc<Self>, id: &str) -> Result<()> {
        if !self.has_executor() {
            bail!("TaskEngine has no executor")
        }
        if !matches!(
            self.store.get(id)?.status,
            PlanStatus::Paused | PlanStatus::Interrupted
        ) {
            bail!("Only paused or interrupted jobs can resume")
        }
        let guard = self.claim(id)?;
        self.store.change(id,"Owner requested resume",|plan|{
            if !matches!(plan.status,PlanStatus::Paused|PlanStatus::Interrupted) {bail!("Only paused or interrupted jobs can resume")}
            if plan.tasks.iter().any(|t|t.status==TaskStatus::Interrupted&&!retry_safe(t)) {
                bail!("An interrupted action may already have happened. This job cannot safely resume; review it and create new work explicitly.")
            }
            for task in &mut plan.tasks {if task.status==TaskStatus::Interrupted {task.status=TaskStatus::Pending;task.result=None;plan.results.remove(&task.task_id);}}
            plan.status=PlanStatus::Pending;Ok(())
        })?;
        let engine = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            if let Err(error) = engine.drive(&id, guard).await {
                tracing::error!(job_id=%id,%error,"Resumed job needs review");
            }
        });
        Ok(())
    }
    pub fn enqueue(self: &Arc<Self>, plan: Plan) -> Result<String> {
        if !self.has_executor() {
            bail!("TaskEngine has no executor")
        }
        let id = plan.plan_id.clone();
        self.store.insert(plan)?;
        let guard = self.claim(&id)?;
        let engine = self.clone();
        let started = id.clone();
        tokio::spawn(async move {
            if let Err(error) = engine.drive(&started, guard).await {
                tracing::error!(job_id=%started,%error,"Job needs review");
            }
        });
        Ok(id)
    }
    pub async fn run_plan(&self, plan: Plan) -> Result<Plan> {
        let id = plan.plan_id.clone();
        self.store.insert(plan)?;
        let guard = self.claim(&id)?;
        self.drive(&id, guard).await
    }
    pub async fn shutdown(&self) {
        self.stopping.send_replace(true);
        while !self.active.lock().expect("active jobs").is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn drive(&self, id: &str, _guard: ActiveJob) -> Result<Plan> {
        let mut stopping = self.stopping.subscribe();
        if *stopping.borrow() {
            bail!("Coordinator shutdown interrupted this job")
        }
        tokio::select! {
            biased;
            _=stopping.changed()=>bail!("Coordinator shutdown interrupted this job; remote effects may continue"),
            result=self.drive_loop(id)=>result,
        }
    }
    async fn drive_loop(&self, id: &str) -> Result<Plan> {
        let executor = self
            .executor
            .read()
            .expect("executor lock")
            .clone()
            .context("TaskEngine has no executor")?;
        self.store.change(id, "Job runner started", |plan| {
            if plan.status == PlanStatus::Pending {
                plan.status = PlanStatus::Running;
            }
            Ok(())
        })?;
        loop {
            let plan = self.store.get(id)?;
            match plan.status {
                PlanStatus::Stopping => {
                    return self.store.change(
                        id,
                        "Job stopped; remote effects are not undone",
                        |plan| {
                            plan.status = PlanStatus::Cancelled;
                            for task in &mut plan.tasks {
                                if task.status == TaskStatus::Pending {
                                    task.status = TaskStatus::Cancelled;
                                }
                            }
                            Ok(())
                        },
                    );
                }
                PlanStatus::Pausing => {
                    return self.store.change(id, "Job paused", |p| {
                        p.status = PlanStatus::Paused;
                        Ok(())
                    });
                }
                PlanStatus::Paused | PlanStatus::Cancelled => return Ok(plan),
                PlanStatus::Running => {}
                _ => bail!("Job runner encountered an unexpected state"),
            }
            let ready = plan.ready_indices();
            if ready.is_empty() {
                return self.store.change(id, "Job run finished", |plan| {
                    if matches!(plan.status, PlanStatus::Pausing | PlanStatus::Stopping) {
                        plan.status = if plan.status == PlanStatus::Stopping {
                            PlanStatus::Cancelled
                        } else {
                            PlanStatus::Paused
                        };
                        if plan.status == PlanStatus::Cancelled {
                            for task in &mut plan.tasks {
                                if task.status == TaskStatus::Pending {
                                    task.status = TaskStatus::Cancelled;
                                }
                            }
                        }
                        return Ok(());
                    }
                    // Uncertain predecessors may be explicitly resumed later.
                    // Their dependents must remain pending, not permanently skipped.
                    if plan
                        .tasks
                        .iter()
                        .any(|t| t.status == TaskStatus::Interrupted)
                    {
                        plan.status = PlanStatus::Interrupted;
                        return Ok(());
                    }
                    for task in &mut plan.tasks {
                        if task.status == TaskStatus::Pending {
                            task.status = TaskStatus::Skipped;
                        }
                    }
                    plan.status = if plan
                        .tasks
                        .iter()
                        .any(|t| t.status == TaskStatus::Interrupted)
                    {
                        PlanStatus::Interrupted
                    } else if plan
                        .tasks
                        .iter()
                        .any(|t| matches!(t.status, TaskStatus::Failed | TaskStatus::Skipped))
                    {
                        PlanStatus::Failed
                    } else {
                        PlanStatus::Completed
                    };
                    Ok(())
                });
            }
            let claimed = self.store.change(
                id,
                "Ready task batch checkpointed before dispatch",
                |plan| {
                    if plan.status == PlanStatus::Running {
                        for index in &ready {
                            plan.tasks[*index].status = TaskStatus::Running;
                        }
                    }
                    Ok(())
                },
            )?;
            if claimed.status != PlanStatus::Running {
                continue;
            }
            let outcomes =
                join_all(ready.iter().map(|index| {
                    self.run_task(id, claimed.tasks[*index].clone(), executor.clone())
                }))
                .await;
            for outcome in outcomes {
                outcome?;
            }
        }
    }
    async fn run_task(
        &self,
        job_id: &str,
        mut task: Task,
        executor: Arc<dyn TaskExecutor>,
    ) -> Result<TaskResult> {
        let retries = if retry_safe(&task) {
            task.retry_count
        } else {
            0
        };
        let started = Instant::now();
        for retry in 0..=retries {
            let mut authorized = true;
            let checkpoint = self.store.change(job_id, "Task attempt started", |plan| {
                let current = plan
                    .tasks
                    .iter_mut()
                    .find(|t| t.task_id == task.task_id)
                    .context("Unknown task")?;
                // Close the race between the retry-delay check and this write.
                // A retry is a new dispatch, not another already-sent action.
                if retry > 0 && plan.status != PlanStatus::Running {
                    authorized = false;
                    current.status =
                        if matches!(plan.status, PlanStatus::Pausing | PlanStatus::Paused) {
                            TaskStatus::Pending
                        } else {
                            TaskStatus::Cancelled
                        };
                    return Ok(());
                }
                current.attempts = current
                    .attempts
                    .checked_add(1)
                    .context("Attempt counter exhausted")?;
                Ok(())
            })?;
            task = checkpoint
                .tasks
                .into_iter()
                .find(|t| t.task_id == task.task_id)
                .context("Unknown task")?;
            if !authorized {
                return task
                    .result
                    .clone()
                    .context("Missing preceding attempt result");
            }
            let journal = crate::jobs::AttemptJournal {
                store: self.store.clone(),
                job_id: job_id.into(),
                task_id: task.task_id.clone(),
                attempt: task.attempts,
            };
            self.bus.publish("task.started",json!({"job_id":job_id,"task_id":task.task_id,"attempt":task.attempts,"capability":task.capability,"action":task.action,"request_context":task.request_context})).await;
            let result = timeout(
                Duration::from_millis(task.timeout_ms),
                executor.execute_journaled(&task, journal),
            )
            .await;
            let (data, error, error_type) = match result {
                Ok(Ok(data)) => (data, String::new(), String::new()),
                Ok(Err(error)) => (Value::Null, error.to_string(), "ExecutionError".into()),
                Err(_) => (
                    Value::Null,
                    "Task timed out; remote work may still be running".into(),
                    "TimeoutError".into(),
                ),
            };
            let result = TaskResult {
                task_id: task.task_id.clone(),
                success: error.is_empty(),
                data,
                error,
                error_type,
                duration_ms: started.elapsed().as_millis() as u64,
                attempts: task.attempts,
            };
            let retrying = !result.success
                && retry < retries
                && self.store.get(job_id)?.status == PlanStatus::Running;
            self.store.change(
                job_id,
                if retrying {
                    "Attempt failed; explicit safe retry pending"
                } else {
                    "Task result checkpointed"
                },
                |plan| {
                    let current = plan
                        .tasks
                        .iter_mut()
                        .find(|t| t.task_id == task.task_id)
                        .context("Unknown task")?;
                    current.result = Some(result.clone());
                    if !retrying {
                        current.status = if result.success {
                            TaskStatus::Completed
                        } else if result.error_type == "TimeoutError" {
                            TaskStatus::Interrupted
                        } else {
                            TaskStatus::Failed
                        };
                        plan.results.insert(result.task_id.clone(), result.clone());
                    }
                    Ok(())
                },
            )?;
            if !retrying {
                self.bus.publish("task.completed",json!({"job_id":job_id,"task_id":task.task_id,"success":result.success,"error":result.error,"error_type":result.error_type,"request_context":task.request_context})).await;
                return Ok(result);
            }
            sleep(Duration::from_millis(task.retry_delay_ms)).await;
            if self.store.get(job_id)?.status != PlanStatus::Running {
                self.store
                    .change(job_id, "Retry stopped before another dispatch", |plan| {
                        let current = plan
                            .tasks
                            .iter_mut()
                            .find(|t| t.task_id == task.task_id)
                            .context("Unknown task")?;
                        current.status =
                            if matches!(plan.status, PlanStatus::Pausing | PlanStatus::Paused) {
                                TaskStatus::Pending
                            } else if result.error_type == "TimeoutError" {
                                TaskStatus::Interrupted
                            } else {
                                TaskStatus::Failed
                            };
                        if current.status != TaskStatus::Pending {
                            plan.results.insert(result.task_id.clone(), result.clone());
                        }
                        Ok(())
                    })?;
                return Ok(result);
            }
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

    #[test]
    fn planner_retains_origin_and_applies_resource_only_to_physical_step() {
        let origin =
            eefn::context::RequestContext::new("req".into(), "desk".into(), vec!["Office".into()])
                .unwrap();
        let plan = TaskPlanner
            .plan(Goal {
                request_context: Some(origin.clone()),
                description: "observe".into(),
                template: "observe_room".into(),
                params: json!({"resource_id":"camera-node::cam","prompt":"Describe it"}),
                constraints: json!({}),
            })
            .unwrap();
        assert!(
            plan.tasks
                .iter()
                .all(|task| task.request_context.as_ref() == Some(&origin))
        );
        assert_eq!(plan.tasks[0].params["resource_id"], "camera-node::cam");
        assert!(plan.tasks[1].params.get("resource_id").is_none());
        let restored: Plan = serde_json::from_value(json!(plan)).unwrap();
        assert_eq!(restored.goal.request_context, Some(origin));
    }

    struct FailsAfterSideEffect(std::sync::atomic::AtomicU32);
    #[async_trait]
    impl TaskExecutor for FailsAfterSideEffect {
        async fn execute(&self, _: &Task) -> Result<Value> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            bail!("connection lost after side effect")
        }
    }
    #[tokio::test]
    async fn mutating_tasks_are_not_automatically_replayed() {
        let engine = TaskEngine::new(EventBus::new(20));
        let executor = Arc::new(FailsAfterSideEffect(std::sync::atomic::AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let goal: Goal=serde_json::from_value(json!({"description":"launch app","template":"launch_application","params":{"name":"fixture"}})).unwrap();
        let plan = engine
            .run_plan(TaskPlanner.plan(goal).unwrap())
            .await
            .unwrap();
        assert_eq!(plan.status, PlanStatus::Failed);
        assert_eq!(executor.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(plan.results.values().next().unwrap().attempts, 1);
    }

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
                request_context: None,
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
