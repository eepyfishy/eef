//! Latest durable observation only: no output, resume cursor or execution authority.
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(crate) const MAX_CHECKPOINT_BYTES: usize = 2048;
pub(crate) const JOURNAL_RESERVATION: usize = 4096;

fn payload_digest(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Pending,
    Starting,
    Completed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineProgress {
    pub schema_version: u32,
    pub sequence: usize,
    pub phase: Phase,
    pub current_step: String,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub retained_encoded_bytes: usize,
    pub resolved_encoded_bytes: usize,
    pub recorded_at_ms: u64,
    pub durable: bool,
    pub termination_confirmed: bool,
}
impl PipelineProgress {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || !self.durable
            || self.termination_confirmed
            || !(1..=crate::pipeline::MAX_STEPS).contains(&self.total_steps)
            || self.completed_steps > self.total_steps
            || self.sequence > 2 * self.total_steps
            || crate::pipeline::step_id(&self.current_step).is_err()
            || self.retained_encoded_bytes > crate::pipeline::MAX_RETAINED_BYTES
            || self.resolved_encoded_bytes > crate::pipeline::MAX_RETAINED_BYTES
            || self.recorded_at_ms == 0
            || self.recorded_at_ms > i64::MAX as u64
        {
            bail!("Invalid pipeline observation")
        }
        let valid = match self.phase {
            Phase::Pending => {
                self.sequence == 0
                    && self.completed_steps == 0
                    && self.retained_encoded_bytes == 0
                    && self.resolved_encoded_bytes == 0
            }
            Phase::Starting => {
                self.completed_steps < self.total_steps
                    && self.sequence == 2 * self.completed_steps + 1
            }
            Phase::Completed => {
                self.completed_steps > 0
                    && self.sequence == 2 * self.completed_steps
                    && self.resolved_encoded_bytes == 0
            }
            Phase::Failed => {
                self.completed_steps < self.total_steps
                    && self.sequence == 2 * (self.completed_steps + 1)
                    && self.resolved_encoded_bytes == 0
            }
        };
        if !valid {
            bail!("Inconsistent pipeline observation")
        }
        Ok(())
    }
    pub fn confirms(&self, grant: &crate::grants::ExecutionGrant) -> bool {
        let Ok(definition) = crate::pipeline::FinitePipeline::from_grant(grant) else {
            return false;
        };
        if self.validate().is_err()
            || self.total_steps != definition.steps.len()
            || self.retained_encoded_bytes > definition.max_retained_bytes
            || self.resolved_encoded_bytes > definition.max_retained_bytes
        {
            return false;
        }
        let index = match self.phase {
            Phase::Completed => self.completed_steps - 1,
            _ => self.completed_steps,
        };
        let step = &definition.steps[index];
        self.current_step == step.id
            && (self.phase != Phase::Failed
                || (step.capability == "process.exec" && step.action == "run"))
    }
    pub(crate) fn checkpoint(
        grant: &crate::grants::ExecutionGrant,
        index: usize,
        phase: Phase,
        retained: usize,
        resolved: usize,
    ) -> Result<Self> {
        let definition = crate::pipeline::FinitePipeline::from_grant(grant)?;
        let step = definition
            .steps
            .get(index)
            .context("Pipeline observation step missing")?;
        let progress = Self {
            schema_version: 1,
            sequence: match phase {
                Phase::Pending => 0,
                Phase::Starting => 2 * index + 1,
                _ => 2 * index + 2,
            },
            phase,
            current_step: step.id.clone(),
            completed_steps: index + usize::from(phase == Phase::Completed),
            total_steps: definition.steps.len(),
            retained_encoded_bytes: retained,
            resolved_encoded_bytes: resolved,
            recorded_at_ms: crate::protocol::now_ms(),
            durable: true,
            termination_confirmed: false,
        };
        if !progress.confirms(grant) {
            bail!("Pipeline observation scope changed")
        }
        Ok(progress)
    }
}

