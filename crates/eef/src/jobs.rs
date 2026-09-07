//! Single-coordinator durable job journal. No election or cross-coordinator lease.
use crate::task::{Plan, PlanStatus, TaskStatus};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

// Logical quota headroom, not a promise that the physical disk cannot fill.
const RECOVERY_RESERVE: usize = 4096;
fn reserve(plan: &Plan) -> usize {
    match plan.status {
        PlanStatus::Pending | PlanStatus::Running => RECOVERY_RESERVE,
        PlanStatus::Pausing | PlanStatus::Stopping => 3072,
        PlanStatus::Paused | PlanStatus::Interrupted => 1024,
        _ => 0,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub attempt: u32,
    pub node_id: String,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    pub assigned_at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobEvent {
    pub revision: u64,
    pub at_ms: u64,
    pub message: String,
}

#[derive(Debug)]
pub struct JobStore {
    connection: Mutex<Connection>,
    _file_lock: Option<File>,
    max_count: usize,
    max_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct AttemptJournal {
    pub store: Arc<JobStore>,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
}
impl AttemptJournal {
    pub fn assign(&self, node: &str, resource: Option<&str>, model: Option<&str>) -> Result<()> {
        self.store
            .change(&self.job_id, "Executor assigned", |plan| {
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|t| t.task_id == self.task_id)
                    .context("Unknown task")?;
                if task.status != TaskStatus::Running || task.attempts != self.attempt {
                    bail!("Stale execution attempt")
                }
                if task.assignments.len() >= 128 {
                    bail!("Task assignment history limit reached")
                }
                task.assignments.push(Assignment {
                    attempt: self.attempt,
                    node_id: node.into(),
                    resource_id: resource.map(str::to_owned),
                    model_id: model.map(str::to_owned),
                    assigned_at_ms: eefn::protocol::now_ms(),
                });
                Ok(())
            })?;
        Ok(())
    }
}

impl JobStore {
    pub fn open(path: &Path, max_count: usize, max_bytes: usize) -> Result<Arc<Self>> {
        let path = path
            .canonicalize()
            .context("Open job journal beside the existing database")?;
        let lock_path = path.with_file_name(format!(
            "{}.jobs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("Another coordinator already owns this job database")?;
        Self::initialize(Connection::open(path)?, Some(lock), max_count, max_bytes)
    }
    pub fn in_memory() -> Result<Arc<Self>> {
        Self::initialize(Connection::open_in_memory()?, None, 500, 64 * 1024 * 1024)
    }
    fn initialize(
        connection: Connection,
        lock: Option<File>,
        max_count: usize,
        max_bytes: usize,
    ) -> Result<Arc<Self>> {
        if max_count == 0 || max_bytes < 1024 {
            bail!("Job storage limits must allow at least one job and 1024 bytes")
        }
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS durable_jobs (job_id TEXT PRIMARY KEY, plan_json TEXT NOT NULL, updated_ms INTEGER NOT NULL, reserved_bytes INTEGER NOT NULL DEFAULT 0);")?;
        let columns = connection
            .prepare("PRAGMA table_info(durable_jobs)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let migrated = !columns.iter().any(|c| c == "reserved_bytes");
        if migrated {
            connection.execute_batch(
                "ALTER TABLE durable_jobs ADD COLUMN reserved_bytes INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        let store = Arc::new(Self {
            connection: Mutex::new(connection),
            _file_lock: lock,
            max_count,
            max_bytes,
        });
        for plan in store.list()? {
            {
                // Older development journals did not budget for interruption.
                // Reserve before conversion, also recovering a migration that
                // stopped after adding the column but before backfilling it.
                store.connection.lock().expect("job journal").execute(
                    "UPDATE durable_jobs SET reserved_bytes=?2 WHERE job_id=?1 AND reserved_bytes<?2",
                    params![plan.plan_id, i64::try_from(reserve(&plan))?],
                )?;
            }
            if matches!(
                plan.status,
                PlanStatus::Pending
                    | PlanStatus::Running
                    | PlanStatus::Pausing
                    | PlanStatus::Stopping
            ) {
                store.interrupt(&plan.plan_id)?;
            }
        }
        Ok(store)
    }
    pub fn list(&self) -> Result<Vec<Plan>> {
        let connection = self.connection.lock().expect("job journal");
        let mut statement = connection
            .prepare("SELECT plan_json FROM durable_jobs ORDER BY updated_ms DESC,job_id")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| serde_json::from_str(&s).context("Damaged job record; retained for recovery"))
            .collect()
    }
    pub fn get(&self, id: &str) -> Result<Plan> {
        let connection = self.connection.lock().expect("job journal");
        read(&connection, id)
    }
    pub fn insert(&self, mut plan: Plan) -> Result<()> {
        let connection = self.connection.lock().expect("job journal");
        validate(&plan)?;
        let count: i64 =
            connection.query_row("SELECT count(*) FROM durable_jobs", [], |r| r.get(0))?;
        if usize::try_from(count)? >= self.max_count {
            bail!("Job history is full. Remove finished jobs or increase jobs.max_count.")
        }
        stamp(&mut plan, "Job saved before execution");
        let encoded = serde_json::to_string(&plan)?;
        self.check_size(&connection, &plan.plan_id, encoded.len(), reserve(&plan))?;
        connection.execute(
            "INSERT INTO durable_jobs(job_id,plan_json,updated_ms,reserved_bytes) VALUES (?1,?2,?3,?4)",
            params![plan.plan_id, encoded, i64::try_from(plan.updated_at_ms)?, i64::try_from(reserve(&plan))?],
        )?;
        Ok(())
    }
    pub fn change(
        &self,
        id: &str,
        event: &str,
        change: impl FnOnce(&mut Plan) -> Result<()>,
    ) -> Result<Plan> {
        let connection = self.connection.lock().expect("job journal");
        let mut plan = read(&connection, id)?;
        change(&mut plan)?;
        stamp(&mut plan, event);
        let encoded = serde_json::to_string(&plan)?;
        self.check_size(&connection, id, encoded.len(), reserve(&plan))?;
        connection.execute(
            "UPDATE durable_jobs SET plan_json=?2,updated_ms=?3,reserved_bytes=?4 WHERE job_id=?1",
            params![
                id,
                encoded,
                i64::try_from(plan.updated_at_ms)?,
                i64::try_from(reserve(&plan))?
            ],
        )?;
        Ok(plan)
    }
    fn check_size(
        &self,
        connection: &Connection,
        id: &str,
        size: usize,
        reserved: usize,
    ) -> Result<()> {
        let cost = size.checked_add(reserved).context("Job size overflow")?;
        let previous: Option<i64> = connection.query_row("SELECT length(CAST(plan_json AS BLOB))+reserved_bytes FROM durable_jobs WHERE job_id=?1",[id],|r|r.get(0)).optional()?;
        // Lowering a quota must not prevent recovery/removal of existing work.
        // Only size-reducing checkpoints may exceed the newly lowered quota.
        if previous.is_some_and(|old| old >= 0 && cost as u64 <= old as u64) {
            return Ok(());
        }
        // Large media belongs on bounded data paths, not in the job journal.
        if cost > 8 * 1024 * 1024 {
            bail!("Job record exceeds 8 MiB; execution cannot advance without a durable checkpoint")
        }
        let other:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs WHERE job_id<>?1",[id],|r|r.get(0))?;
        if cost > self.max_bytes.saturating_sub(usize::try_from(other)?) {
            bail!("Job storage is full. Remove finished jobs or increase jobs.max_bytes.")
        }
        Ok(())
    }
    pub fn usage(&self) -> Result<serde_json::Value> {
        let connection = self.connection.lock().expect("job journal");
        let (count,payload,reserved):(i64,i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(plan_json AS BLOB))),0),COALESCE(SUM(reserved_bytes),0) FROM durable_jobs",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let used = usize::try_from(payload)?.saturating_add(usize::try_from(reserved)?);
        Ok(
            serde_json::json!({"count":count,"max_count":self.max_count,"payload_bytes":payload,"reserved_bytes":reserved,"max_bytes":self.max_bytes,"available_bytes":self.max_bytes.saturating_sub(used),"over_limit":used>self.max_bytes||usize::try_from(count)?>self.max_count}),
        )
    }
    pub fn interrupt(&self, id: &str) -> Result<()> {
        if !matches!(
            self.get(id)?.status,
            PlanStatus::Pending | PlanStatus::Running | PlanStatus::Pausing | PlanStatus::Stopping
        ) {
            return Ok(());
        }
        self.change(
            id,
            "Execution interrupted; review before resuming",
            |plan| {
                if matches!(
                    plan.status,
                    PlanStatus::Pending
                        | PlanStatus::Running
                        | PlanStatus::Pausing
                        | PlanStatus::Stopping
                ) {
                    plan.status = PlanStatus::Interrupted;
                    for task in &mut plan.tasks {
                        if task.status == TaskStatus::Running {
                            task.status = TaskStatus::Interrupted;
                        }
                    }
                }
                Ok(())
            },
        )?;
        Ok(())
    }
    pub fn remove_finished(&self, id: &str) -> Result<()> {
        let connection = self.connection.lock().expect("job journal");
        let plan = read(&connection, id)?;
        if !matches!(
            plan.status,
            PlanStatus::Completed | PlanStatus::Failed | PlanStatus::Cancelled
        ) {
            bail!("Only finished jobs can be removed")
        }
        connection.execute("DELETE FROM durable_jobs WHERE job_id=?1", [id])?;
        Ok(())
    }
}
fn read(connection: &Connection, id: &str) -> Result<Plan> {
    let raw: String = connection
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [id],
            |row| row.get(0),
        )
        .context("Job not found")?;
    serde_json::from_str(&raw).context("Damaged job record; retained for recovery")
}
fn stamp(plan: &mut Plan, event: &str) {
    plan.revision = plan.revision.saturating_add(1);
    plan.updated_at_ms = eefn::protocol::now_ms();
    plan.history.push(JobEvent {
        revision: plan.revision,
        at_ms: plan.updated_at_ms,
        message: event.into(),
    });
    if plan.history.len() > 128 {
        plan.history.remove(0);
    }
}
fn validate(plan: &Plan) -> Result<()> {
    if plan.status != PlanStatus::Pending
        || !plan.results.is_empty()
        || plan.revision != 0
        || !plan.history.is_empty()
    {
        bail!("New jobs must have fresh pending state")
    }
    if plan.plan_id.is_empty()
        || plan.plan_id.len() > 128
        || plan.tasks.is_empty()
        || plan.tasks.len() > 128
    {
        bail!("Jobs require an ID and between 1 and 128 tasks")
    }
    let mut seen = std::collections::HashSet::new();
    for task in &plan.tasks {
        if task.status != TaskStatus::Pending
            || task.result.is_some()
            || task.attempts != 0
            || !task.assignments.is_empty()
        {
            bail!("New tasks must have fresh pending state")
        }
        if task.task_id.is_empty()
            || task.task_id.len() > 128
            || !seen.insert(&task.task_id)
            || task.retry_count > 10
            || task.timeout_ms == 0
            || task.timeout_ms > 3_600_000
            || task.retry_delay_ms > 60_000
        {
            bail!("Invalid task identity, retry count or timeout")
        }
        if task.request_context != plan.goal.request_context {
            bail!("Task origin must match its job")
        }
    }
    let mut resolved = std::collections::HashSet::new();
    loop {
        let before = resolved.len();
        for task in &plan.tasks {
            if task.dependencies.iter().all(|id| resolved.contains(id)) {
                resolved.insert(task.task_id.clone());
            }
        }
        if resolved.len() == plan.tasks.len() {
            return Ok(());
        }
        if before == resolved.len() {
            bail!("Job dependencies contain a cycle or missing task")
        }
    }
}

/// UI responses deliberately omit file contents, media and raw execution parameters.
pub fn view(plan: &Plan, details: bool) -> serde_json::Value {
    use serde_json::json;
    let trim = |text: &str| text.chars().take(512).collect::<String>();
    let mut value = json!({
        "id": plan.plan_id, "description": trim(&plan.goal.description),
        "status": plan.status, "revision": plan.revision, "updated_at_ms": plan.updated_at_ms,
        "request_context": plan.goal.request_context, "task_count": plan.tasks.len(),
        "completed_tasks": plan.tasks.iter().filter(|t|t.status==TaskStatus::Completed).count(),
        "can_resume": matches!(plan.status,PlanStatus::Paused|PlanStatus::Interrupted) && !plan.tasks.iter().any(|t|t.status==TaskStatus::Interrupted&&!crate::task::retry_safe(t)),
    });
    if details {
        value["tasks"] =
            json!(plan.tasks.iter().map(|t|json!({
            "id":t.task_id,"capability":t.capability,"action":t.action,"status":t.status,
            "attempts":t.attempts,"assignment":t.assignments.last(),
            "error":t.result.as_ref().map(|r|trim(&r.error)),
        })).collect::<Vec<_>>());
        value["history"] = json!(plan.history);
    }
    value
}

impl crate::task::TaskEngine {
    /// Node requests are scoped to the immutable authenticated origin, not the executor.
    /// None is reserved for the loopback coordinator owner API.
    pub fn job_request(
        self: &Arc<Self>,
        kind: &str,
        payload: serde_json::Value,
        origin: Option<eefn::context::RequestContext>,
    ) -> Result<serde_json::Value> {
        use serde_json::json;
        let owns = |plan: &Plan| {
            origin.as_ref().is_none_or(|node| {
                plan.goal
                    .request_context
                    .as_ref()
                    .is_some_and(|c| c.origin_node() == node.origin_node())
            })
        };
        if kind == "jobs.list" {
            let all = self.store.list()?;
            let visible = all.iter().filter(|p| owns(p)).collect::<Vec<_>>();
            return Ok(
                json!({"jobs":visible.iter().take(100).map(|p|view(p,false)).collect::<Vec<_>>(),"total":visible.len(),"storage":self.store.usage()?}),
            );
        }
        if kind == "jobs.create" {
            let mut goal: crate::task::Goal = serde_json::from_value(payload)?;
            goal.request_context = origin;
            let id = self.enqueue(crate::task::TaskPlanner.plan(goal)?)?;
            return Ok(view(&self.store.get(&id)?, true));
        }
        let id = payload["id"].as_str().context("Job ID is required")?;
        let plan = self.store.get(id)?;
        if !owns(&plan) {
            bail!("Job not found for this node")
        }
        match kind {
            "jobs.get" => return Ok(view(&plan, true)),
            "jobs.pause" => {
                self.pause(id)?;
            }
            "jobs.resume" => self.resume_job(id)?,
            "jobs.stop" => {
                self.cancel(id)?;
            }
            "jobs.remove" => {
                self.store.remove_finished(id)?;
                return Ok(json!({"removed":true}));
            }
            _ => bail!("Unknown job operation"),
        }
        Ok(view(&self.store.get(id)?, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::EventBus,
        task::{Goal, Task, TaskEngine, TaskExecutor, TaskPlanner},
    };
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn plan(template: &str) -> Plan {
        TaskPlanner.plan(serde_json::from_value::<Goal>(json!({
            "description":"durable fixture", "template":template,
            "params":{"path":"fixture","content":"private output"},
            "request_context":{"request_id":"origin-request","origin_node":"owner","origin_area":["Home","Study"]}
        })).unwrap()).unwrap()
    }
    fn disk() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(Connection::open(&path).unwrap());
        (dir, path)
    }
    fn open(path: &Path) -> Arc<JobStore> {
        JobStore::open(path, 500, 64 * 1024 * 1024).unwrap()
    }
    async fn settled(engine: &TaskEngine, id: &str) -> Plan {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let p = engine.store.get(id).unwrap();
                if !matches!(
                    p.status,
                    PlanStatus::Pending
                        | PlanStatus::Running
                        | PlanStatus::Pausing
                        | PlanStatus::Stopping
                ) {
                    return p;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
    struct Counting(AtomicU32);
    #[async_trait]
    impl TaskExecutor for Counting {
        async fn execute(&self, _: &Task) -> Result<Value> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!("private result"))
        }
        async fn execute_journaled(&self, task: &Task, journal: AttemptJournal) -> Result<Value> {
            journal.assign("executor-node", Some("executor-node::file"), None)?;
            let saved = journal.store.get(&journal.job_id)?;
            let checkpoint = saved
                .tasks
                .iter()
                .find(|t| t.task_id == task.task_id)
                .unwrap();
            assert_eq!(checkpoint.status, TaskStatus::Running);
            assert_eq!(
                checkpoint.assignments.last().unwrap().attempt,
                task.attempts
            );
            self.execute(task).await
        }
    }
    struct Blocking {
        entered: tokio::sync::Semaphore,
        release: tokio::sync::Semaphore,
        calls: AtomicU32,
    }
    impl Blocking {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: tokio::sync::Semaphore::new(0),
                release: tokio::sync::Semaphore::new(0),
                calls: AtomicU32::new(0),
            })
        }
        async fn entered(&self) {
            tokio::time::timeout(Duration::from_secs(3), self.entered.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        }
    }
    #[async_trait]
    impl TaskExecutor for Blocking {
        async fn execute(&self, _: &Task) -> Result<Value> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
            Ok(json!("done"))
        }
    }

    #[tokio::test]
    async fn completed_results_origin_identity_and_assignment_survive_reopen() {
        let (_dir, path) = disk();
        let engine = TaskEngine::with_store(EventBus::new(20), open(&path));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let original = plan("read_file");
        let id = original.plan_id.clone();
        let task_id = original.tasks[0].task_id.clone();
        let completed = engine.run_plan(original.clone()).await.unwrap();
        assert_eq!(completed.status, PlanStatus::Completed);
        assert_eq!(executor.0.load(Ordering::SeqCst), 1);
        drop(engine);
        let store = open(&path);
        let restored = store.get(&id).unwrap();
        assert_eq!(restored.goal.request_context, original.goal.request_context);
        assert_eq!(restored.tasks[0].task_id, task_id);
        assert_eq!(restored.tasks[0].assignments[0].node_id, "executor-node");
        assert_eq!(restored.results[&task_id].data, "private result");
        assert_eq!(restored.revision, completed.revision);
        assert!(!view(&restored, true).to_string().contains("private result"));
    }

    #[tokio::test]
    async fn restart_never_auto_replays_and_explicit_resume_keeps_successful_predecessor() {
        let (_dir, path) = disk();
        let store = open(&path);
        let mut job = plan("read_file");
        let mut second = job.tasks[0].clone();
        second.task_id = "step-two".into();
        second.dependencies = vec![job.tasks[0].task_id.clone()];
        job.tasks.push(second);
        let id = job.plan_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "simulated process death", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Completed;
                let result = crate::task::TaskResult {
                    task_id: p.tasks[0].task_id.clone(),
                    success: true,
                    data: json!("saved result"),
                    error: String::new(),
                    error_type: String::new(),
                    duration_ms: 1,
                    attempts: 1,
                };
                p.tasks[0].result = Some(result.clone());
                p.results.insert(result.task_id.clone(), result);
                p.tasks[1].status = TaskStatus::Running;
                p.tasks[1].attempts = 1;
                Ok(())
            })
            .unwrap();
        drop(store);
        let store = open(&path);
        assert_eq!(store.get(&id).unwrap().status, PlanStatus::Interrupted);
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        engine.set_executor(executor.clone());
        tokio::task::yield_now().await;
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
        engine.resume_job(&id).unwrap();
        let restored = settled(&engine, &id).await;
        assert_eq!(restored.status, PlanStatus::Completed);
        assert_eq!(executor.0.load(Ordering::SeqCst), 1);
        assert_eq!(restored.tasks[1].attempts, 2);
        assert_eq!(
            restored.results[&restored.tasks[0].task_id].data,
            "saved result"
        );
    }

    #[tokio::test]
    async fn uncertain_mutation_refuses_resume_but_can_be_stopped() {
        let store = JobStore::in_memory().unwrap();
        let job = plan("write_file");
        let id = job.plan_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "interrupted mutation", |p| {
                p.status = PlanStatus::Interrupted;
                p.tasks[0].status = TaskStatus::Interrupted;
                Ok(())
            })
            .unwrap();
        let engine = TaskEngine::with_store(EventBus::new(10), store);
        engine.set_executor(Arc::new(Counting(AtomicU32::new(0))));
        assert!(
            engine
                .resume_job(&id)
                .unwrap_err()
                .to_string()
                .contains("may already have happened")
        );
        assert_eq!(
            engine.store.get(&id).unwrap().status,
            PlanStatus::Interrupted
        );
        assert_eq!(engine.cancel(&id).unwrap().status, PlanStatus::Cancelled);
        engine.store.remove_finished(&id).unwrap();
        assert!(engine.store.get(&id).is_err());
    }

    #[tokio::test]
    async fn pause_finishes_claimed_batch_and_resume_does_not_repeat_it() {
        let engine = TaskEngine::new(EventBus::new(20));
        let executor = Blocking::new();
        engine.set_executor(executor.clone());
        let mut job = plan("read_file");
        let mut second = job.tasks[0].clone();
        second.task_id = "second".into();
        second.dependencies = vec![job.tasks[0].task_id.clone()];
        job.tasks.push(second);
        let id = engine.enqueue(job).unwrap();
        executor.entered().await;
        assert_eq!(engine.pause(&id).unwrap().status, PlanStatus::Pausing);
        assert!(engine.resume_job(&id).is_err());
        executor.release.add_permits(1);
        let paused = settled(&engine, &id).await;
        assert_eq!(paused.status, PlanStatus::Paused);
        assert_eq!(paused.tasks[1].status, TaskStatus::Pending);
        engine.resume_job(&id).unwrap();
        executor.entered().await;
        assert!(engine.resume_job(&id).is_err());
        executor.release.add_permits(1);
        assert_eq!(settled(&engine, &id).await.status, PlanStatus::Completed);
        assert_eq!(executor.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stop_paused_job_does_not_wait_or_execute_pending_steps() {
        let engine = TaskEngine::new(EventBus::new(20));
        engine.set_executor(Arc::new(Counting(AtomicU32::new(0))));
        let job = plan("read_file");
        let id = job.plan_id.clone();
        engine.store.insert(job).unwrap();
        engine.pause(&id).unwrap();
        let stopped = engine.cancel(&id).unwrap();
        assert_eq!(stopped.status, PlanStatus::Cancelled);
        assert_eq!(stopped.tasks[0].status, TaskStatus::Cancelled);
        assert!(engine.resume_job(&id).is_err());
    }

    #[tokio::test]
    async fn shutdown_interrupts_runner_and_releases_database_ownership() {
        let (_dir, path) = disk();
        let engine = TaskEngine::with_store(EventBus::new(20), open(&path));
        let executor = Blocking::new();
        engine.set_executor(executor.clone());
        let id = engine.enqueue(plan("write_file")).unwrap();
        executor.entered().await;
        tokio::time::timeout(Duration::from_secs(3), engine.shutdown())
            .await
            .unwrap();
        assert_eq!(
            engine.store.get(&id).unwrap().status,
            PlanStatus::Interrupted
        );
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        drop(engine);
        assert_eq!(
            open(&path).get(&id).unwrap().tasks[0].status,
            TaskStatus::Interrupted
        );
    }

    #[test]
    fn database_has_one_owner_and_new_plans_reject_cycles_and_imported_state() {
        let (_dir, path) = disk();
        let store = open(&path);
        assert!(JobStore::open(&path, 500, 64 * 1024 * 1024).is_err());
        let mut job = plan("read_file");
        let task_id = job.tasks[0].task_id.clone();
        job.tasks[0].dependencies.push(task_id);
        assert!(store.insert(job).is_err());
        let mut job = plan("read_file");
        job.tasks[0].status = TaskStatus::Completed;
        assert!(store.insert(job).is_err());
        drop(store);
        assert!(JobStore::open(&path, 500, 64 * 1024 * 1024).is_ok());
    }

    #[test]
    fn quotas_and_stale_attempt_reject_changes_without_losing_saved_state() {
        let store =
            JobStore::initialize(Connection::open_in_memory().unwrap(), None, 1, 8192).unwrap();
        let job = plan("read_file");
        let id = job.plan_id.clone();
        let task_id = job.tasks[0].task_id.clone();
        store.insert(job).unwrap();
        assert!(store.insert(plan("read_file")).is_err());
        let before = serde_json::to_value(store.get(&id).unwrap()).unwrap();
        assert!(
            store
                .change(&id, "too large", |p| {
                    p.goal.description = "x".repeat(9000);
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(store.get(&id).unwrap()).unwrap(),
            before
        );
        let journal = AttemptJournal {
            store: store.clone(),
            job_id: id.clone(),
            task_id,
            attempt: 1,
        };
        assert!(journal.assign("node", None, None).is_err());
        assert!(store.remove_finished(&id).is_err());
    }

    #[tokio::test]
    async fn node_controls_are_origin_scoped_and_do_not_accept_forged_context() {
        let engine = TaskEngine::new(EventBus::new(20));
        engine.set_executor(Arc::new(Counting(AtomicU32::new(0))));
        let origin =
            eefn::context::RequestContext::new("r".into(), "node-a".into(), vec!["Room".into()])
                .unwrap();
        let other =
            eefn::context::RequestContext::new("s".into(), "node-b".into(), vec![]).unwrap();
        let created=engine.job_request("jobs.create",json!({"description":"fixture","template":"read_file","request_context":{"request_id":"fake","origin_node":"node-b","origin_area":[]}}),Some(origin.clone())).unwrap();
        let id = created["id"].as_str().unwrap();
        settled(&engine, id).await;
        assert_eq!(
            engine.store.get(id).unwrap().goal.request_context,
            Some(origin.clone())
        );
        assert_eq!(
            engine
                .job_request("jobs.list", json!({}), Some(other.clone()))
                .unwrap()["jobs"],
            json!([])
        );
        for kind in [
            "jobs.get",
            "jobs.pause",
            "jobs.resume",
            "jobs.stop",
            "jobs.remove",
        ] {
            assert!(
                engine
                    .job_request(kind, json!({"id":id}), Some(other.clone()))
                    .is_err()
            );
        }
        assert_eq!(
            engine
                .job_request("jobs.list", json!({}), Some(origin))
                .unwrap()["jobs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn timed_out_predecessor_keeps_pending_dependents_for_explicit_resume() {
        let engine = TaskEngine::new(EventBus::new(20));
        let blocking = Blocking::new();
        engine.set_executor(blocking);
        let mut job = plan("read_file");
        job.tasks[0].timeout_ms = 20;
        job.tasks[0].retry_count = 0;
        let mut next = job.tasks[0].clone();
        next.task_id = "dependent".into();
        next.dependencies = vec![job.tasks[0].task_id.clone()];
        job.tasks.push(next);
        let interrupted = engine.run_plan(job).await.unwrap();
        assert_eq!(interrupted.status, PlanStatus::Interrupted);
        assert_eq!(interrupted.tasks[1].status, TaskStatus::Pending);
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        engine.resume_job(&interrupted.plan_id).unwrap();
        assert_eq!(
            settled(&engine, &interrupted.plan_id).await.status,
            PlanStatus::Completed
        );
        assert_eq!(executor.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unavailable_checkpoint_space_prevents_executor_dispatch() {
        let mut job = plan("read_file");
        job.goal.description = "x".repeat(1024);
        let size = serde_json::to_vec(&job).unwrap().len();
        let store = JobStore::initialize(
            Connection::open_in_memory().unwrap(),
            None,
            1,
            size + RECOVERY_RESERVE + 200,
        )
        .unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        assert!(engine.run_plan(job).await.is_err());
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lowered_quota_still_allows_recovery_stop_and_history_removal() {
        let (_dir, path) = disk();
        let store = open(&path);
        let mut job = plan("read_file");
        job.goal.description = "x".repeat(6000);
        let id = job.plan_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "running fixture", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Running;
                Ok(())
            })
            .unwrap();
        assert_eq!(store.usage().unwrap()["reserved_bytes"], RECOVERY_RESERVE);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(store.get(&id).unwrap().status, PlanStatus::Interrupted);
        assert_eq!(store.usage().unwrap()["over_limit"], true);
        let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
        assert_eq!(engine.cancel(&id).unwrap().status, PlanStatus::Cancelled);
        store.remove_finished(&id).unwrap();
        assert_eq!(store.usage().unwrap()["count"], 0);
        assert_eq!(store.usage().unwrap()["available_bytes"], 1024);
    }

    #[test]
    fn legacy_development_journal_migrates_with_recovery_headroom() {
        for partially_migrated in [false, true] {
            let (_dir, path) = disk();
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch("CREATE TABLE durable_jobs(job_id TEXT PRIMARY KEY,plan_json TEXT NOT NULL,updated_ms INTEGER NOT NULL)").unwrap();
            let mut job = plan("write_file");
            job.status = PlanStatus::Running;
            job.tasks[0].status = TaskStatus::Running;
            let id = job.plan_id.clone();
            connection
                .execute(
                    "INSERT INTO durable_jobs VALUES (?1,?2,1)",
                    params![id, serde_json::to_string(&job).unwrap()],
                )
                .unwrap();
            if partially_migrated {
                connection.execute_batch("ALTER TABLE durable_jobs ADD COLUMN reserved_bytes INTEGER NOT NULL DEFAULT 0").unwrap();
            }
            drop(connection);
            let store = JobStore::open(&path, 1, 1024).unwrap();
            let restored = store.get(&id).unwrap();
            assert_eq!(restored.status, PlanStatus::Interrupted);
            assert_eq!(restored.goal.request_context, job.goal.request_context);
            assert_eq!(store.usage().unwrap()["reserved_bytes"], 1024);
            assert!(!view(&restored, false)["can_resume"].as_bool().unwrap());
        }
    }
}
