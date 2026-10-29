//! Exact pinned-session stop intent in the existing consumption journal.
//! A durable stop receipt never claims OS/device/external effects have stopped.
use super::*;
use std::collections::{HashMap, HashSet};
use tokio::sync::watch;
#[path = "grant_stop_read.rs"]
pub mod inspection;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub guarantee: String,
}
impl Advertisement {
    pub fn new(available: bool) -> Self {
        Self {
            schema_version: 1,
            available,
            guarantee: "durable_intent_only".into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.guarantee != "durable_intent_only" {
            bail!("Unsupported grant-stop advertisement")
        }
        Ok(())
    }
}

/// Bounded control identity only; no prompts, arguments, model input or media.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub grant_id: String,
    pub grant_sha256: String,
    pub issuer_id: String,
    pub session_id: String,
    pub executor_node: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
}
impl Scope {
    pub fn from_grant(g: &ExecutionGrant) -> Result<Self> {
        g.validate()?;
        Ok(Self {
            grant_id: g.grant_id.clone(),
            grant_sha256: definition_digest(&json!(g))?,
            issuer_id: g.issuer_id.clone(),
            session_id: g.session_id.clone(),
            executor_node: g.executor_node.clone(),
            job_id: g.job_id.clone(),
            task_id: g.task_id.clone(),
            attempt: g.attempt,
        })
    }
    pub fn validate(&self) -> Result<()> {
        crate::automation_occurrence::uuid(&self.grant_id)?;
        crate::automation_occurrence::uuid(&self.session_id)?;
        crate::network::validate_remote_node_id(&self.executor_node)?;
        crate::request_drafts::validate_id(&self.job_id)?;
        crate::request_drafts::validate_id(&self.task_id)?;
        if !hex_digest(&self.grant_sha256)
            || !hex_digest(&self.issuer_id)
            || !(1..=11).contains(&self.attempt)
        {
            bail!("Invalid grant-stop control scope")
        }
        Ok(())
    }
    pub fn matches_session(&self, issuer: &str, session: &str, node: &str) -> bool {
        self.issuer_id == issuer && self.session_id == session && self.executor_node == node
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub scope: Scope,
}
impl WireRequest {
    pub fn validate(&self, issuer: &str, session: &str, node: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        self.scope.validate()?;
        if self.kind != "stop_grant" || !self.scope.matches_session(issuer, session, node) {
            bail!("Stop requires the original pinned issuer/session/node scope")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub grant_id: String,
    pub grant_sha256: String,
    pub issuer_id: String,
    pub session_id: String,
    pub executor_node: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub recorded_at_ms: u64,
    pub receipt_preexisted: bool,
    pub reused: bool,
    pub cancellation_signalled: bool,
    pub persisted: bool,
    pub stop_requested: bool,
    pub execution_stopped: bool,
    pub external_effects_stopped: bool,
    pub execution_authorized: bool,
    pub automatically_retried: bool,
}
impl Report {
    fn identity_valid(&self) -> bool {
        crate::job_commands::validate_operation_id(&self.grant_id).is_ok()
            && crate::automation_occurrence::uuid(&self.session_id).is_ok()
            && crate::network::validate_remote_node_id(&self.executor_node).is_ok()
            && crate::request_drafts::validate_id(&self.job_id).is_ok()
            && crate::request_drafts::validate_id(&self.task_id).is_ok()
            && hex_digest(&self.grant_sha256)
            && hex_digest(&self.issuer_id)
            && (1..=11).contains(&self.attempt)
            && self.recorded_at_ms > 0
            && self.recorded_at_ms <= i64::MAX as u64
    }
    pub fn confirms(&self, g: &ExecutionGrant) -> bool {
        Scope::from_grant(g).is_ok_and(|scope| self.confirms_scope(&scope))
    }
    pub fn confirms_scope(&self, g: &Scope) -> bool {
        self.identity_valid()
            && g.validate().is_ok()
            && self.schema_version == 1
            && self.report_type == "grant_stop_receipt"
            && self.grant_id == g.grant_id
            && self.issuer_id == g.issuer_id
            && self.session_id == g.session_id
            && self.executor_node == g.executor_node
            && self.job_id == g.job_id
            && self.task_id == g.task_id
            && self.attempt == g.attempt
            && g.grant_sha256 == self.grant_sha256
            && self.persisted
            && self.stop_requested
            && !self.execution_stopped
            && !self.external_effects_stopped
            && !self.execution_authorized
            && !self.automatically_retried
    }
}
#[derive(Default)]
pub(super) struct Registry(Mutex<HashMap<String, (String, watch::Sender<bool>)>>);
pub(super) struct Watch {
    pub(super) receiver: watch::Receiver<bool>,
    id: String,
    registry: Arc<Registry>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .expect("grant stop registry")
            .remove(&self.id);
    }
}
pub(super) fn available(db: &Connection) -> Result<bool> {
    let objects: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('execution_stop_meta','execution_stop_requests')", [], |r| r.get(0))?;
    if objects == 0 {
        return Ok(false);
    }
    let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('execution_stop_meta','execution_stop_requests')", [], |r| r.get(0))?;
    if objects != 2 || tables != 2 {
        bail!("Incomplete stop evidence schema; no repair or work admission")
    }
    let versions = db
        .prepare("SELECT schema_version FROM execution_stop_meta")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if versions != [1] {
        bail!("Unsupported stop evidence schema; retained without repair")
    }
    Ok(true)
}
fn initialize(db: &Connection) -> Result<()> {
    if !available(db)? {
        db.execute_batch("CREATE TABLE execution_stop_meta(schema_version INTEGER NOT NULL PRIMARY KEY); INSERT INTO execution_stop_meta VALUES(1); CREATE TABLE execution_stop_requests(grant_id TEXT NOT NULL PRIMARY KEY,grant_digest TEXT NOT NULL,issuer_id TEXT NOT NULL,session_id TEXT NOT NULL,node_id TEXT NOT NULL,job_id TEXT NOT NULL,task_id TEXT NOT NULL,attempt INTEGER NOT NULL,recorded_ms INTEGER NOT NULL,receipt_preexisted INTEGER NOT NULL);")?;
    }
    Ok(())
}
fn read(db: &Connection, id: &str) -> Result<Option<Report>> {
    if !available(db)? {
        return Ok(None);
    }
    let invalid:i64=db.query_row("SELECT count(*) FROM execution_stop_requests WHERE grant_id=?1 AND receipt_preexisted NOT IN (0,1)",[id],|r|r.get(0))?;
    if invalid != 0 {
        bail!("Noncanonical stop evidence flag")
    }
    // Validate cardinality/byte bounds before materializing corrupt journal text.
    let (count,size):(i64,Option<i64>)=db.query_row("SELECT count(*),max(length(CAST(grant_id AS BLOB))+length(CAST(grant_digest AS BLOB))+length(CAST(issuer_id AS BLOB))+length(CAST(session_id AS BLOB))+length(CAST(node_id AS BLOB))+length(CAST(job_id AS BLOB))+length(CAST(task_id AS BLOB))) FROM execution_stop_requests WHERE grant_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if count > 1 || size.is_some_and(|n| !(1..=4096).contains(&n)) {
        bail!("Damaged or oversized stop evidence")
    }
    Ok(db.query_row("SELECT grant_id,grant_digest,issuer_id,session_id,node_id,job_id,task_id,attempt,recorded_ms,receipt_preexisted FROM execution_stop_requests WHERE grant_id=?1", [id], |r| Ok(Report {
        schema_version:1,report_type:"grant_stop_receipt".into(),grant_id:r.get(0)?,grant_sha256:r.get(1)?,issuer_id:r.get(2)?,session_id:r.get(3)?,executor_node:r.get(4)?,job_id:r.get(5)?,task_id:r.get(6)?,attempt:r.get(7)?,recorded_at_ms:u64::try_from(r.get::<_,i64>(8)?).map_err(|_|rusqlite::Error::InvalidQuery)?,receipt_preexisted:r.get(9)?,reused:true,cancellation_signalled:false,persisted:true,stop_requested:true,execution_stopped:false,external_effects_stopped:false,execution_authorized:false,automatically_retried:false
    })).optional()?)
}
pub(super) fn recorded_scope(db: &Connection, g: &Scope) -> Result<bool> {
    let Some(report) = read(db, &g.grant_id)? else {
        return Ok(false);
    };
    if !report.confirms_scope(g) {
        bail!("Stop evidence does not match the exact grant")
    }
    Ok(true)
}
pub(super) fn validate_all(db: &Connection, node: &str) -> Result<()> {
    if !available(db)? {
        return Ok(());
    }
    let bad: i64 = db.query_row("SELECT count(*) FROM execution_stop_requests WHERE length(grant_id)!=36 OR length(grant_digest)!=64 OR length(issuer_id)!=64 OR length(session_id)!=36 OR length(node_id)>255 OR length(job_id)!=32 OR length(task_id)!=32 OR attempt<1 OR attempt>11 OR recorded_ms<1 OR receipt_preexisted NOT IN (0,1)", [], |r| r.get(0))?;
    if bad != 0 {
        bail!("Damaged stop evidence; retained without repair")
    }
    let ids = db
        .prepare("SELECT grant_id FROM execution_stop_requests")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    for id in ids {
        let report = read(db, &id)?.context("Stop evidence vanished")?;
        let receipt =
            crate::request_receipt::read_consumed(db, node, Some(&report.issuer_id), &id)?;
        if !seen.insert(id)
            || !report.identity_valid()
            || report.executor_node != node
            || receipt.grant_sha256 != report.grant_sha256
            || receipt.job_id != report.job_id
            || receipt.task_id != report.task_id
            || receipt.attempt != report.attempt
        {
            bail!("Stop evidence differs from consumption journal")
        }
    }
    Ok(())
}
impl GrantLedger {
    pub(super) fn watch_stop(&self, g: &ExecutionGrant) -> Result<Watch> {
        self.watch_stop_scope(&Scope::from_grant(g)?)
    }
    pub(super) fn watch_stop_scope(&self, g: &Scope) -> Result<Watch> {
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let stopped = recorded_scope(&db, g)?;
        let mut active = self.stops.0.lock().expect("grant stop registry");
        if active.contains_key(&g.grant_id) {
            bail!("Grant already has a stop watcher")
        }
        let (sender, receiver) = watch::channel(stopped);
        active.insert(g.grant_id.clone(), (g.grant_sha256.clone(), sender));
        Ok(Watch {
            receiver,
            id: g.grant_id.clone(),
            registry: self.stops.clone(),
        })
    }
    pub fn request_stop(
        &self,
        g: &ExecutionGrant,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Report> {
        self.request_stop_scope(&Scope::from_grant(g)?, issuer, session, node)
    }
    pub fn request_stop_scope(
        &self,
        g: &Scope,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Report> {
        self.request_stop_scope_after_signal(g, issuer, session, node, || {})
    }
    fn request_stop_scope_after_signal(
        &self,
        g: &Scope,
        issuer: &str,
        session: &str,
        node: &str,
        after_signal: impl FnOnce(),
    ) -> Result<Report> {
        g.validate()?;
        if node != self.node_id || !g.matches_session(issuer, session, node) {
            bail!("Stop scope differs from pinned session")
        }
        let digest = g.grant_sha256.clone();
        let mut db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut report = if let Some(report) = read(&tx, &g.grant_id)? {
            if !report.confirms_scope(g) {
                bail!("Stop conflicts with retained scope")
            }
            report
        } else {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT grant_digest FROM execution_receipts WHERE grant_id=?1",
                    [&g.grant_id],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.as_ref().is_some_and(|d| d != &digest) {
                bail!("Stop conflicts with consumed grant scope")
            }
            let receipt_preexisted = existing.is_some();
            let count: i64 =
                tx.query_row("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))?;
            let page_size =
                usize::try_from(tx.query_row::<i64, _, _>("PRAGMA page_size", [], |r| r.get(0))?)?;
            let pages =
                usize::try_from(tx.query_row::<i64, _, _>("PRAGMA page_count", [], |r| r.get(0))?)?;
            let reserved: i64 = tx.query_row("SELECT coalesce(sum(max_result_bytes+?1+4*?2),0) FROM execution_receipts WHERE state='accepted' AND max_result_bytes>0", params![i64::try_from(REPORT_HEADROOM)?,i64::try_from(page_size)?], |r| r.get(0))?;
            let progress: i64 = if crate::pipeline_progress::available(&tx)? {
                tx.query_row("SELECT count(*) FROM execution_pipeline_progress p JOIN execution_receipts r ON p.grant_id=r.grant_id WHERE r.state='accepted' AND r.max_result_bytes>0", [], |r| r.get(0))?
            } else {
                0
            };
            if (!receipt_preexisted && usize::try_from(count)? >= self.policy.max_receipts)
                || pages
                    .saturating_mul(page_size)
                    .saturating_add(usize::try_from(reserved)?)
                    .saturating_add(
                        usize::try_from(progress)?
                            .saturating_mul(crate::pipeline_progress::JOURNAL_RESERVATION),
                    )
                    .saturating_add(8 * page_size)
                    > self.policy.max_journal_bytes
            {
                bail!("Stop journal quota exhausted; no durable stop confirmation")
            }
            let now = crate::protocol::now_ms().max(1);
            if !receipt_preexisted {
                // Consume the exact authority as UNKNOWN before admission can win.
                // Zero reserved output is existing metadata-only evidence, never
                // a fabricated execution acceptance, negative-effect or success proof.
                let conflict: i64 = tx.query_row("SELECT count(*) FROM execution_receipts WHERE issuer_id=?1 AND job_id=?2 AND task_id=?3 AND attempt=?4", params![g.issuer_id,g.job_id,g.task_id,g.attempt], |r| r.get(0))?;
                if conflict != 0 {
                    bail!("Stop attempt slot belongs to another grant")
                }
                tx.execute("INSERT INTO execution_receipts(issuer_id,job_id,task_id,attempt,grant_id,grant_digest,state,accepted_ms,updated_ms,max_result_bytes,report,report_sha256) VALUES(?1,?2,?3,?4,?5,?6,'unknown',?7,?7,0,NULL,NULL)", params![g.issuer_id,g.job_id,g.task_id,g.attempt,g.grant_id,digest,i64::try_from(now)?])?;
            }
            initialize(&tx)?;
            tx.execute(
                "INSERT INTO execution_stop_requests VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    g.grant_id,
                    digest,
                    g.issuer_id,
                    g.session_id,
                    g.executor_node,
                    g.job_id,
                    g.task_id,
                    g.attempt,
                    i64::try_from(now)?,
                    receipt_preexisted
                ],
            )?;
            let mut report = read(&tx, &g.grant_id)?.context("Stop receipt unavailable")?;
            report.reused = false;
            report
        };
        tx.commit()?;
        // Keep the journal lock through signal/registration so a claim-then-watch
        // race cannot miss a durable stop. No operation is started by this path.
        if let Some((expected, sender)) = self
            .stops
            .0
            .lock()
            .expect("grant stop registry")
            .get(&g.grant_id)
        {
            if *expected != digest {
                bail!("Active stop scope differs")
            }
            report.cancellation_signalled = sender.receiver_count() > 0;
            sender.send_replace(true);
        }
        // The shipping caller always supplies a no-op. Tests observe cleanup
        // while this actual journal lock is still held, not with a fake sleep.
        after_signal();
        if !report.confirms_scope(g) {
            bail!("Stop receipt failed confirmation")
        }
        Ok(report)
    }
}

#[cfg(test)]
#[path = "grant_stop_tests.rs"]
mod tests;