/// An independently versioned adjunct preserves the root journal's v1/v2 read
/// compatibility. Missing legacy observations remain missing, never zero progress.
pub(crate) fn available(db: &Connection) -> Result<bool> {
    let objects: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('execution_pipeline_progress','execution_pipeline_progress_meta')", [], |r| r.get(0))?;
    let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('execution_pipeline_progress','execution_pipeline_progress_meta')", [], |r| r.get(0))?;
    if objects == 0 {
        return Ok(false);
    }
    if tables != 2 || objects != 2 {
        bail!("Incomplete pipeline observation schema; retained without repair")
    }
    let versions = db
        .prepare("SELECT schema_version FROM execution_pipeline_progress_meta")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if versions != [1] {
        bail!("Unsupported pipeline observation schema; retained without repair")
    }
    db.prepare("SELECT grant_id,grant_digest,payload,payload_sha256 FROM execution_pipeline_progress LIMIT 0")?;
    Ok(true)
}
/// Creation must share the caller's claim transaction, never a separate commit.
pub(crate) fn initialize(db: &Connection) -> Result<()> {
    if available(db)? {
        let duplicates: i64 = db.query_row("SELECT count(*) FROM (SELECT grant_id FROM execution_pipeline_progress GROUP BY grant_id HAVING count(*)!=1)", [], |r| r.get(0))?;
        let invalid: i64 = db.query_row("SELECT count(*) FROM execution_pipeline_progress p LEFT JOIN execution_receipts r ON r.grant_id=p.grant_id WHERE r.grant_id IS NULL OR p.grant_digest!=r.grant_digest", [], |r| r.get(0))?;
        if invalid != 0 || duplicates != 0 {
            bail!("Pipeline observation ownership changed")
        }
        return Ok(());
    }
    if db.is_autocommit() {
        bail!("Pipeline observation creation requires the claim transaction")
    }
    db.execute_batch("CREATE TABLE execution_pipeline_progress_meta(schema_version INTEGER NOT NULL PRIMARY KEY); INSERT INTO execution_pipeline_progress_meta VALUES(1); CREATE TABLE execution_pipeline_progress(grant_id TEXT NOT NULL PRIMARY KEY,grant_digest TEXT NOT NULL,payload TEXT NOT NULL,payload_sha256 TEXT NOT NULL);")?;
    Ok(())
}
pub(crate) fn initial(
    db: &Connection,
    grant: &crate::grants::ExecutionGrant,
    accepted_at_ms: u64,
) -> Result<()> {
    if grant.capability != "pipeline.run" {
        return Ok(());
    }
    if !available(db)? {
        bail!("Pipeline observation journal unavailable")
    }
    let mut checkpoint = PipelineProgress::checkpoint(grant, 0, Phase::Pending, 0, 0)?;
    checkpoint.recorded_at_ms = accepted_at_ms;
    let payload = serde_json::to_string(&checkpoint)?;
    if payload.len() > MAX_CHECKPOINT_BYTES {
        bail!("Pipeline observation exceeds its reservation")
    }
    db.execute(
        "INSERT INTO execution_pipeline_progress VALUES(?1,?2,?3,?4)",
        params![
            grant.grant_id,
            crate::grants::definition_digest(&json!(grant))?,
            payload,
            payload_digest(&payload)
        ],
    )?;
    Ok(())
}
pub(crate) fn update(
    db: &mut Connection,
    node: &str,
    grant: &crate::grants::ExecutionGrant,
    checkpoint: PipelineProgress,
) -> Result<PipelineProgress> {
    if !checkpoint.confirms(grant) || checkpoint.phase == Phase::Pending {
        bail!("Invalid pipeline observation transition")
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let receipt =
        crate::request_receipt::read_consumed(&tx, node, Some(&grant.issuer_id), &grant.grant_id)?;
    if !receipt.confirms(grant) || receipt.state != "accepted" {
        bail!("Pipeline receipt is no longer accepted")
    }
    let previous = receipt
        .pipeline_progress
        .context("Initial pipeline observation unavailable")?;
    if previous.phase == Phase::Failed
        || (previous.phase == Phase::Completed && previous.completed_steps == previous.total_steps)
        || checkpoint.sequence != previous.sequence + 1
        || checkpoint.recorded_at_ms < receipt.updated_at_ms
    {
        bail!("Pipeline observation sequence or clock changed")
    }
    let payload = serde_json::to_string(&checkpoint)?;
    if payload.len() > MAX_CHECKPOINT_BYTES {
        bail!("Pipeline observation exceeds its reservation")
    }
    let rows = tx.execute(
        "UPDATE execution_pipeline_progress SET payload=?3,payload_sha256=?4 WHERE grant_id=?1 AND grant_digest=?2",
        params![grant.grant_id, receipt.grant_sha256, payload, payload_digest(&payload)],
    )?;
    if rows != 1 {
        bail!("Pipeline observation ownership changed")
    }
    let rows = tx.execute("UPDATE execution_receipts SET updated_ms=?3 WHERE grant_id=?1 AND grant_digest=?2 AND state='accepted'", params![grant.grant_id,receipt.grant_sha256,i64::try_from(checkpoint.recorded_at_ms)?])?;
    if rows != 1 {
        bail!("Pipeline receipt changed")
    }
    tx.commit()?;
    Ok(checkpoint)
}

/// SQL JOIN projection keeps receipt state/time and progress in one read snapshot.
pub(crate) fn projection(db: &Connection) -> Result<(&'static str, &'static str)> {
    if available(db)? {
        Ok((
            "p.grant_digest, CASE WHEN length(CAST(p.payload AS BLOB))<=2048 THEN p.payload ELSE NULL END, p.payload_sha256",
            " LEFT JOIN execution_pipeline_progress p ON p.grant_id=execution_receipts.grant_id",
        ))
    } else {
        Ok(("NULL,NULL,NULL", ""))
    }
}
pub(crate) fn decode(
    digest: Option<String>,
    payload: Option<String>,
    checksum: Option<String>,
    expected: &str,
) -> Result<Option<PipelineProgress>> {
    match (digest, payload, checksum) {
        (None, None, None) => Ok(None),
        (Some(digest), Some(payload), Some(checksum))
            if digest == expected
                && payload.len() <= MAX_CHECKPOINT_BYTES
                && checksum == payload_digest(&payload) =>
        {
            let checkpoint: PipelineProgress = serde_json::from_str(&payload)?;
            checkpoint.validate()?;
            Ok(Some(checkpoint))
        }
        _ => bail!("Damaged pipeline observation; retained without repair"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::{ExecutionGrant, GrantLedger};

    fn grant() -> ExecutionGrant {
        let mut grant = crate::grants::tests::grant();
        grant.capability = "pipeline.run".into();
        grant.params = json!({"pipeline":{"schema_version":1,"max_retained_bytes":1024,"steps":[
            {"id":"first","capability":"system.info","action":"run","params":{}},
            {"id":"last","capability":"system.info","action":"run","params":{}}
        ]}});
        grant
    }
    fn read(ledger: &GrantLedger, g: &ExecutionGrant) -> crate::request_receipt::ConsumedReceipt {
        serde_json::from_value(ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()).unwrap()
    }
    fn save(
        ledger: &GrantLedger,
        g: &ExecutionGrant,
        index: usize,
        phase: Phase,
    ) -> PipelineProgress {
        ledger
            .record_pipeline_progress(
                g,
                PipelineProgress::checkpoint(g, index, phase, 0, 0).unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn claim_and_latest_progress_are_durable_bounded_and_not_resume_authority() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let g = grant();
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        ledger.claim(&g).unwrap();
        let initial = read(&ledger, &g);
        assert_eq!(
            initial.pipeline_progress.as_ref().unwrap().phase,
            Phase::Pending
        );
        assert_eq!(
            initial.pipeline_progress.as_ref().unwrap().recorded_at_ms,
            initial.accepted_at_ms
        );
        assert!(initial.confirms(&g));
        assert!(ledger.receipt(&"c".repeat(64), &g.grant_id).is_err());
        let last = save(&ledger, &g, 0, Phase::Starting);
        assert_eq!(last.sequence, 1);
        assert!(!last.termination_confirmed);
        drop(ledger);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let retained = read(&reopened, &g);
        assert_eq!(
            retained.state, "accepted",
            "observation cannot invent a terminal outcome"
        );
        assert_eq!(retained.pipeline_progress, Some(last));
        assert!(
            reopened.claim(&g).is_err(),
            "restart cannot replay a consumed parent"
        );
        let db = Connection::open(&path).unwrap();
        let (count, size): (i64, i64) = db.query_row("SELECT count(*),max(length(CAST(payload AS BLOB))) FROM execution_pipeline_progress", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(count, 1);
        assert!(size <= MAX_CHECKPOINT_BYTES as i64);
    }

    #[test]
    fn transitions_are_exact_and_terminal_progress_is_not_a_terminal_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let ledger =
            GrantLedger::open(&dir.path().join("r.db"), Default::default(), "owner").unwrap();
        let g = grant();
        ledger.claim(&g).unwrap();
        assert!(
            ledger
                .record_pipeline_progress(
                    &g,
                    PipelineProgress::checkpoint(&g, 0, Phase::Completed, 0, 0).unwrap()
                )
                .is_err()
        );
        let first = save(&ledger, &g, 0, Phase::Starting);
        assert!(ledger.record_pipeline_progress(&g, first).is_err());
        save(&ledger, &g, 0, Phase::Completed);
        save(&ledger, &g, 1, Phase::Starting);
        let last = save(&ledger, &g, 1, Phase::Completed);
        assert_eq!(last.sequence, 4);
        assert_eq!(last.completed_steps, 2);
        assert_eq!(read(&ledger, &g).state, "accepted");
        assert!(ledger.record_pipeline_progress(&g, last).is_err());
        assert!(ledger.claim(&g).is_err());
    }

    #[test]
    fn checkpoint_scope_clock_and_authority_changes_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ledger =
            GrantLedger::open(&dir.path().join("r.db"), Default::default(), "owner").unwrap();
        let g = grant();
        ledger.claim(&g).unwrap();
        let starting = PipelineProgress::checkpoint(&g, 0, Phase::Starting, 0, 0).unwrap();
        for change in [
            |p: &mut PipelineProgress| p.current_step = "last".into(),
            |p: &mut PipelineProgress| p.total_steps = 1,
            |p: &mut PipelineProgress| p.termination_confirmed = true,
            |p: &mut PipelineProgress| p.durable = false,
            |p: &mut PipelineProgress| p.recorded_at_ms = 1,
            |p: &mut PipelineProgress| p.retained_encoded_bytes = 1025,
            |p: &mut PipelineProgress| p.resolved_encoded_bytes = 1025,
        ] {
            let mut changed = starting.clone();
            change(&mut changed);
            assert!(ledger.record_pipeline_progress(&g, changed).is_err());
        }
        let mut wrong = g.clone();
        wrong.issuer_id = "c".repeat(64);
        assert!(
            ledger
                .record_pipeline_progress(&wrong, starting.clone())
                .is_err()
        );
        wrong = g.clone();
        wrong.params["pipeline"]["max_retained_bytes"] = json!(512);
        assert!(ledger.record_pipeline_progress(&wrong, starting).is_err());
        assert_eq!(read(&ledger, &g).pipeline_progress.unwrap().sequence, 0);
    }

    #[test]
    fn failed_native_stage_is_observed_without_completing_or_advancing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let ledger =
            GrantLedger::open(&dir.path().join("r.db"), Default::default(), "owner").unwrap();
        let mut g = grant();
        g.params["pipeline"]["steps"][0] = json!({"id":"native","capability":"process.exec","action":"run","params":{"program":"fixture","args":[]}});
        ledger.claim(&g).unwrap();
        save(&ledger, &g, 0, Phase::Starting);
        let failure = save(&ledger, &g, 0, Phase::Failed);
        assert_eq!(failure.completed_steps, 0);
        assert!(failure.confirms(&g));
        assert!(
            ledger
                .record_pipeline_progress(
                    &g,
                    PipelineProgress::checkpoint(&g, 1, Phase::Starting, 0, 0).unwrap()
                )
                .is_err()
        );
        assert_eq!(read(&ledger, &g).state, "accepted");
    }

    #[test]
    fn initial_checkpoint_failure_rolls_back_consumption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db");
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        initialize(&db).unwrap();
        db.execute_batch("COMMIT").unwrap();
        db.execute_batch("CREATE TRIGGER fail_progress BEFORE INSERT ON execution_pipeline_progress BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let g = grant();
        assert!(ledger.claim(&g).is_err());
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
                .unwrap(),
            0
        );
        db.execute_batch("DROP TRIGGER fail_progress").unwrap();
        ledger.claim(&g).unwrap();
    }

    #[test]
    fn legacy_missing_progress_remains_unknown_and_inspection_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.db");
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let g = grant();
        ledger.claim(&g).unwrap();
        drop(ledger);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "DROP TABLE execution_pipeline_progress; DROP TABLE execution_pipeline_progress_meta;",
        )
        .unwrap();
        assert!(
            crate::request_receipt::read_consumed(&db, "owner", Some(&g.issuer_id), &g.grant_id)
                .unwrap()
                .pipeline_progress
                .is_none()
        );
        assert!(!available(&db).unwrap());
        drop(db);
        let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        assert!(read(&reopened, &g).pipeline_progress.is_none());
        assert!(
            reopened
                .record_pipeline_progress(
                    &g,
                    PipelineProgress::checkpoint(&g, 0, Phase::Starting, 0, 0).unwrap()
                )
                .is_err()
        );
        assert!(reopened.claim(&g).is_err());
    }

    #[test]
    fn corrupt_oversized_or_substituted_payload_is_not_repaired_or_exposed() {
        for damage in ["checksum", "payload", "oversize", "scope", "future"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("r.db");
            let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
            let g = grant();
            ledger.claim(&g).unwrap();
            let db = Connection::open(&path).unwrap();
            match damage {
                "checksum" => {
                    db.execute(
                        "UPDATE execution_pipeline_progress SET payload_sha256='bad'",
                        [],
                    )
                    .unwrap();
                }
                "payload" => {
                    db.execute("UPDATE execution_pipeline_progress SET payload='{}'", [])
                        .unwrap();
                }
                "oversize" => {
                    let raw = "x".repeat(MAX_CHECKPOINT_BYTES + 1);
                    db.execute(
                        "UPDATE execution_pipeline_progress SET payload=?1,payload_sha256=?2",
                        params![raw, payload_digest(&raw)],
                    )
                    .unwrap();
                }
                "scope" => {
                    db.execute(
                        "UPDATE execution_pipeline_progress SET grant_digest=?1",
                        ["d".repeat(64)],
                    )
                    .unwrap();
                }
                "future" => {
                    db.execute(
                        "UPDATE execution_pipeline_progress_meta SET schema_version=2",
                        [],
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            let before: String = db
                .query_row("SELECT payload FROM execution_pipeline_progress", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert!(
                ledger.receipt(&g.issuer_id, &g.grant_id).is_err(),
                "{damage}"
            );
            drop(ledger);
            assert!(
                GrantLedger::open(&path, Default::default(), "owner").is_err(),
                "{damage}"
            );
            assert_eq!(
                db.query_row::<String, _, _>(
                    "SELECT payload FROM execution_pipeline_progress",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
                before
            );
        }
    }

    #[test]
    fn partial_and_disguised_schema_is_rejected_without_repair() {
        for schema in [
            "CREATE TABLE execution_pipeline_progress_meta(schema_version INTEGER); INSERT INTO execution_pipeline_progress_meta VALUES(1);",
            "CREATE VIEW execution_pipeline_progress AS SELECT 1 AS grant_id;",
            "CREATE TABLE execution_pipeline_progress_meta(schema_version INTEGER); INSERT INTO execution_pipeline_progress_meta VALUES(1); CREATE TABLE execution_pipeline_progress(grant_id TEXT);",
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch(schema).unwrap();
            assert!(available(&db).is_err());
            assert!(initialize(&mut db).is_err());
        }
    }
}
