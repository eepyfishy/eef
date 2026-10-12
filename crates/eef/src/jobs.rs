//! Single-coordinator durable job journal. No election or cross-coordinator lease.
use crate::task::{Plan, PlanStatus, TaskStatus};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::File,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[path = "automation_authorizations.rs"]
mod automation_authorizations;
#[path = "automation_occurrences.rs"]
mod automation_occurrences;
#[path = "automation_schedules.rs"]
pub(crate) mod automation_schedules;
#[path = "automations.rs"]
mod automations;

// Logical quota headroom, not a promise that the physical disk cannot fill.
const RECOVERY_RESERVE: usize = 4096;
fn reserve(plan: &Plan) -> usize {
    let grant_reserve = crate::request_execution::reserved_bytes(plan);
    if grant_reserve > 0 {
        return grant_reserve;
    }
    match plan.status {
        PlanStatus::Pending | PlanStatus::Running => RECOVERY_RESERVE,
        PlanStatus::Pausing | PlanStatus::Stopping => 3072,
        PlanStatus::Review
        | PlanStatus::AwaitingApproval
        | PlanStatus::Approved
        | PlanStatus::Paused
        | PlanStatus::Interrupted => 1024,
        _ => 0,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub attempt: u32,
    pub node_id: String,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    /// Unknown for older development records; never infer a backend from an ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
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

pub(crate) struct JobAdmission {
    pub plan: Plan,
    pub reused: bool,
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
        self.assign_with_backend(node, resource, model, None)
    }
    pub fn assign_model(&self, node: &str, backend: &str, model: &str) -> Result<()> {
        if backend.is_empty() || backend.len() > 64 || backend.chars().any(char::is_control) {
            bail!("Invalid model backend assignment")
        }
        self.assign_with_backend(node, None, Some(model), Some(backend))
    }
    fn assign_with_backend(
        &self,
        node: &str,
        resource: Option<&str>,
        model: Option<&str>,
        backend: Option<&str>,
    ) -> Result<()> {
        eefn::network::validate_node_id(node)?;
        if node == "*" || self.attempt == 0 {
            bail!("Assignments require an exact executor and a nonzero attempt")
        }
        for value in resource.into_iter().chain(model) {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                bail!("Invalid resource/model assignment")
            }
        }
        self.store
            .change(&self.job_id, "Executor assigned", |plan| {
                if !matches!(
                    plan.status,
                    PlanStatus::Running | PlanStatus::Pausing | PlanStatus::Stopping
                ) {
                    bail!("Job no longer owns an active execution attempt")
                }
                let task = plan
                    .tasks
                    .iter_mut()
                    .find(|t| t.task_id == self.task_id)
                    .context("Unknown task")?;
                if task.status != TaskStatus::Running || task.attempts != self.attempt {
                    bail!("Stale execution attempt")
                }
                // An assignment is not authority to replay. Reject even an
                // identical second assignment; retries need a new attempt.
                if task.assignments.iter().any(|a| a.attempt == self.attempt) {
                    bail!("Execution attempt already assigned; do not replay or migrate it")
                }
                if task.assignments.len() >= 128 {
                    bail!("Task assignment history limit reached")
                }
                task.assignments.push(Assignment {
                    attempt: self.attempt,
                    node_id: node.into(),
                    resource_id: resource.map(str::to_owned),
                    model_id: model.map(str::to_owned),
                    backend: backend.map(str::to_owned),
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
        connection.execute_batch("CREATE TABLE IF NOT EXISTS automation_definitions (definition_id TEXT PRIMARY KEY, record_json TEXT NOT NULL);")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS automation_authorizations (authorization_id TEXT PRIMARY KEY, record_json TEXT NOT NULL, reserved_bytes INTEGER NOT NULL);")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS automation_occurrences (reservation_id TEXT PRIMARY KEY, job_id TEXT UNIQUE NOT NULL, record_json TEXT NOT NULL);")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS automation_occurrence_archives (archive_id TEXT PRIMARY KEY, reservation_id TEXT UNIQUE NOT NULL, job_id TEXT UNIQUE NOT NULL, record_json TEXT NOT NULL);")?;
        automation_schedules::initialize(&connection)?;
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
        store.validate_automations()?;
        automation_schedules::interrupt_on_open(&store)?;
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
            .map(|s| {
                let plan: Plan = serde_json::from_str(&s)
                    .context("Damaged job record; retained for recovery")?;
                validate_retained_review(&plan)?;
                automation_occurrences::validate_job(&connection, &plan)?;
                Ok(plan)
            })
            .collect()
    }
    pub fn get(&self, id: &str) -> Result<Plan> {
        let connection = self.connection.lock().expect("job journal");
        read(&connection, id)
    }
    pub fn insert(&self, plan: Plan) -> Result<()> {
        let connection = self.connection.lock().expect("job journal");
        self.insert_locked(&connection, plan)?;
        Ok(())
    }

    /// Deduplicate node-origin creation while its job is retained. The same
    /// journal lock covers lookup and insertion; no separate receipt can commit
    /// without its job. Legacy ambiguous receipts fail closed, never pick one.
    pub(crate) fn admit_request(&self, plan: Plan) -> Result<JobAdmission> {
        validate(&plan)?;
        let origin = plan
            .goal
            .request_context
            .as_ref()
            .context("Node origin is required")?;
        let connection = self.connection.lock().expect("job journal");
        automation_occurrences::refuse_archived_request(&connection, origin)?;
        let mut statement = connection.prepare("SELECT plan_json FROM durable_jobs")?;
        let mut rows = statement.query([])?;
        let mut existing: Option<Plan> = None;
        while let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            let saved: Plan =
                serde_json::from_str(&raw).context("Damaged job record; admission refused")?;
            validate_retained_review(&saved)?;
            if saved.goal.request_context.as_ref().is_some_and(|context| {
                context.origin_node() == origin.origin_node()
                    && context.request_id() == origin.request_id()
            }) {
                if existing.is_some() {
                    bail!(
                        "Operation receipt is ambiguous in retained history; inspect jobs.find before creating new work"
                    )
                }
                existing = Some(saved);
            }
        }
        drop(rows);
        drop(statement);
        if let Some(saved) = existing {
            // Retain the original origin/area snapshot, even after a node moves.
            // Compare input, not generated task IDs, attempts or mutable results.
            if match (&saved.request_review, &plan.request_review) {
                (Some(a), Some(b)) => !a.same_opening(b),
                (None, None) => {
                    saved.goal.description != plan.goal.description
                        || saved.goal.template != plan.goal.template
                        || saved.goal.params != plan.goal.params
                        || saved.goal.constraints != plan.goal.constraints
                }
                _ => true,
            } {
                bail!(
                    "Operation receipt conflicts with a retained job; use jobs.find to inspect it"
                )
            }
            return Ok(JobAdmission {
                plan: saved,
                reused: true,
            });
        }
        let plan = self.insert_locked(&connection, plan)?;
        Ok(JobAdmission {
            plan,
            reused: false,
        })
    }

    fn insert_locked(&self, connection: &Connection, mut plan: Plan) -> Result<Plan> {
        validate(&plan)?;
        if let Some(context) = &plan.goal.request_context {
            automation_schedules::source_authority(connection, context)?;
        }
        if connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM automation_occurrence_archives WHERE job_id=?1)",
            [&plan.plan_id],
            |r| r.get::<_, bool>(0),
        )? {
            bail!("Archived occurrence job identity cannot be recreated");
        }
        if automations::count(connection)? >= self.max_count {
            bail!("Job history is full. Remove finished jobs or increase jobs.max_count.")
        }
        let event = if plan.status == PlanStatus::Review {
            "Request review saved; no execution authorized"
        } else {
            "Job saved before execution"
        };
        stamp(&mut plan, event)?;
        let encoded = serde_json::to_string(&plan)?;
        self.check_size(connection, &plan.plan_id, encoded.len(), reserve(&plan))?;
        connection.execute(
            "INSERT INTO durable_jobs(job_id,plan_json,updated_ms,reserved_bytes) VALUES (?1,?2,?3,?4)",
            params![plan.plan_id, encoded, i64::try_from(plan.updated_at_ms)?, i64::try_from(reserve(&plan))?],
        )?;
        Ok(plan)
    }
    pub fn change(
        &self,
        id: &str,
        event: &str,
        change: impl FnOnce(&mut Plan) -> Result<()>,
    ) -> Result<Plan> {
        self.edit(id, event, |plan| {
            change(plan)?;
            Ok(true)
        })
    }
    pub(crate) fn edit(
        &self,
        id: &str,
        event: &str,
        edit: impl FnOnce(&mut Plan) -> Result<bool>,
    ) -> Result<Plan> {
        self.edit_with_gate(id, event, None, edit)
    }
    pub(crate) fn edit_with_policy_fence(
        &self,
        id: &str,
        event: &str,
        runtime: &str,
        issuer: &str,
        edit: impl FnOnce(&mut Plan) -> Result<bool>,
    ) -> Result<Plan> {
        self.edit_with_gate(id, event, Some((runtime, issuer)), edit)
    }
    fn edit_with_gate(
        &self,
        id: &str,
        event: &str,
        fence: Option<(&str, &str)>,
        edit: impl FnOnce(&mut Plan) -> Result<bool>,
    ) -> Result<Plan> {
        let connection = self.connection.lock().expect("job journal");
        let mut plan = read(&connection, id)?;
        let uncompiled = plan.planning.is_none();
        if let Some((runtime, issuer)) = fence {
            automation_occurrences::fence(
                &connection,
                &plan,
                runtime,
                issuer,
                eefn::protocol::now_ms,
            )?;
        }
        if !edit(&mut plan)? {
            return Ok(plan);
        }
        if uncompiled && plan.planning.is_some() {
            if let Some(context) = &plan.goal.request_context {
                automation_schedules::source_authority(&connection, context)?;
            }
        }
        stamp(&mut plan, event)?;
        validate_retained_review(&plan)?;
        automation_occurrences::validate_job(&connection, &plan)?;
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
        let other = usize::try_from(other)?
            .checked_add(automations::definition_bytes(connection)?)
            .context("Storage size overflow")?;
        if cost > self.max_bytes.saturating_sub(other) {
            bail!("Job storage is full. Remove finished jobs or increase jobs.max_bytes.")
        }
        Ok(())
    }
    pub fn usage(&self) -> Result<serde_json::Value> {
        let connection = self.connection.lock().expect("job journal");
        let (count,payload,reserved):(i64,i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(plan_json AS BLOB))),0),COALESCE(SUM(reserved_bytes),0) FROM durable_jobs",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let (automation_count,automation_bytes):(i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_definitions",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let (authorization_count,authorization_bytes,authorization_reserved_bytes):(i64,i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0),COALESCE(SUM(reserved_bytes),0) FROM automation_authorizations",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let (occurrence_count,occurrence_bytes):(i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrences", [], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let (occurrence_archive_count,occurrence_archive_bytes):(i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrence_archives", [], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let (schedule_count,schedule_bytes,schedule_reserved_bytes):(i64,i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0),COALESCE(SUM(reserved_bytes),0) FROM automation_schedules", [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let (trigger_count,trigger_bytes):(i64,i64)=connection.query_row("SELECT count(*),COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_triggers", [], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let used = usize::try_from(payload)?
            .saturating_add(usize::try_from(reserved)?)
            .saturating_add(usize::try_from(automation_bytes)?)
            .saturating_add(usize::try_from(authorization_bytes)?)
            .saturating_add(usize::try_from(authorization_reserved_bytes)?)
            .saturating_add(usize::try_from(occurrence_bytes)?)
            .saturating_add(usize::try_from(occurrence_archive_bytes)?)
            .saturating_add(usize::try_from(schedule_bytes)?)
            .saturating_add(usize::try_from(schedule_reserved_bytes)?)
            .saturating_add(usize::try_from(trigger_bytes)?);
        Ok(
            serde_json::json!({"count":count,"automation_count":automation_count,"automation_bytes":automation_bytes,"authorization_count":authorization_count,
                "authorization_bytes":authorization_bytes,"authorization_reserved_bytes":authorization_reserved_bytes,
                "occurrence_count":occurrence_count,"occurrence_bytes":occurrence_bytes,"max_count":self.max_count,
                "occurrence_archive_count":occurrence_archive_count,"occurrence_archive_bytes":occurrence_archive_bytes,
                "schedule_count":schedule_count,"schedule_bytes":schedule_bytes,"schedule_reserved_bytes":schedule_reserved_bytes,
                "trigger_count":trigger_count,"trigger_bytes":trigger_bytes,
                "payload_bytes":payload,"reserved_bytes":reserved,"max_bytes":self.max_bytes,"available_bytes":self.max_bytes.saturating_sub(used),
                "over_limit":used>self.max_bytes||automations::count(&connection)?>self.max_count}),
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
                    if crate::request_execution::can_settle_unissued_stop(plan) {
                        crate::request_execution::settle_unissued_stop(plan);
                        return Ok(());
                    }
                    crate::request_execution::interrupt(plan);
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
        automation_schedules::refuse_removal(&connection, &plan)?;
        if connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM automation_occurrences WHERE job_id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            bail!(
                "Occurrence lineage must be retained; use the exact owner automation-occurrences archive command. No budget refund or replacement authorized"
            );
        }
        if !matches!(
            plan.status,
            PlanStatus::Completed
                | PlanStatus::Failed
                | PlanStatus::Cancelled
                | PlanStatus::Abandoned
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
    let plan: Plan =
        serde_json::from_str(&raw).context("Damaged job record; retained for recovery")?;
    validate_retained_review(&plan)?;
    automation_occurrences::validate_job(connection, &plan)?;
    Ok(plan)
}
pub(crate) fn validate_retained_review(plan: &Plan) -> Result<()> {
    if let Some(draft) = &plan.request_review {
        draft.validate()?;
        if draft
            .proposals
            .iter()
            .filter_map(|p| p.resolution.as_ref())
            .any(|e| e.source_revision >= plan.revision)
        {
            bail!("Damaged resolution revision; retained for recovery")
        }
        let context = plan
            .goal
            .request_context
            .as_ref()
            .context("Damaged review origin; retained for recovery")?;
        for source in draft
            .proposals
            .iter()
            .filter_map(|p| p.preparation.as_ref())
        {
            if source.evidence.job_id != plan.plan_id
                || source.evidence.source_revision >= plan.revision
                || json!(source.request_context) != json!(context)
            {
                bail!("Damaged preparation origin/revision; retained for recovery")
            }
        }
        eefn::job_commands::validate_operation_id(context.request_id())?;
        eefn::network::validate_remote_node_id(context.origin_node())?;
        if (plan.execution.is_none() && !plan.results.is_empty())
            || context.request_id() != draft.proposals[0].operation_id
            || plan.goal.description != draft.proposals[0].proposal.goal
        {
            bail!("Damaged review state; retained for recovery")
        }
        if plan.planning.is_some() {
            crate::request_planning::validate_retained(plan)?;
        } else if plan.approval.is_some()
            || plan.execution.is_some()
            || !matches!(plan.status, PlanStatus::Review | PlanStatus::Cancelled)
            || !plan.tasks.is_empty()
            || plan.goal.template != "request_review"
            || plan.goal.params != json!({})
            || plan.goal.constraints != json!({})
        {
            bail!("Damaged unplanned review state; retained for recovery")
        }
    } else if matches!(
        plan.status,
        PlanStatus::Review
            | PlanStatus::AwaitingApproval
            | PlanStatus::Approved
            | PlanStatus::Abandoned
    ) || plan.planning.is_some()
        || plan.approval.is_some()
        || plan.execution.is_some()
    {
        bail!("Damaged review data; retained for recovery")
    }
    Ok(())
}
fn stamp(plan: &mut Plan, event: &str) -> Result<()> {
    plan.revision = plan
        .revision
        .checked_add(1)
        .context("Job revision exhausted; checkpoint unchanged")?;
    plan.updated_at_ms = eefn::protocol::now_ms();
    plan.history.push(JobEvent {
        revision: plan.revision,
        at_ms: plan.updated_at_ms,
        message: event.into(),
    });
    if plan.history.len() > 128 {
        plan.history.remove(0);
    }
    Ok(())
}
fn validate(plan: &Plan) -> Result<()> {
    validate_retained_review(plan)?;
    if !matches!(plan.status, PlanStatus::Pending | PlanStatus::Review)
        || !plan.results.is_empty()
        || plan.revision != 0
        || !plan.history.is_empty()
    {
        bail!("New jobs must have fresh pending state")
    }
    if let Some(draft) = &plan.request_review {
        validate_retained_review(plan)?;
        draft.validate()?;
        if plan.status != PlanStatus::Review
            || plan.planning.is_some()
            || !plan.tasks.is_empty()
            || draft.proposals.len() != 1
            || !draft.answers.is_empty()
            || plan
                .goal
                .request_context
                .as_ref()
                .is_none_or(|ctx| ctx.request_id() != draft.proposals[0].operation_id)
        {
            bail!("New review records must be fresh, origin-bound and non-executable")
        }
        if plan.plan_id.len() != 32 {
            bail!("Invalid review identity")
        }
        eefn::request_drafts::validate_id(&plan.plan_id)?;
        return Ok(());
    }
    if plan.status == PlanStatus::Review {
        bail!("Review record is missing its validated proposal")
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
        "can_resume": plan.request_review.is_none() && matches!(plan.status,PlanStatus::Paused|PlanStatus::Interrupted) && !plan.tasks.iter().any(|t|t.status==TaskStatus::Interrupted&&!crate::task::retry_safe(t)),
    });
    if let Some(draft) = &plan.request_review {
        value["review_only"] = json!(plan.execution.is_none());
        value["review_state"] = json!(draft.state());
        value["answer_count"] = json!(draft.answers.len());
        value["proposal_count"] = json!(draft.proposals.len());
        value["planning_state"] = json!(if plan.execution.is_some() {
            "execution_requested"
        } else if plan.approval.is_some() {
            "approved_no_grant"
        } else if plan.planning.is_some() {
            "compiled_no_grant"
        } else {
            "not_compiled"
        });
        if let Some(approval) = &plan.approval {
            value["approval_active"] = json!(approval.active_at(plan, eefn::protocol::now_ms()));
            value["approval_expires_at_ms"] = json!(approval.expires_at_ms);
        }
        if let Some(execution) = &plan.execution {
            value["tracking_abandoned"] = json!(execution.abandonment.is_some());
            value["remote_effects_unknown"] = json!(
                execution
                    .attempts
                    .iter()
                    .any(|a| a.state == crate::request_execution::AttemptState::Unknown)
            );
            value["can_abandon_tracking"] = json!(
                    plan.status == PlanStatus::Interrupted
                        && execution.abandonment.is_none()
                        && execution.attempts.last().is_some_and(
                            |a| a.state == crate::request_execution::AttemptState::Unknown
                        )
                );
            value["grant_attempts"] = json!(execution.attempts.iter().map(|a|json!({"task_id":a.grant.task_id,"grant_id":a.grant.grant_id,
                "state":a.state,"node_id":a.grant.executor_node,"acknowledged":a.acknowledgement.is_some(),"success":a.final_report().map(|r|r.success)})).collect::<Vec<_>>());
            if let Some(r) = execution.attempts.iter().find_map(|a| a.recovery.as_ref()) {
                value["recovery"] = json!({"operation_id":r.command.operation_id,"grant_id":r.command.grant_id,
                    "actor":r.actor,"applied_revision":r.command.expected_revision+1,"result_sha256":r.result.report_sha256,
                    "reconciled":true,"execution_requested":false,"automatically_retried":false,"remote_stop_confirmed":false});
            }
        }
    }
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
    fn job_view(&self, plan: &Plan, details: bool) -> serde_json::Value {
        let mut value = view(plan, details);
        if self.require_legacy_execution().is_err() {
            value["can_resume"] = json!(false);
        }
        value
    }
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
        if kind == "jobs.list" || kind == "jobs.find" {
            let operation_id = if kind == "jobs.find" {
                let id = payload["operation_id"]
                    .as_str()
                    .context("operation ID is required")?;
                eefn::job_commands::validate_operation_id(id)?;
                Some(id)
            } else {
                None
            };
            let all = self.store.list()?;
            let visible = all
                .iter()
                .filter(|p| {
                    owns(p)
                        && operation_id.is_none_or(|id| {
                            p.goal
                                .request_context
                                .as_ref()
                                .is_some_and(|context| context.request_id() == id)
                        })
                })
                .collect::<Vec<_>>();
            return Ok(
                json!({"jobs":visible.iter().take(100).map(|p|self.job_view(p,false)).collect::<Vec<_>>(),"total":visible.len(),"storage":self.store.usage()?,
                "operation_id":operation_id,"truncated":visible.len()>100}),
            );
        }
        if kind == "jobs.create" {
            self.require_legacy_execution()?;
            let mut goal: crate::task::Goal = serde_json::from_value(payload)?;
            goal.request_context = origin;
            let plan = crate::task::TaskPlanner.plan(goal)?;
            if plan.goal.request_context.is_some() {
                let admission = self.enqueue_request(plan)?;
                let mut result = view(&admission.plan, true);
                result["admission"] = json!({"reused":admission.reused,"scope":"origin_and_operation_id","retention":"while_job_history_exists"});
                return Ok(result);
            }
            let id = self.enqueue(plan)?;
            return Ok(view(&self.store.get(&id)?, true));
        }
        let id = payload["id"].as_str().context("Job ID is required")?;
        let plan = self.store.get(id)?;
        if !owns(&plan) {
            bail!("Job not found for this node")
        }
        match kind {
            "jobs.get" => return Ok(self.job_view(&plan, true)),
            "jobs.output" => return Ok(text_output(&plan)),
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
        Ok(self.job_view(&self.store.get(id)?, true))
    }
}

/// Explicit origin-authorized output inspection. Default job views stay redacted.
fn text_output(plan: &Plan) -> serde_json::Value {
    use serde_json::json;
    let matching = plan
        .tasks
        .iter()
        .filter(|t| matches!(t.capability.as_str(), "llm.infer" | "vlm.analyze"))
        .collect::<Vec<_>>();
    let mut remaining = 32768usize;
    let outputs = matching.iter().take(32).map(|task| {
        let text = if let Some(e) = &plan.execution {
            e.attempts.iter().find(|a|a.grant.task_id==task.task_id).and_then(|a|a.final_report()).and_then(|r|r.data["content"].as_str())
        } else {task.result.as_ref().and_then(|r| r.data["content"].as_str())};
        let mut truncated = false;
        let content = text.map(|text| {
            let mut end = text.len().min(remaining);
            while !text.is_char_boundary(end) { end -= 1; }
            remaining -= end; truncated = end < text.len();
            &text[..end]
        });
        json!({"task_id":task.task_id,"capability":task.capability,"status":task.status,"content":content,"truncated":truncated})
    }).collect::<Vec<_>>();
    json!({"schema_version":1,"report_type":"job_text_output","id":plan.plan_id,"status":plan.status,
        "outputs":outputs,"matching_tasks":matching.len(),"tasks_truncated":matching.len()>32,
        "note":"Explicit text-model output only; file contents, media and execution parameters are not exported. Pending output is null."})
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

    #[test]
    fn exhausted_revision_rejects_mutation_without_reusing_a_checkpoint_number() {
        let store = JobStore::in_memory().unwrap();
        let job = plan("generate_text");
        let id = job.plan_id.clone();
        store.insert(job).unwrap();
        let mut exhausted = store.get(&id).unwrap();
        exhausted.revision = u64::MAX;
        let raw = serde_json::to_string(&exhausted).unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
                params![id, raw],
            )
            .unwrap();
        assert!(
            store
                .change(&id, "must not commit", |p| {
                    p.status = PlanStatus::Cancelled;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(store.get(&id).unwrap()), json!(exhausted));
    }

    #[test]
    fn an_attempt_has_one_atomic_assignment_and_rejects_replay_or_migration() {
        let store = JobStore::in_memory().unwrap();
        let job = plan("generate_text");
        let id = job.plan_id.clone();
        let task_id = job.tasks[0].task_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "fixture attempt", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Running;
                p.tasks[0].attempts = 1;
                Ok(())
            })
            .unwrap();
        let journal = AttemptJournal {
            store: store.clone(),
            job_id: id.clone(),
            task_id,
            attempt: 1,
        };
        let barrier = std::sync::Barrier::new(24);
        let winners = std::thread::scope(|scope| {
            let contenders = (0..24)
                .map(|n| {
                    let journal = &journal;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        journal
                            .assign_model(&format!("node-{n}"), "ollama", "same-id")
                            .is_ok()
                    })
                })
                .collect::<Vec<_>>();
            contenders
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|won| *won)
                .count()
        });
        assert_eq!(winners, 1);
        let saved = store.get(&id).unwrap();
        assert_eq!(saved.tasks[0].assignments.len(), 1);
        let assignment = &saved.tasks[0].assignments[0];
        assert_eq!(assignment.backend.as_deref(), Some("ollama"));
        let before = json!(saved);
        assert!(
            journal
                .assign_model(&assignment.node_id, "ollama", "same-id")
                .is_err()
        );
        assert!(
            journal
                .assign_model("another-node", "llamacpp", "same-id")
                .is_err()
        );
        assert_eq!(json!(store.get(&id).unwrap()), before);
        store
            .change(&id, "fixture next attempt", |p| {
                p.tasks[0].attempts = 2;
                Ok(())
            })
            .unwrap();
        let before = json!(store.get(&id).unwrap());
        assert!(
            journal
                .assign_model("stale-node", "ollama", "same-id")
                .is_err()
        );
        assert_eq!(json!(store.get(&id).unwrap()), before);
        AttemptJournal {
            attempt: 2,
            ..journal.clone()
        }
        .assign_model("next-node", "llamacpp", "same-id")
        .unwrap();
        assert_eq!(store.get(&id).unwrap().tasks[0].assignments.len(), 2);
    }

    #[test]
    fn old_assignments_keep_backend_unknown_and_bad_assignments_do_not_change_history() {
        let old = json!({"attempt":1,"node_id":"node","resource_id":null,"model_id":"same-id","assigned_at_ms":1});
        let assignment: Assignment = serde_json::from_value(old.clone()).unwrap();
        assert!(assignment.backend.is_none());
        assert_eq!(json!(assignment), old);
        let store = JobStore::in_memory().unwrap();
        let job = plan("generate_text");
        let id = job.plan_id.clone();
        let task_id = job.tasks[0].task_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "fixture stopped attempt", |p| {
                p.status = PlanStatus::Interrupted;
                p.tasks[0].status = TaskStatus::Running;
                p.tasks[0].attempts = 1;
                Ok(())
            })
            .unwrap();
        let journal = AttemptJournal {
            store: store.clone(),
            job_id: id.clone(),
            task_id,
            attempt: 1,
        };
        let before = json!(store.get(&id).unwrap());
        assert!(journal.assign_model("node", "ollama", "same-id").is_err());
        for (node, backend, model) in [
            ("*", "ollama", "same-id"),
            (" node", "ollama", "same-id"),
            ("node", "", "same-id"),
            ("node", "ollama", ""),
        ] {
            assert!(journal.assign_model(node, backend, model).is_err());
        }
        assert_eq!(json!(store.get(&id).unwrap()), before);
    }

    #[test]
    fn explicit_text_output_is_bounded_utf8_and_does_not_export_files_or_parameters() {
        let mut job = plan("generate_text");
        let result = crate::task::TaskResult {
            task_id: job.tasks[0].task_id.clone(),
            success: true,
            data: json!({"content":"🙂".repeat(20000),"secret":"do not export"}),
            error: String::new(),
            error_type: String::new(),
            duration_ms: 0,
            attempts: 1,
        };
        job.tasks[0].result = Some(result);
        let mut file = job.tasks[0].clone();
        file.capability = "filesystem".into();
        job.tasks.push(file);
        let output = text_output(&job);
        assert_eq!(output["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(
            output["outputs"][0]["content"].as_str().unwrap().len(),
            32768
        );
        assert_eq!(output["outputs"][0]["truncated"], true);
        assert!(!output.to_string().contains("do not export"));
        assert!(!output.to_string().contains("private output"));
        assert!(!view(&job, true).to_string().contains('🙂'));
        job.tasks[0].result = None;
        assert!(text_output(&job)["outputs"][0]["content"].is_null());
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

    #[test]
    fn model_assignment_backend_survives_database_reopen_without_reassignment() {
        let (_dir, path) = disk();
        let store = open(&path);
        let job = plan("generate_text");
        let id = job.plan_id.clone();
        let task_id = job.tasks[0].task_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "fixture attempt", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Running;
                p.tasks[0].attempts = 1;
                Ok(())
            })
            .unwrap();
        let journal = AttemptJournal {
            store: store.clone(),
            job_id: id.clone(),
            task_id,
            attempt: 1,
        };
        journal
            .assign_model("selected-node", "llamacpp", "same-id")
            .unwrap();
        let assignment = json!(store.get(&id).unwrap().tasks[0].assignments[0]);
        drop(journal);
        drop(store);
        let reopened = open(&path);
        let restored = reopened.get(&id).unwrap();
        assert_eq!(restored.status, PlanStatus::Interrupted);
        assert_eq!(json!(restored.tasks[0].assignments[0]), assignment);
        assert_eq!(restored.tasks[0].assignments.len(), 1);
        assert_eq!(restored.tasks[0].attempts, 1);
    }

    fn receipt_origin(node: &str, id: &str, area: &str) -> eefn::context::RequestContext {
        eefn::context::RequestContext::new(id.into(), node.into(), vec![area.into()]).unwrap()
    }

    fn receipt_payload() -> Value {
        json!({"description":"receipt fixture","template":"write_file","params":{"path":"fixture","content":"once"}})
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_receipt_admission_dispatches_once_and_rejects_changed_input() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let receipt = uuid::Uuid::new_v4().to_string();
        let origin = receipt_origin("owner", &receipt, "Study");
        let barrier = Arc::new(tokio::sync::Barrier::new(32));
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let engine = engine.clone();
            let origin = origin.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                engine
                    .job_request("jobs.create", receipt_payload(), Some(origin))
                    .unwrap()
            }));
        }
        let mut replies = Vec::new();
        for task in tasks {
            replies.push(task.await.unwrap());
        }
        let id = replies[0]["id"].as_str().unwrap();
        assert!(replies.iter().all(|reply| reply["id"] == id));
        assert_eq!(
            replies
                .iter()
                .filter(|reply| reply["admission"]["reused"] == false)
                .count(),
            1
        );
        assert_eq!(settled(&engine, id).await.status, PlanStatus::Completed);
        assert_eq!(executor.0.load(Ordering::SeqCst), 1);
        let before = serde_json::to_value(engine.store.get(id).unwrap()).unwrap();
        let moved = receipt_origin("owner", &receipt, "Living room");
        let again = engine
            .job_request("jobs.create", receipt_payload(), Some(moved))
            .unwrap();
        assert_eq!(again["admission"]["reused"], true);
        assert_eq!(again["request_context"]["origin_area"], json!(["Study"]));
        assert_eq!(
            serde_json::to_value(engine.store.get(id).unwrap()).unwrap(),
            before
        );
        for (field, value) in [
            ("description", json!("changed")),
            ("template", json!("read_file")),
            ("params", json!({"path":"different"})),
            ("constraints", json!({"node_id":"different"})),
        ] {
            let mut changed = receipt_payload();
            changed[field] = value;
            let error = engine
                .job_request("jobs.create", changed, Some(origin.clone()))
                .unwrap_err();
            assert!(error.to_string().contains("conflicts"));
        }
        assert_eq!(engine.store.list().unwrap().len(), 1);
        let other = engine
            .job_request(
                "jobs.create",
                receipt_payload(),
                Some(receipt_origin("other", &receipt, "Study")),
            )
            .unwrap();
        assert_ne!(other["id"], id);
        assert_eq!(other["admission"]["reused"], false);
        settled(&engine, other["id"].as_str().unwrap()).await;
        assert_eq!(executor.0.load(Ordering::SeqCst), 2);
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn receipt_survives_restart_without_resuming_uncertain_work() {
        let (_dir, path) = disk();
        let store = open(&path);
        let origin = receipt_origin("owner", &uuid::Uuid::new_v4().to_string(), "Study");
        let mut goal: Goal = serde_json::from_value(receipt_payload()).unwrap();
        goal.request_context = Some(origin.clone());
        let admitted = store
            .admit_request(TaskPlanner.plan(goal).unwrap())
            .unwrap();
        let id = admitted.plan.plan_id;
        store
            .change(&id, "fixture uncertain dispatch", |plan| {
                plan.status = PlanStatus::Running;
                plan.tasks[0].status = TaskStatus::Running;
                plan.tasks[0].attempts = 1;
                Ok(())
            })
            .unwrap();
        drop(store);
        let store = open(&path);
        let before = serde_json::to_value(store.get(&id).unwrap()).unwrap();
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), store);
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let reused = engine
            .job_request("jobs.create", receipt_payload(), Some(origin))
            .unwrap();
        assert_eq!(reused["id"], id);
        assert_eq!(reused["status"], "interrupted");
        assert_eq!(reused["admission"]["reused"], true);
        assert_eq!(
            serde_json::to_value(engine.store.get(&id).unwrap()).unwrap(),
            before
        );
        engine.shutdown().await;
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn legacy_duplicate_receipts_fail_closed_without_modifying_history() {
        let store = JobStore::in_memory().unwrap();
        let job = plan("write_file");
        let goal = job.goal.clone();
        store.insert(job).unwrap();
        store
            .insert(TaskPlanner.plan(goal.clone()).unwrap())
            .unwrap();
        let before = serde_json::to_value(store.list().unwrap()).unwrap();
        let result = store.admit_request(TaskPlanner.plan(goal).unwrap());
        assert!(result.err().unwrap().to_string().contains("ambiguous"));
        assert_eq!(serde_json::to_value(store.list().unwrap()).unwrap(), before);
    }

    #[tokio::test]
    async fn receipt_reuse_at_quota_and_shutdown_do_not_create_new_runners() {
        let store = JobStore::initialize(Connection::open_in_memory().unwrap(), None, 1, 64 * 1024)
            .unwrap();
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), store);
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let origin = receipt_origin("owner", &uuid::Uuid::new_v4().to_string(), "Study");
        let first = engine
            .job_request("jobs.create", receipt_payload(), Some(origin.clone()))
            .unwrap();
        let id = first["id"].as_str().unwrap();
        settled(&engine, id).await;
        assert_eq!(
            engine
                .job_request("jobs.create", receipt_payload(), Some(origin.clone()))
                .unwrap()["admission"]["reused"],
            true
        );
        let other = receipt_origin("owner", &uuid::Uuid::new_v4().to_string(), "Study");
        assert!(
            engine
                .job_request("jobs.create", receipt_payload(), Some(other.clone()))
                .is_err()
        );
        assert_eq!(engine.store.list().unwrap().len(), 1);
        // History removal explicitly ends this protection; there is no hidden,
        // unlimited tombstone ledger or claim that deleted receipts remain safe.
        engine.store.remove_finished(id).unwrap();
        let recreated = engine
            .job_request("jobs.create", receipt_payload(), Some(origin.clone()))
            .unwrap();
        assert_ne!(recreated["id"], id);
        assert_eq!(recreated["admission"]["reused"], false);
        settled(&engine, recreated["id"].as_str().unwrap()).await;
        engine.shutdown().await;
        let before = serde_json::to_value(engine.store.list().unwrap()).unwrap();
        assert!(
            engine
                .job_request("jobs.create", receipt_payload(), Some(other))
                .is_err()
        );
        assert!(
            engine
                .job_request("jobs.create", receipt_payload(), Some(origin))
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(engine.store.list().unwrap()).unwrap(),
            before
        );
        assert_eq!(executor.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn receipt_admission_racing_shutdown_leaves_no_pending_job() {
        for _ in 0..20 {
            let engine = TaskEngine::legacy_for_test(EventBus::new(20));
            let origin = receipt_origin("owner", &uuid::Uuid::new_v4().to_string(), "Study");
            assert!(
                engine
                    .job_request("jobs.create", receipt_payload(), Some(origin.clone()))
                    .is_err()
            );
            assert!(
                engine.store.list().unwrap().is_empty(),
                "missing executor cannot persist work"
            );
            let executor = Arc::new(Counting(AtomicU32::new(0)));
            engine.set_executor(executor.clone());
            let barrier = Arc::new(tokio::sync::Barrier::new(17));
            let mut requests = Vec::new();
            for _ in 0..16 {
                let engine = engine.clone();
                let origin = origin.clone();
                let barrier = barrier.clone();
                requests.push(tokio::spawn(async move {
                    barrier.wait().await;
                    engine.job_request("jobs.create", receipt_payload(), Some(origin))
                }));
            }
            let stopping = engine.clone();
            let stop = tokio::spawn(async move {
                barrier.wait().await;
                stopping.shutdown().await;
            });
            let mut accepted = std::collections::HashSet::new();
            for request in requests {
                if let Ok(reply) = request.await.unwrap() {
                    accepted.insert(reply["id"].as_str().unwrap().to_owned());
                }
            }
            tokio::time::timeout(Duration::from_secs(5), stop)
                .await
                .unwrap()
                .unwrap();
            let saved = engine.store.list().unwrap();
            assert_eq!(saved.len(), accepted.len());
            assert!(saved.len() <= 1);
            assert!(
                saved.iter().all(|job| matches!(
                    job.status,
                    PlanStatus::Completed | PlanStatus::Interrupted
                ))
            );
            assert!(executor.0.load(Ordering::SeqCst) <= 1);
        }
    }

    #[test]
    fn damaged_history_refuses_receipt_admission_without_overwriting_evidence() {
        let store = JobStore::in_memory().unwrap();
        store.connection.lock().unwrap().execute(
            "INSERT INTO durable_jobs(job_id,plan_json,updated_ms,reserved_bytes) VALUES ('damaged','broken',0,0)", [],
        ).unwrap();
        let error = store.admit_request(plan("write_file")).err().unwrap();
        assert!(error.to_string().contains("Damaged job record"));
        let connection = store.connection.lock().unwrap();
        let count: i64 = connection
            .query_row("SELECT count(*) FROM durable_jobs", [], |row| row.get(0))
            .unwrap();
        let raw: String = connection
            .query_row(
                "SELECT plan_json FROM durable_jobs WHERE job_id='damaged'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(raw, "broken");
    }
    #[test]
    fn receipt_lookup_survives_reopen_and_filters_before_the_result_limit() {
        let (_dir, path) = disk();
        let store = open(&path);
        let operation_id = uuid::Uuid::new_v4().to_string();
        let origin =
            eefn::context::RequestContext::new(operation_id.clone(), "owner".into(), vec![])
                .unwrap();
        let mut target = plan("generate_text");
        target.goal.request_context = Some(origin.clone());
        let target = TaskPlanner.plan(target.goal).unwrap();
        let target_id = target.plan_id.clone();
        for job in std::iter::once(target).chain((0..105).map(|_| plan("generate_text"))) {
            let id = job.plan_id.clone();
            store.insert(job).unwrap();
            store
                .change(&id, "fixture retained history", |p| {
                    p.status = PlanStatus::Completed;
                    Ok(())
                })
                .unwrap();
        }
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE durable_jobs SET updated_ms=0 WHERE job_id=?1",
                [&target_id],
            )
            .unwrap();
        drop(store);
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(10), open(&path));
        let page = engine
            .job_request("jobs.list", json!({}), Some(origin.clone()))
            .unwrap();
        assert_eq!(page["total"], 106);
        assert_eq!(page["truncated"], true);
        assert!(
            !page["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|job| job["id"] == target_id)
        );
        let found = engine
            .job_request(
                "jobs.find",
                json!({"operation_id":operation_id}),
                Some(origin),
            )
            .unwrap();
        assert_eq!(found["total"], 1);
        assert_eq!(found["jobs"][0]["id"], target_id);
        assert!(!found.to_string().contains("private output"));
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
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), open(&path));
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
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), store);
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
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(10), store);
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
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
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
    async fn stop_after_resume_before_first_poll_does_not_dispatch() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let job = plan("write_file");
        let id = job.plan_id.clone();
        engine.store.insert(job).unwrap();
        engine.pause(&id).unwrap();
        engine.resume_job(&id).unwrap();
        assert_eq!(engine.cancel(&id).unwrap().status, PlanStatus::Stopping);
        let saved = settled(&engine, &id).await;
        assert_eq!(saved.status, PlanStatus::Cancelled);
        assert_eq!(saved.tasks[0].status, TaskStatus::Cancelled);
        assert_eq!(saved.tasks[0].attempts, 0);
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn stop_during_execution_keeps_finished_effects_and_skips_dependents() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Blocking::new();
        engine.set_executor(executor.clone());
        let mut job = plan("write_file");
        let mut second = job.tasks[0].clone();
        second.task_id = "after-stopped-batch".into();
        second.dependencies = vec![job.tasks[0].task_id.clone()];
        job.tasks.push(second);
        let id = engine.enqueue(job).unwrap();
        executor.entered().await;
        assert_eq!(engine.cancel(&id).unwrap().status, PlanStatus::Stopping);
        assert_eq!(
            engine.store.get(&id).unwrap().tasks[0].status,
            TaskStatus::Running
        );
        executor.release.add_permits(1);
        let saved = settled(&engine, &id).await;
        assert_eq!(saved.status, PlanStatus::Cancelled);
        assert_eq!(saved.tasks[0].status, TaskStatus::Completed);
        assert_eq!(saved.tasks[0].attempts, 1);
        assert_eq!(saved.results[&saved.tasks[0].task_id].data, "done");
        assert_eq!(saved.tasks[1].status, TaskStatus::Cancelled);
        assert_eq!(saved.tasks[1].attempts, 0);
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        let before = serde_json::to_value(&saved).unwrap();
        assert!(
            engine
                .cancel(&id)
                .unwrap_err()
                .to_string()
                .contains("already finished")
        );
        assert_eq!(
            serde_json::to_value(engine.store.get(&id).unwrap()).unwrap(),
            before
        );
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn stop_paused_job_does_not_wait_or_execute_pending_steps() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
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
    async fn shutdown_rejects_new_jobs_without_persisting_pending_work() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        engine.shutdown().await;
        let before = engine.store.usage().unwrap();
        assert!(
            engine
                .enqueue(plan("write_file"))
                .unwrap_err()
                .to_string()
                .contains("stopping")
        );
        assert!(
            engine.store.list().unwrap().is_empty(),
            "Rejected enqueue must not save a job"
        );
        assert!(
            engine
                .run_plan(plan("write_file"))
                .await
                .unwrap_err()
                .to_string()
                .contains("stopping")
        );
        assert!(
            engine.store.list().unwrap().is_empty(),
            "Rejected run_plan must not save a job"
        );
        assert_eq!(engine.store.usage().unwrap(), before);
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn missing_executor_rejects_both_admission_paths_without_saving() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        assert!(
            engine
                .enqueue(plan("read_file"))
                .unwrap_err()
                .to_string()
                .contains("no executor")
        );
        assert!(
            engine
                .run_plan(plan("read_file"))
                .await
                .unwrap_err()
                .to_string()
                .contains("no executor")
        );
        assert!(engine.store.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn shutdown_refuses_resume_without_changing_saved_checkpoints() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let mut ids = Vec::new();
        for status in [PlanStatus::Paused, PlanStatus::Interrupted] {
            let job = plan("read_file");
            let id = job.plan_id.clone();
            engine.store.insert(job).unwrap();
            engine
                .store
                .change(&id, "fixture checkpoint", |p| {
                    p.status = status;
                    Ok(())
                })
                .unwrap();
            ids.push(id);
        }
        let before = serde_json::to_value(engine.store.list().unwrap()).unwrap();
        engine.shutdown().await;
        for id in ids {
            assert!(
                engine
                    .resume_job(&id)
                    .unwrap_err()
                    .to_string()
                    .contains("stopping")
            );
        }
        assert_eq!(
            serde_json::to_value(engine.store.list().unwrap()).unwrap(),
            before
        );
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn shutdown_drains_admitted_jobs_before_their_first_poll() {
        // The single-thread runtime cannot poll spawned runners before shutdown.
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        let executor = Arc::new(Counting(AtomicU32::new(0)));
        engine.set_executor(executor.clone());
        let mut ids = Vec::new();
        for _ in 0..16 {
            ids.push(engine.enqueue(plan("write_file")).unwrap());
        }
        tokio::time::timeout(Duration::from_secs(3), engine.shutdown())
            .await
            .unwrap();
        for id in ids {
            let job = engine.store.get(&id).unwrap();
            assert_eq!(job.status, PlanStatus::Interrupted);
            assert_eq!(job.tasks[0].attempts, 0);
        }
        assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_admission_and_shutdown_leave_no_orphan_pending_jobs() {
        let engine = TaskEngine::legacy_for_test(EventBus::new(100));
        let executor = Blocking::new();
        engine.set_executor(executor.clone());
        let initial = engine.enqueue(plan("write_file")).unwrap();
        executor.entered().await;
        let barrier = Arc::new(tokio::sync::Barrier::new(33));
        let mut submissions = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let engine = engine.clone();
            let barrier = barrier.clone();
            submissions.spawn(async move {
                barrier.wait().await;
                engine.enqueue(plan("write_file"))
            });
        }
        let shutdown = tokio::spawn({
            let engine = engine.clone();
            async move {
                barrier.wait().await;
                engine.shutdown().await;
            }
        });
        let mut accepted = std::collections::HashSet::from([initial]);
        while let Some(result) = submissions.join_next().await {
            match result.unwrap() {
                Ok(id) => {
                    assert!(accepted.insert(id));
                }
                Err(error) => assert!(error.to_string().contains("stopping"), "{error}"),
            }
        }
        tokio::time::timeout(Duration::from_secs(5), shutdown)
            .await
            .unwrap()
            .unwrap();
        let stored = engine.store.list().unwrap();
        assert_eq!(
            stored.len(),
            accepted.len(),
            "Only admitted jobs may be saved"
        );
        for job in stored {
            assert!(accepted.contains(&job.plan_id));
            assert_eq!(job.status, PlanStatus::Interrupted);
            assert!(job.tasks[0].attempts <= 1);
        }
        assert!(engine.enqueue(plan("write_file")).is_err());
        assert_eq!(engine.store.list().unwrap().len(), accepted.len());
    }

    #[tokio::test]
    async fn failed_admission_releases_claim_without_changing_existing_jobs() {
        let store =
            JobStore::initialize(Connection::open_in_memory().unwrap(), None, 1, 65536).unwrap();
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), store);
        engine.set_executor(Arc::new(Counting(AtomicU32::new(0))));
        let existing = plan("read_file");
        let old_id = existing.plan_id.clone();
        engine.store.insert(existing).unwrap();
        engine.pause(&old_id).unwrap();
        let before = serde_json::to_value(engine.store.get(&old_id).unwrap()).unwrap();
        let next = plan("read_file");
        assert!(
            engine
                .enqueue(next.clone())
                .unwrap_err()
                .to_string()
                .contains("history is full")
        );
        assert_eq!(
            serde_json::to_value(engine.store.get(&old_id).unwrap()).unwrap(),
            before
        );
        engine.cancel(&old_id).unwrap();
        engine.store.remove_finished(&old_id).unwrap();
        let id = engine.enqueue(next).unwrap();
        assert_eq!(settled(&engine, &id).await.status, PlanStatus::Completed);
        engine.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_interrupts_runner_and_releases_database_ownership() {
        let (_dir, path) = disk();
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), open(&path));
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
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
        engine.set_executor(Arc::new(Counting(AtomicU32::new(0))));
        let operation_id = uuid::Uuid::new_v4().to_string();
        let origin = eefn::context::RequestContext::new(
            operation_id.clone(),
            "node-a".into(),
            vec!["Room".into()],
        )
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
        let found = engine
            .job_request(
                "jobs.find",
                json!({"operation_id":operation_id}),
                Some(origin.clone()),
            )
            .unwrap();
        assert_eq!(found["total"], 1);
        assert_eq!(found["jobs"][0]["id"], id);
        assert_eq!(found["truncated"], false);
        assert_eq!(
            engine
                .job_request(
                    "jobs.find",
                    json!({"operation_id":operation_id}),
                    Some(other.clone())
                )
                .unwrap()["total"],
            0
        );
        assert_eq!(
            engine
                .job_request(
                    "jobs.find",
                    json!({"operation_id":uuid::Uuid::new_v4().to_string()}),
                    Some(origin.clone())
                )
                .unwrap()["total"],
            0
        );
        assert!(
            engine
                .job_request(
                    "jobs.find",
                    json!({"operation_id":"bad"}),
                    Some(origin.clone())
                )
                .is_err()
        );
        assert_eq!(
            engine
                .job_request("jobs.list", json!({}), Some(other.clone()))
                .unwrap()["jobs"],
            json!([])
        );
        for kind in [
            "jobs.get",
            "jobs.output",
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
        let engine = TaskEngine::legacy_for_test(EventBus::new(20));
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
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(20), store);
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
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(10), store.clone());
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
