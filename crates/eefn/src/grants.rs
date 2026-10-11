//! Exact one-operation authority from an owner-pinned TLS coordinator session.
//! A receipt is consumed before acknowledgement/effects. It is never auto-replayed.
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const MAX_DURATION_MS: u64 = 30_000;
// Full report identity/flags/error overhead, in addition to bounded output.
pub(crate) const REPORT_HEADROOM: usize = 8192;
/// Durable coordinator journal hook. Called on validated, session-bound reports
/// before acknowledging them to the RPC caller; never relies on broadcast events.
pub trait GrantObserver: Send + Sync {
    fn record(&self, report: &GrantReport) -> Result<()>;
}
pub fn journal_path(config: &Path) -> std::path::PathBuf {
    let mut name = config.as_os_str().to_os_string();
    name.push(".grant-receipts.sqlite3");
    name.into()
}
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct GrantDenied(pub &'static str);
pub fn denial_code(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<GrantDenied>()
        .map_or("grant_journal_unavailable", |e| e.0)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub grant: ExecutionGrant,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantAdvertisement {
    pub schema_version: u32,
    pub available: bool,
    pub limits: Option<GrantPolicy>,
    pub deadline_guarantee: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_mode: Option<String>,
    #[serde(default)]
    pub retained_results: bool,
}
impl GrantAdvertisement {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.deadline_guarantee != "rpc_future_only"
            || (self.available && self.limits.is_none())
            || (self.retained_results && !self.available)
            || self
                .timing_mode
                .as_deref()
                .is_some_and(|mode| mode != crate::grant_time::MODE)
        {
            bail!("Invalid grant support advertisement")
        }
        if let Some(p) = &self.limits {
            p.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, clap::Subcommand)]
pub enum GrantAction {
    /// Inspect a consumed receipt locally; no connection or replay.
    Receipt {
        #[arg(long)]
        id: String,
    },
    /// Read an explicitly requested retained final report/output; never replay work.
    Result {
        #[arg(long)]
        id: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub grant_id: String,
}

impl crate::NodeService {
    pub fn grant_result(&self, request: ReceiptRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Invalid grant result schema/node")
        }
        let db = Connection::open_with_flags(
            journal_path(&self.config_path),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(Duration::from_millis(200))?;
        let result =
            crate::request_result::read_result(&db, &self.node_id, None, &request.grant_id)?;
        Ok(
            json!({"schema_version":1,"report_type":"node_grant_result","success":true,
            "node_id":self.node_id,"grant_id":request.grant_id,"result":result,
            "execution_requested":false,"automatically_retried":false,"reconciled":false,
            "note":"Explicit read-only retained report/output, not termination, rollback or replay authority. Missing output does not prove no effects. Coordinator tracking is unchanged."}),
        )
    }
    pub fn grant_receipt(&self, request: ReceiptRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Invalid grant receipt schema/node")
        }
        crate::job_commands::validate_operation_id(&request.grant_id)?;
        let path = journal_path(&self.config_path);
        // Never create, repair or initialize a database during inspection.
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_millis(200))?;
        let value =
            crate::request_receipt::read_consumed(&db, &self.node_id, None, &request.grant_id)?;
        Ok(
            json!({"schema_version":1,"report_type":"node_grant_receipt","success":true,"node_id":self.node_id,"grant_id":request.grant_id,
            "receipt":value,"execution_requested":false,"automatically_retried":false,
            "note":"Read-only consumed receipt; accepted may be running or interrupted/uncertain. No output or automatic recovery/replay is provided."}),
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GrantPolicy {
    pub max_receipts: usize,
    pub max_journal_bytes: usize,
    pub max_duration_ms: u64,
    pub max_result_bytes: usize,
}
impl Default for GrantPolicy {
    fn default() -> Self {
        Self {
            max_receipts: 4096,
            max_journal_bytes: 8 * 1024 * 1024,
            max_duration_ms: MAX_DURATION_MS,
            max_result_bytes: 64 * 1024,
        }
    }
}
impl GrantPolicy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100_000).contains(&self.max_receipts)
            || !(64 * 1024..=64 * 1024 * 1024).contains(&self.max_journal_bytes)
            || !(1..=MAX_DURATION_MS).contains(&self.max_duration_ms)
            || !(1024..=1024 * 1024).contains(&self.max_result_bytes)
        {
            bail!("Invalid grant receipt/duration/result bounds")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantLimits {
    pub max_execution_ms: u64,
    pub max_result_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionGrant {
    pub schema_version: u32,
    pub grant_id: String,
    pub issuer_id: String,
    pub session_id: String,
    pub executor_node: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub plan_sha256: String,
    pub request_context: crate::context::RequestContext,
    pub capability: String,
    pub action: String,
    pub params: Value,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    pub backend: Option<String>,
    pub model_role: Option<String>,
    pub priority: i32,
    pub limits: GrantLimits,
    pub reporting: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
}
fn hex_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
impl ExecutionGrant {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        crate::job_commands::validate_operation_id(&self.session_id)?;
        if uuid::Uuid::parse_str(&self.session_id)?.is_nil()
            || uuid::Uuid::parse_str(&self.grant_id)?.is_nil()
        {
            bail!("Nil grant/session identity")
        }
        crate::request_drafts::validate_id(&self.job_id)?;
        crate::request_drafts::validate_id(&self.task_id)?;
        crate::network::validate_remote_node_id(&self.executor_node)?;
        crate::network::validate_remote_node_id(self.request_context.origin_node())?;
        crate::job_commands::validate_operation_id(self.request_context.request_id())?;
        if self.schema_version != 1
            || !hex_digest(&self.issuer_id)
            || !hex_digest(&self.plan_sha256)
            || !(1..=11).contains(&self.attempt)
            || !identifier(&self.capability)
            || !identifier(&self.action)
            || !self.params.is_object()
            || serde_json::to_vec(&self.params)?.len() > 64 * 1024
            || !(-1000..=1000).contains(&self.priority)
            || self.reporting != "acceptance_and_completion"
            || !(1..=MAX_DURATION_MS).contains(&self.limits.max_execution_ms)
            || !(1024..=1024 * 1024).contains(&self.limits.max_result_bytes)
            || self.issued_at_ms == 0
            || self.expires_at_ms <= self.issued_at_ms
            || self.expires_at_ms - self.issued_at_ms > MAX_DURATION_MS
        {
            bail!("Invalid execution grant scope or limits")
        }
        if !matches!(
            (self.capability.as_str(), self.action.as_str()),
            ("system.info" | "system.ping", "run")
                | ("filesystem", "read" | "write" | "list")
                | ("process.exec", "run")
                | ("application.control", "launch")
                | ("http.request", "send")
                | ("stt.transcribe", "transcribe")
                | ("network.wol", "wake")
                | ("llm.infer" | "vlm.analyze", "run" | "analyze")
                | ("camera.capture" | "screen.capture", "capture")
                | ("audio.capture", "record")
                | ("audio.play", "play")
                | ("tts.speak", "speak")
                | (
                    "input.control",
                    "type" | "press" | "move" | "click" | "scroll"
                )
        ) {
            bail!("Unsupported granted operation contract")
        }
        if let Some(resource) = &self.resource_id {
            let prefix = format!("{}::", self.executor_node);
            if resource.len() > 256
                || resource.len() <= prefix.len()
                || !resource.starts_with(&prefix)
                || resource.chars().any(char::is_control)
                || self.params["resource_id"] != json!(resource)
            {
                bail!("Grant resource must be exact and belong to its executor")
            }
        } else if self.params.get("resource_id").is_some() {
            bail!("Ungranted physical resource")
        }
        if matches!(self.capability.as_str(), "llm.infer" | "vlm.analyze") {
            let model = self
                .model_id
                .as_deref()
                .context("Grant requires an exact model")?;
            crate::model_selection::validate_id(model)?;
            let backend = self
                .backend
                .as_deref()
                .context("Grant requires an exact backend")?;
            if !matches!(backend, "ollama" | "llamacpp")
                || self.resource_id.is_some()
                || self.params["model_id"] != json!(model)
                || self.params.get("model").is_some_and(|m| m != &json!(model))
                || self
                    .params
                    .get("backend")
                    .is_some_and(|b| b != &json!(backend))
            {
                bail!("Conflicting granted model/backend")
            }
            if self.model_role.as_deref().is_some_and(|r| !identifier(r)) {
                bail!("Invalid model role")
            }
        } else if self.model_id.is_some() || self.backend.is_some() || self.model_role.is_some() {
            bail!("Non-model operation cannot claim a model")
        }
        Ok(())
    }
    pub fn matches_session(&self, issuer: &str, session: &str, node: &str) -> bool {
        self.issuer_id == issuer && self.session_id == session && self.executor_node == node
    }
}
/// Explicit sorted-map canonicalization: stable across serde_json feature modes.
pub fn definition_digest(value: &Value) -> Result<String> {
    fn sorted(v: &Value) -> Value {
        match v {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), sorted(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&sorted(
        value,
    ))?)))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Accepted,
    Completed,
    Unknown,
    Rejected,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantReport {
    pub schema_version: u32,
    pub state: ReceiptState,
    pub grant_id: String,
    pub grant_sha256: String,
    pub issuer_id: String,
    pub session_id: String,
    pub executor_node: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub plan_sha256: String,
    pub accepted: bool,
    pub persisted: bool,
    pub execution_may_have_started: bool,
    pub success: bool,
    pub data: Value,
    pub error_code: Option<String>,
}
impl GrantReport {
    pub fn new(
        g: &ExecutionGrant,
        state: ReceiptState,
        data: Value,
        error_code: Option<&str>,
    ) -> Self {
        Self {
            schema_version: 1,
            state,
            grant_id: g.grant_id.clone(),
            grant_sha256: definition_digest(&json!(g)).expect("Typed grant serialization"),
            issuer_id: g.issuer_id.clone(),
            session_id: g.session_id.clone(),
            executor_node: g.executor_node.clone(),
            job_id: g.job_id.clone(),
            task_id: g.task_id.clone(),
            attempt: g.attempt,
            plan_sha256: g.plan_sha256.clone(),
            accepted: state != ReceiptState::Rejected,
            persisted: state != ReceiptState::Rejected,
            execution_may_have_started: matches!(
                state,
                ReceiptState::Completed | ReceiptState::Unknown
            ),
            success: state == ReceiptState::Completed && error_code.is_none(),
            data,
            error_code: error_code.map(str::to_owned),
        }
    }
    pub fn confirms(&self, g: &ExecutionGrant) -> bool {
        self.schema_version == 1
            && self.grant_id == g.grant_id
            && definition_digest(&json!(g)).is_ok_and(|h| h == self.grant_sha256)
            && self.issuer_id == g.issuer_id
            && self.session_id == g.session_id
            && self.executor_node == g.executor_node
            && self.job_id == g.job_id
            && self.task_id == g.task_id
            && self.attempt == g.attempt
            && self.plan_sha256 == g.plan_sha256
            && self.accepted == (self.state != ReceiptState::Rejected)
            && (!self.persisted || self.accepted)
            && (self.state == ReceiptState::Unknown || self.persisted == self.accepted)
            && self.execution_may_have_started
                == matches!(self.state, ReceiptState::Completed | ReceiptState::Unknown)
            && (!self.success
                || (self.state == ReceiptState::Completed && self.error_code.is_none()))
            && (self.state != ReceiptState::Accepted
                || (self.persisted
                    && self.data.is_null()
                    && !self.success
                    && self.error_code.is_none()))
            && serde_json::to_vec(&self.data).is_ok_and(|v| v.len() <= g.limits.max_result_bytes)
            && self.error_code.as_deref().is_none_or(identifier)
            && match self.state {
                ReceiptState::Rejected | ReceiptState::Unknown => {
                    !self.success && self.data.is_null() && self.error_code.is_some()
                }
                ReceiptState::Completed => self.success == self.error_code.is_none(),
                ReceiptState::Accepted => true,
            }
    }
}
pub struct GrantLedger {
    node_id: String,
    connection: Mutex<Connection>,
    policy: GrantPolicy,
}
impl GrantLedger {
    pub fn open(path: &Path, policy: GrantPolicy, node_id: &str) -> Result<Arc<Self>> {
        policy.validate()?;
        crate::network::validate_remote_node_id(node_id)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(200))?;
        let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            bail!("Damaged grant journal; retained for recovery")
        }
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !matches!(version, 0 | 1 | 2) {
            bail!("Unknown grant journal version; retained for recovery")
        }
        if version == 0 {
            let old: i64 = db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='execution_receipts'",
                [],
                |r| r.get(0),
            )?;
            if old != 0 {
                bail!("Unversioned grant receipts; retained for recovery")
            }
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch("CREATE TABLE IF NOT EXISTS execution_receipt_owner(owner TEXT NOT NULL PRIMARY KEY CHECK(owner='node'),node_id TEXT NOT NULL);")?;
            tx.execute(
                "INSERT OR IGNORE INTO execution_receipt_owner VALUES ('node',?1)",
                [node_id],
            )?;
            let owner: String = tx.query_row(
                "SELECT node_id FROM execution_receipt_owner WHERE owner='node'",
                [],
                |r| r.get(0),
            )?;
            if owner != node_id {
                bail!("Grant journal belongs to a different node")
            }
            tx.execute_batch("CREATE TABLE execution_receipts(issuer_id TEXT NOT NULL,job_id TEXT NOT NULL,task_id TEXT NOT NULL,attempt INTEGER NOT NULL,grant_id TEXT NOT NULL UNIQUE,grant_digest TEXT NOT NULL,state TEXT NOT NULL,accepted_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,PRIMARY KEY(issuer_id,job_id,task_id,attempt)); PRAGMA user_version=1;")?;
            tx.commit()?;
        }
        let saved_owner: Option<String> = db
            .query_row(
                "SELECT node_id FROM execution_receipt_owner WHERE owner='node'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if saved_owner.is_none() {
            bail!("Damaged grant journal owner; retained for recovery")
        }
        if saved_owner.as_deref().is_some_and(|id| id != node_id) {
            bail!("Grant journal belongs to a different node")
        }
        let invalid:i64=db.query_row("SELECT count(*) FROM execution_receipts WHERE length(issuer_id)!=64 OR length(job_id)!=32 OR length(task_id)!=32 OR attempt<1 OR attempt>11 OR length(grant_id)!=36 OR length(grant_digest)!=64 OR state NOT IN ('accepted','completed','unknown')",[],|r|r.get(0))?;
        if invalid != 0 {
            bail!("Damaged grant receipts; retained for recovery")
        }
        let ids = {
            let mut query = db.prepare("SELECT grant_id FROM execution_receipts")?;
            query
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        // Validate before migrating. Never turn a damaged legacy record into a
        // new schema or admit more work beside malformed retained evidence.
        for id in &ids {
            crate::request_result::read_result(&db, node_id, None, id)?;
        }
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
        let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        db.pragma_update(
            None,
            "max_page_count",
            i64::try_from(policy.max_journal_bytes)? / page_size,
        )?;
        // Additive migration retains consumed legacy receipts without inventing
        // output/success. Owner/version/metadata checks precede any migration.
        // Older binaries reject version 2; do not overlap or downgrade runtimes.
        if version != 2 {
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch("ALTER TABLE execution_receipts ADD COLUMN max_result_bytes INTEGER NOT NULL DEFAULT 0; ALTER TABLE execution_receipts ADD COLUMN report TEXT; ALTER TABLE execution_receipts ADD COLUMN report_sha256 TEXT; PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        Ok(Arc::new(Self {
            node_id: node_id.into(),
            connection: Mutex::new(db),
            policy,
        }))
    }
    pub fn policy(&self) -> &GrantPolicy {
        &self.policy
    }
    pub fn claim(&self, g: &ExecutionGrant) -> Result<()> {
        g.validate()?;
        if g.limits.max_result_bytes > self.policy.max_result_bytes
            || g.limits.max_execution_ms > self.policy.max_duration_ms
        {
            return Err(GrantDenied("grant_owner_limits_exceeded").into());
        }
        if g.executor_node != self.node_id {
            bail!("Grant journal owner mismatch")
        }
        let digest = definition_digest(&json!(g))?;
        let mut db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal lock unavailable"))?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let used: i64 = tx.query_row(
            "SELECT count(*) FROM execution_receipts WHERE grant_id=?1",
            [&g.grant_id],
            |r| r.get(0),
        )?;
        if used != 0 {
            return Err(GrantDenied("grant_attempt_replayed_or_unresolved").into());
        }
        let previous:Option<(u32,String)>=tx.query_row("SELECT attempt,state FROM execution_receipts WHERE issuer_id=?1 AND job_id=?2 AND task_id=?3 ORDER BY attempt DESC LIMIT 1",
            params![g.issuer_id,g.job_id,g.task_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if previous.is_some_and(|(attempt, state)| g.attempt <= attempt || state != "completed") {
            return Err(GrantDenied("grant_attempt_replayed_or_unresolved").into());
        }
        let count: i64 =
            tx.query_row("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))?;
        let pages =
            usize::try_from(tx.query_row::<i64, _, _>("PRAGMA page_count", [], |r| r.get(0))?)?;
        let page_size =
            usize::try_from(tx.query_row::<i64, _, _>("PRAGMA page_size", [], |r| r.get(0))?)?;
        let reserved: i64 = tx.query_row(
            "SELECT coalesce(sum(max_result_bytes+?2+4*?1),0) FROM execution_receipts WHERE state='accepted' AND max_result_bytes>0",
            params![i64::try_from(page_size)?,i64::try_from(REPORT_HEADROOM)?], |r| r.get(0),
        )?;
        if usize::try_from(count)? >= self.policy.max_receipts
            || pages
                .saturating_mul(page_size)
                .saturating_add(usize::try_from(reserved)?)
                .saturating_add(g.limits.max_result_bytes)
                .saturating_add(REPORT_HEADROOM)
                .saturating_add(4 * page_size)
                > self.policy.max_journal_bytes
        {
            return Err(GrantDenied("grant_receipt_quota_full").into());
        }
        let now = i64::try_from(crate::protocol::now_ms())?;
        tx.execute(
            "INSERT INTO execution_receipts(issuer_id,job_id,task_id,attempt,grant_id,grant_digest,state,accepted_ms,updated_ms,max_result_bytes,report) VALUES (?1,?2,?3,?4,?5,?6,'accepted',?7,?7,?8,NULL)",
            params![
                g.issuer_id,
                g.job_id,
                g.task_id,
                g.attempt,
                g.grant_id,
                digest,
                now,
                i64::try_from(g.limits.max_result_bytes)?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    fn finish(&self, g: &ExecutionGrant, report: &GrantReport) -> Result<()> {
        if !report.confirms(g) || !report.persisted {
            bail!("Invalid final grant report")
        }
        let state = match report.state {
            ReceiptState::Completed => "completed",
            ReceiptState::Unknown => "unknown",
            _ => bail!("Invalid terminal receipt"),
        };
        let encoded = serde_json::to_string(report)?;
        if encoded.len() > g.limits.max_result_bytes.saturating_add(REPORT_HEADROOM) {
            bail!("Retained report exceeds its reserved bound")
        }
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal lock unavailable"))?;
        let n=db.execute("UPDATE execution_receipts SET state=?2,updated_ms=max(accepted_ms,?3),report=?5,report_sha256=?7 WHERE grant_id=?1 AND grant_digest=?4 AND state='accepted' AND report IS NULL AND report_sha256 IS NULL AND max_result_bytes=?6",
            params![g.grant_id,state,i64::try_from(crate::protocol::now_ms())?,definition_digest(&json!(g))?,encoded,i64::try_from(g.limits.max_result_bytes)?,definition_digest(&json!(report))?])?;
        if n != 1 {
            bail!("Grant receipt changed before completion")
        }
        Ok(())
    }
    pub fn receipt(&self, issuer: &str, grant_id: &str) -> Result<Value> {
        crate::job_commands::validate_operation_id(grant_id)?;
        if !hex_digest(issuer) {
            bail!("Invalid receipt issuer")
        }
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal lock unavailable"))?;
        Ok(json!(crate::request_receipt::read_consumed(
            &db,
            &self.node_id,
            Some(issuer),
            grant_id
        )?))
    }
    pub fn result(&self, issuer: &str, grant_id: &str, expected_digest: &str) -> Result<Value> {
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal lock unavailable"))?;
        let result =
            crate::request_result::read_result(&db, &self.node_id, Some(issuer), grant_id)?;
        if result.receipt.grant_sha256 != expected_digest {
            bail!("Retained report scope changed")
        }
        Ok(json!(result))
    }
}
pub struct AcceptedGrant {
    grant: ExecutionGrant,
    ledger: Arc<GrantLedger>,
    deadline: Instant,
    accepted: Instant,
    accepted_wall: u64,
    final_checkpoint_attempted: bool,
}
impl Drop for AcceptedGrant {
    fn drop(&mut self) {
        if !self.final_checkpoint_attempted {
            // Connection/task cancellation is not remote/backend termination.
            // Retain uncertainty and consumption while releasing unfinished
            // output reservations when possible. No replay or second write
            // attempt after an explicit final checkpoint failure.
            let report = GrantReport::new(
                &self.grant,
                ReceiptState::Unknown,
                Value::Null,
                Some("grant_runtime_dropped_effects_unknown"),
            );
            let _ = self.ledger.finish(&self.grant, &report);
        }
    }
}
impl AcceptedGrant {
    /// Aligned-wall-clock adapter is test-only; release builds require an
    /// authenticated fresh lease proof and expose no timestamp fallback.
    #[cfg(test)]
    pub fn prepare(
        g: ExecutionGrant,
        ledger: Arc<GrantLedger>,
        engine: &crate::NodeEngine,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Self> {
        let now = crate::protocol::now_ms();
        if now < g.issued_at_ms || now >= g.expires_at_ms {
            return Err(GrantDenied("grant_clock_or_expiry_invalid").into());
        }
        let deadline = Instant::now()
            + Duration::from_millis(g.limits.max_execution_ms.min(g.expires_at_ms - now));
        Self::prepare_deadline(g, ledger, engine, issuer, session, node, deadline)
    }
    pub fn prepare_with_lease(
        g: ExecutionGrant,
        ledger: Arc<GrantLedger>,
        engine: &crate::NodeEngine,
        issuer: &str,
        session: &str,
        node: &str,
        lease: crate::grant_time::GrantLease,
    ) -> Result<Self> {
        let deadline = lease
            .deadline(&g)
            .map_err(|_| GrantDenied("grant_lease_changed_or_expired"))?;
        Self::prepare_deadline(g, ledger, engine, issuer, session, node, deadline)
    }
    fn prepare_deadline(
        g: ExecutionGrant,
        ledger: Arc<GrantLedger>,
        engine: &crate::NodeEngine,
        issuer: &str,
        session: &str,
        node: &str,
        deadline: Instant,
    ) -> Result<Self> {
        g.validate()
            .map_err(|_| GrantDenied("invalid_grant_scope"))?;
        if !g.matches_session(issuer, session, node) {
            return Err(GrantDenied("grant_identity_changed").into());
        }
        if Instant::now() >= deadline {
            return Err(GrantDenied("grant_lease_changed_or_expired").into());
        }
        if g.limits.max_execution_ms > ledger.policy.max_duration_ms
            || g.limits.max_result_bytes > ledger.policy.max_result_bytes
        {
            return Err(GrantDenied("grant_owner_limits_exceeded").into());
        }
        engine
            .check_grant_policy(&g)
            .map_err(|_| GrantDenied("local_policy_denied_or_changed"))?;
        let accepted = Instant::now();
        // Pair both local clocks before the durable claim. Its I/O consumes
        // the existing lease and a clock jump during it must not be hidden
        // by taking a new wall-clock baseline after the write.
        let accepted_wall = crate::protocol::now_ms();
        let deadline = deadline.min(accepted + Duration::from_millis(g.limits.max_execution_ms));
        ledger.claim(&g)?;
        Ok(Self {
            grant: g,
            ledger,
            deadline,
            accepted,
            accepted_wall,
            final_checkpoint_attempted: false,
        })
    }
    pub fn acknowledgement(&self) -> GrantReport {
        GrantReport::new(&self.grant, ReceiptState::Accepted, Value::Null, None)
    }
    pub async fn run(mut self, engine: &crate::NodeEngine) -> GrantReport {
        let g = &self.grant;
        let mut params = g.params.clone();
        if let Some(model) = &g.model_id {
            params["model"] = json!(model);
            params["backend"] = json!(g.backend);
        }
        let operation = engine.execute(&g.capability, &g.action, params);
        tokio::pin!(operation);
        let deadline = tokio::time::Instant::from_std(self.deadline);
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        let mut result = loop {
            let now = crate::protocol::now_ms();
            let expected = self.accepted_wall.saturating_add(
                self.accepted
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
            );
            if Instant::now() >= self.deadline || now.abs_diff(expected) > 1000 {
                break GrantReport::new(
                    g,
                    ReceiptState::Unknown,
                    Value::Null,
                    Some("grant_deadline_or_clock_changed"),
                );
            }
            if engine.check_grant_policy(g).is_err() {
                break GrantReport::new(
                    g,
                    ReceiptState::Unknown,
                    Value::Null,
                    Some("local_policy_changed"),
                );
            }
            tokio::select! {
                biased;
                _=tokio::time::sleep_until(deadline)=>break GrantReport::new(g,ReceiptState::Unknown,Value::Null,Some("grant_deadline_exceeded")),
                _=tick.tick()=>{},
                value=&mut operation=>break match value {
                    Ok(data) if serde_json::to_vec(&data).is_ok_and(|v|v.len()<=g.limits.max_result_bytes)=>GrantReport::new(g,ReceiptState::Completed,data,None),
                    Ok(_)=>GrantReport::new(g,ReceiptState::Completed,Value::Null,Some("result_too_large")),
                    Err(_)=>GrantReport::new(g,ReceiptState::Unknown,Value::Null,Some("execution_failed_effects_unknown")),
                }
            }
        };
        if result.state == ReceiptState::Completed && Instant::now() >= self.deadline {
            result = GrantReport::new(
                g,
                ReceiptState::Unknown,
                Value::Null,
                Some("completion_after_deadline"),
            );
        }
        self.final_checkpoint_attempted = true;
        if self.ledger.finish(g, &result).is_err() {
            result = GrantReport::new(
                g,
                ReceiptState::Unknown,
                Value::Null,
                Some("completion_checkpoint_failed"),
            );
            result.persisted = false;
        }
        result
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[tokio::test]
    async fn dropped_grant_retains_uncertainty_without_replay_and_never_overwrites_completed_evidence()
     {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let g = grant();
        let engine = engine(dir.path());
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            "owner",
        )
        .unwrap();
        assert_eq!(
            ledger
                .result(
                    &g.issuer_id,
                    &g.grant_id,
                    &definition_digest(&json!(g)).unwrap()
                )
                .unwrap()["availability"],
            "no_final_checkpoint"
        );
        drop(accepted);
        let unknown: crate::request_result::RetainedResult = serde_json::from_value(
            ledger
                .result(
                    &g.issuer_id,
                    &g.grant_id,
                    &definition_digest(&json!(g)).unwrap(),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(unknown.receipt.state, "unknown");
        assert!(unknown.confirms(&g));
        let report = unknown.report.as_ref().unwrap();
        assert!(!report.success);
        assert!(report.data.is_null());
        assert_eq!(
            report.error_code.as_deref(),
            Some("grant_runtime_dropped_effects_unknown")
        );
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        let completed = grant();
        let accepted = AcceptedGrant::prepare(
            completed.clone(),
            ledger.clone(),
            &engine,
            &completed.issuer_id,
            &completed.session_id,
            "owner",
        )
        .unwrap();
        let report = accepted.run(&engine).await;
        assert!(report.success);
        let saved = ledger
            .result(
                &completed.issuer_id,
                &completed.grant_id,
                &definition_digest(&json!(completed)).unwrap(),
            )
            .unwrap();
        assert_eq!(
            saved["report"],
            json!(report),
            "normal completion must not run the dropped-runtime checkpoint"
        );
        drop(ledger);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert_eq!(
            json!(
                reopened
                    .result(
                        &g.issuer_id,
                        &g.grant_id,
                        &definition_digest(&json!(g)).unwrap()
                    )
                    .unwrap()
            ),
            json!(unknown)
        );
    }
    pub(crate) fn grant() -> ExecutionGrant {
        let now = crate::protocol::now_ms();
        ExecutionGrant {
            schema_version: 1,
            grant_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            session_id: uuid::Uuid::new_v4().to_string(),
            executor_node: "owner".into(),
            job_id: uuid::Uuid::new_v4().simple().to_string(),
            task_id: uuid::Uuid::new_v4().simple().to_string(),
            attempt: 1,
            plan_sha256: "b".repeat(64),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                "owner".into(),
                vec!["Room".into()],
            )
            .unwrap(),
            capability: "system.ping".into(),
            action: "run".into(),
            params: json!({}),
            resource_id: None,
            model_id: None,
            backend: None,
            model_role: None,
            priority: 0,
            limits: GrantLimits {
                max_execution_ms: 5000,
                max_result_bytes: 65536,
            },
            reporting: "acceptance_and_completion".into(),
            issued_at_ms: now - 1,
            expires_at_ms: now + 10_000,
        }
    }

    #[test]
    fn fractional_result_round_trips_preserve_integrity_digest() {
        let g = grant();
        // Timestamp-scale floats plus signed fractions/subnormals. A default
        // approximate decimal parser may change a last bit after storage/wire
        // decode; valid retained evidence must not fail its exact checksum.
        for index in 0..512_u64 {
            let sample = f64::from_bits(0x41dab2de80000000 + index * 7919);
            let report = GrantReport::new(
                &g,
                ReceiptState::Completed,
                json!({"time":sample,"signed":-sample,"tiny":f64::from_bits(index+1),"fraction":sample/7.0}),
                None,
            );
            let encoded = serde_json::to_vec(&report).unwrap();
            let decoded: GrantReport = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(
                definition_digest(&json!(report)).unwrap(),
                definition_digest(&json!(decoded)).unwrap(),
                "Valid numeric evidence changed at sample {index}"
            );
            assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        }
    }
    fn engine(dir: &Path) -> crate::NodeEngine {
        crate::NodeEngine::new(false, vec![], dir.to_owned()).unwrap()
    }
    #[test]
    fn strict_scope_and_report_reject_authority_widening() {
        let g = grant();
        g.validate().unwrap();
        let mut raw = json!(g);
        raw["permissions"] = json!(true);
        assert!(serde_json::from_value::<ExecutionGrant>(raw).is_err());
        let mutations: [fn(&mut ExecutionGrant); 6] = [
            |g: &mut ExecutionGrant| g.session_id = uuid::Uuid::nil().to_string(),
            |g: &mut ExecutionGrant| g.executor_node = "local".into(),
            |g: &mut ExecutionGrant| g.capability = "node.configure".into(),
            |g: &mut ExecutionGrant| g.limits.max_execution_ms = 30001,
            |g: &mut ExecutionGrant| g.params["resource_id"] = json!("other::cam"),
            |g: &mut ExecutionGrant| g.model_id = Some("ungranted".into()),
        ];
        for mutate in mutations {
            let mut bad = g.clone();
            mutate(&mut bad);
            assert!(bad.validate().is_err());
        }
        let ack = GrantReport::new(&g, ReceiptState::Accepted, Value::Null, None);
        assert!(ack.confirms(&g));
        let mut bad = ack.clone();
        bad.persisted = false;
        assert!(!bad.confirms(&g));
        let mut bad = ack.clone();
        bad.execution_may_have_started = true;
        assert!(!bad.confirms(&g));
        let mut bad = ack.clone();
        bad.grant_sha256 = "c".repeat(64);
        assert!(!bad.confirms(&g));
        let result = GrantReport::new(&g, ReceiptState::Completed, json!({"ok":true}), None);
        assert!(result.confirms(&g));
        let mut bad = result;
        bad.persisted = false;
        assert!(!bad.confirms(&g));
        let mut vision = g.clone();
        vision.capability = "llm.infer".into();
        vision.model_id = Some("m".into());
        vision.backend = Some("ollama".into());
        vision.params = json!({"model_id":"m","prompt":"x"});
        vision.validate().unwrap();
        vision.params["backend"] = json!("llamacpp");
        assert!(vision.validate().is_err());
    }
    #[test]
    fn racing_receipts_replay_and_quota_fail_before_effects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let ledger = GrantLedger::open(
            &path,
            GrantPolicy {
                max_receipts: 1,
                ..Default::default()
            },
            "owner",
        )
        .unwrap();
        let g = grant();
        let independent = GrantLedger::open(
            &path,
            GrantPolicy {
                max_receipts: 1,
                ..Default::default()
            },
            "owner",
        )
        .unwrap();
        let barrier = std::sync::Barrier::new(24);
        let wins = std::thread::scope(|s| {
            let futures = (0..24)
                .map(|index| {
                    let handle = if index % 2 == 0 {
                        &ledger
                    } else {
                        &independent
                    };
                    let barrier = &barrier;
                    let g = &g;
                    s.spawn(move || {
                        barrier.wait();
                        handle.claim(g).is_ok()
                    })
                })
                .collect::<Vec<_>>();
            futures
                .into_iter()
                .map(|f| f.join().unwrap())
                .filter(|ok| *ok)
                .count()
        });
        assert_eq!(wins, 1);
        assert!(ledger.claim(&g).is_err());
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        let another = grant();
        assert_eq!(
            denial_code(&ledger.claim(&another).unwrap_err()),
            "grant_receipt_quota_full"
        );
        let mut higher = g.clone();
        higher.attempt = 2;
        higher.grant_id = uuid::Uuid::new_v4().to_string();
        assert!(ledger.claim(&higher).is_err());
        drop(ledger);
        drop(independent);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert!(reopened.claim(&g).is_err());
        assert!(reopened.claim(&higher).is_err());
        assert!(reopened.receipt(&"c".repeat(64), &g.grant_id).is_err());
        assert!(GrantLedger::open(&path, Default::default(), "other").is_err());
    }
    #[tokio::test]
    async fn preparation_requires_exact_session_clock_policy_and_durable_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let engine = engine(dir.path());
        let g = grant();
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                "other",
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        let mut expired = g.clone();
        expired.expires_at_ms = expired.issued_at_ms + 1;
        assert!(
            AcceptedGrant::prepare(
                expired,
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        let mut future = g.clone();
        future.issued_at_ms = crate::protocol::now_ms() + 1000;
        future.expires_at_ms = future.issued_at_ms + 1000;
        assert!(
            AcceptedGrant::prepare(
                future,
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        let mut denied = g.clone();
        denied.capability = "filesystem".into();
        denied.action = "write".into();
        denied.params = json!({"path":"out","content":"data"});
        assert!(
            AcceptedGrant::prepare(
                denied.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        assert!(ledger.receipt(&g.issuer_id, &denied.grant_id).is_err());
        let prepared = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            "owner",
        )
        .unwrap();
        assert!(prepared.acknowledgement().confirms(&g));
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        let done = prepared.run(&engine).await;
        assert!(done.success);
        assert!(done.confirms(&g));
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "completed"
        );
        assert!(
            AcceptedGrant::prepare(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        );
        let mut later = grant();
        later.limits.max_execution_ms = 1;
        let prepared = AcceptedGrant::prepare(
            later.clone(),
            ledger.clone(),
            &engine,
            &later.issuer_id,
            &later.session_id,
            "owner",
        )
        .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let timed = prepared.run(&engine).await;
        assert_eq!(timed.state, ReceiptState::Unknown);
        assert!(timed.confirms(&later));
        assert_eq!(
            ledger.receipt(&later.issuer_id, &later.grant_id).unwrap()["state"],
            "unknown"
        );
    }
    #[tokio::test]
    async fn saved_permission_revocation_rejects_prepared_work_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let policy: crate::NodePolicy = serde_json::from_value(
            json!({"filesystem":{"write":true},"full_access":["filesystem.write"]}),
        )
        .unwrap();
        std::fs::write(
            &config,
            serde_json::to_vec(&json!({"node_id":"owner","permissions":policy})).unwrap(),
        )
        .unwrap();
        let service = crate::NodeService::new(config, "owner".into());
        let engine = engine(dir.path())
            .with_policy(policy)
            .unwrap()
            .with_service(service.clone());
        let ledger =
            GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner")
                .unwrap();
        let mut g = grant();
        let out = dir.path().join("effect.txt");
        g.capability = "filesystem".into();
        g.action = "write".into();
        g.params = json!({"path":out,"content":"must not happen"});
        let prepared = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            "owner",
        )
        .unwrap();
        service
            .save_configuration(json!({"node_id":"owner","permissions":{}}))
            .unwrap();
        let report = prepared.run(&engine).await;
        assert_eq!(report.state, ReceiptState::Unknown);
        assert!(!out.exists());
        assert!(
            AcceptedGrant::prepare(
                grant(),
                ledger,
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner"
            )
            .is_err()
        ); // A different session is denied too.
    }
    #[test]
    fn corrupt_journal_is_not_recreated_and_canonical_digest_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        std::fs::write(&path, b"damaged private receipt file").unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"damaged private receipt file"
        );
        assert_eq!(
            definition_digest(&json!({"b":[{"z":1,"a":2}],"a":0})).unwrap(),
            definition_digest(&serde_json::from_str(r#"{"a":0,"b":[{"a":2,"z":1}]}"#).unwrap())
                .unwrap()
        );
        assert_ne!(
            journal_path(Path::new("node.json")),
            journal_path(Path::new("node.yaml"))
        );
    }
    #[test]
    fn local_receipt_inspection_never_creates_replays_or_exports_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let path = journal_path(&config);
        let service = crate::NodeService::new(config, "owner".into());
        let g = grant();
        let request = || ReceiptRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            grant_id: g.grant_id.clone(),
        };
        assert!(service.grant_receipt(request()).is_err());
        assert!(!path.exists());
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        ledger.claim(&g).unwrap();
        let before = std::fs::read(&path).unwrap();
        let report = service.grant_receipt(request()).unwrap();
        assert_eq!(report["receipt"]["state"], "accepted");
        assert_eq!(report["execution_requested"], false);
        assert_eq!(report["automatically_retried"], false);
        assert!(report.get("params").is_none());
        assert!(report["receipt"].get("request_context").is_none());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut wrong = request();
        wrong.expected_node_id = "other".into();
        assert!(service.grant_receipt(wrong).is_err());
        let mut future = request();
        future.schema_version = 2;
        assert!(service.grant_receipt(future).is_err());
        assert!(serde_json::from_value::<ReceiptRequest>(json!({"schema_version":1,"expected_node_id":"owner","grant_id":g.grant_id,"replay":true})).is_err());
    }
    #[test]
    fn existing_future_or_damaged_receipt_metadata_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        drop(GrantLedger::open(&path, Default::default(), "owner").unwrap());
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA user_version=1; DELETE FROM execution_receipt_owner;")
            .unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn final_reports_are_atomic_bounded_durable_and_explicitly_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let path = journal_path(&config);
        let service = crate::NodeService::new(config, "owner".into());
        let g = grant();
        let request = || ReceiptRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            grant_id: g.grant_id.clone(),
        };
        assert!(service.grant_result(request()).is_err());
        assert!(!path.exists());
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        ledger.claim(&g).unwrap();
        let pending = service.grant_result(request()).unwrap();
        assert_eq!(pending["result"]["availability"], "no_final_checkpoint");
        assert!(pending["result"].get("report").is_none());
        let good = GrantReport::new(
            &g,
            ReceiptState::Completed,
            json!({"content":"private completed result"}),
            None,
        );
        let mut bad = good.clone();
        bad.data = json!("x".repeat(g.limits.max_result_bytes + 1));
        assert!(ledger.finish(&g, &bad).is_err());
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        bad = good.clone();
        bad.grant_sha256 = "c".repeat(64);
        assert!(ledger.finish(&g, &bad).is_err());
        ledger.finish(&g, &good).unwrap();
        assert!(ledger.finish(&g, &good).is_err());
        assert!(ledger.claim(&g).is_err());
        // Routine metadata inspection still does not export output.
        assert!(
            !service
                .grant_receipt(request())
                .unwrap()
                .to_string()
                .contains("private completed result")
        );
        let before = std::fs::read(&path).unwrap();
        let result = service.grant_result(request()).unwrap();
        assert_eq!(result["result"]["report"], json!(good));
        assert_eq!(result["execution_requested"], false);
        assert_eq!(result["reconciled"], false);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(ledger);
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert_eq!(service.grant_result(request()).unwrap(), result);
        assert!(ledger.claim(&g).is_err());
        let db = Connection::open(&path).unwrap();
        let mut changed = json!(good);
        changed["data"] = json!({"content":"changed cached output"});
        db.execute(
            "UPDATE execution_receipts SET report=?1 WHERE grant_id=?2",
            params![changed.to_string(), g.grant_id],
        )
        .unwrap();
        assert!(
            service.grant_result(request()).is_err(),
            "valid JSON output changes must fail the retained checksum"
        );
        db.execute(
            "UPDATE execution_receipts SET report=?1 WHERE grant_id=?2",
            params![serde_json::to_string(&good).unwrap(), g.grant_id],
        )
        .unwrap();
        let mut bad = json!(good);
        bad["issuer_id"] = json!("d".repeat(64));
        db.execute(
            "UPDATE execution_receipts SET report=?1 WHERE grant_id=?2",
            params![bad.to_string(), g.grant_id],
        )
        .unwrap();
        assert!(service.grant_result(request()).is_err());
        let before = std::fs::read(&path).unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn accepted_report_reservations_deny_before_effects_and_release_after_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let policy = GrantPolicy {
            max_journal_bytes: 64 * 1024,
            max_result_bytes: 1024,
            ..Default::default()
        };
        let ledger = GrantLedger::open(&path, policy.clone(), "owner").unwrap();
        let independent = GrantLedger::open(&path, policy, "owner").unwrap();
        let mut g = grant();
        g.limits.max_result_bytes = 1024;
        ledger.claim(&g).unwrap();
        let mut other = grant();
        other.limits.max_result_bytes = 1024;
        assert_eq!(
            denial_code(&independent.claim(&other).unwrap_err()),
            "grant_receipt_quota_full"
        );
        assert!(
            independent
                .receipt(&other.issuer_id, &other.grant_id)
                .is_err()
        );
        ledger
            .finish(
                &g,
                &GrantReport::new(&g, ReceiptState::Completed, json!({"ok":true}), None),
            )
            .unwrap();
        independent.claim(&other).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() <= 64 * 1024);
    }

    #[tokio::test]
    async fn failed_final_checkpoint_keeps_consumption_uncertain_without_output_or_replay() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let path = journal_path(&config);
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let g = grant();
        let engine = engine(dir.path());
        let accepted = AcceptedGrant::prepare(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            "owner",
        )
        .unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TRIGGER refuse_completion BEFORE UPDATE ON execution_receipts BEGIN SELECT RAISE(ABORT,'fixture checkpoint failure'); END;").unwrap();
        let result = accepted.run(&engine).await;
        assert_eq!(result.state, ReceiptState::Unknown);
        assert_eq!(
            result.error_code.as_deref(),
            Some("completion_checkpoint_failed")
        );
        assert!(!result.persisted);
        assert!(result.data.is_null());
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        let service = crate::NodeService::new(config, "owner".into());
        let report = service
            .grant_result(ReceiptRequest {
                schema_version: 1,
                expected_node_id: "owner".into(),
                grant_id: g.grant_id.clone(),
            })
            .unwrap();
        assert_eq!(report["result"]["availability"], "no_final_checkpoint");
        assert!(report["result"].get("report").is_none());
        assert!(ledger.claim(&g).is_err());
    }

    #[test]
    fn legacy_journals_migrate_consumption_without_inventing_results_or_rewriting_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let g = grant();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE execution_receipt_owner(owner TEXT NOT NULL PRIMARY KEY,node_id TEXT NOT NULL); CREATE TABLE execution_receipts(issuer_id TEXT NOT NULL,job_id TEXT NOT NULL,task_id TEXT NOT NULL,attempt INTEGER NOT NULL,grant_id TEXT NOT NULL UNIQUE,grant_digest TEXT NOT NULL,state TEXT NOT NULL,accepted_ms INTEGER NOT NULL,updated_ms INTEGER NOT NULL,PRIMARY KEY(issuer_id,job_id,task_id,attempt)); PRAGMA user_version=1;").unwrap();
        db.execute(
            "INSERT INTO execution_receipt_owner VALUES('node','owner')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO execution_receipts VALUES(?1,?2,?3,1,?4,?5,'completed',1,2)",
            params![
                g.issuer_id,
                g.job_id,
                g.task_id,
                g.grant_id,
                definition_digest(&json!(g)).unwrap()
            ],
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let original = crate::request_result::read_result(&db, "owner", None, &g.grant_id).unwrap();
        assert_eq!(original.availability, "legacy_metadata_only");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        db.execute(
            "UPDATE execution_receipts SET grant_digest=?1",
            ["z".repeat(64)],
        )
        .unwrap();
        let corrupted = std::fs::read(&path).unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupted);
        db.execute(
            "UPDATE execution_receipts SET grant_digest=?1",
            [definition_digest(&json!(g)).unwrap()],
        )
        .unwrap();
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let migrated = crate::request_result::read_result(&db, "owner", None, &g.grant_id).unwrap();
        assert_eq!(json!(migrated), json!(original));
        assert_eq!(
            db.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
                .unwrap(),
            2
        );
        assert!(ledger.claim(&g).is_err());
        drop(ledger);
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_ok());
    }
}
