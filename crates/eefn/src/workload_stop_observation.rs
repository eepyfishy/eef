//! Optional historical owned-process exit alongside a continuous workload's
//! unknown receipt. Normal completion and the v1 stop intent stay independent.
use super::*;
use crate::process_stop::Observation;

const MAX_REPORT_BYTES: i64 = 512;
const TABLE: &str = "execution_workload_stop_exits";

fn checksum(scope: &stop::Scope, observation: &Observation) -> Result<String> {
    crate::grants::definition_digest(&json!({"scope":scope,"observation":observation}))
}

fn confirms_stop(db: &Connection, scope: &stop::Scope, observation: &Observation) -> Result<bool> {
    observation.validate()?;
    Ok(stop::read(db, &scope.grant_id)?.is_some_and(|report| {
        report.confirms_scope(scope)
            && report.receipt_preexisted
            && report.recorded_at_ms <= observation.observed_at_ms
    }))
}

/// Called in the same transaction that retires the exact accepted workload.
/// The workload's existing retained-report reservation covers this small row.
pub(super) fn save(db: &Connection, scope: &stop::Scope, observation: &Observation) -> Result<()> {
    if !confirms_stop(db, scope, observation)? {
        // An unavailable clock ordering or missing exact intent cannot prove
        // post-stop exit. Preserve the ordinary unknown checkpoint.
        return Ok(());
    }
    let report = serde_json::to_string(observation)?;
    ensure!(
        report.len() as i64 <= MAX_REPORT_BYTES,
        "Workload stop observation exceeds its bound"
    );
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS execution_workload_stop_exits(
            grant_id TEXT NOT NULL PRIMARY KEY,
            report TEXT NOT NULL,
            report_sha256 TEXT NOT NULL
        ) WITHOUT ROWID",
    )?;
    db.execute(
        "INSERT INTO execution_workload_stop_exits(grant_id,report,report_sha256) VALUES(?1,?2,?3)",
        params![scope.grant_id, report, checksum(scope, observation)?],
    )?;
    Ok(())
}

fn available(db: &Connection) -> Result<bool> {
    let available: Option<String> = db
        .query_row(
            "SELECT type FROM sqlite_schema WHERE name=?1",
            [TABLE],
            |row| row.get(0),
        )
        .optional()?;
    ensure!(
        available.as_deref().is_none_or(|kind| kind == "table"),
        "Invalid workload stop observation store"
    );
    Ok(available.is_some())
}

pub(super) fn audit_orphans(db: &Connection) -> Result<()> {
    if available(db)? {
        let invalid: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM execution_workload_stop_exits e
             LEFT JOIN execution_receipts r ON r.grant_id=e.grant_id
             WHERE r.grant_id IS NULL OR r.receipt_kind!='workload' OR r.state!='unknown')",
            [],
            |row| row.get(0),
        )?;
        ensure!(
            !invalid,
            "Unbound workload stop observation; retained without repair"
        );
    }
    Ok(())
}

pub(super) fn read(db: &Connection, receipt: &Receipt) -> Result<Option<Observation>> {
    if !available(db)? {
        return Ok(None);
    }
    let scope = receipt.definition.scope()?;
    let saved: Option<(Option<String>, Option<String>)> = db
        .query_row(
            "SELECT CASE WHEN length(CAST(report AS BLOB))<=?2 THEN report END,
                CASE WHEN length(CAST(report_sha256 AS BLOB))=64 THEN report_sha256 END
         FROM execution_workload_stop_exits WHERE grant_id=?1",
            params![scope.grant_id, MAX_REPORT_BYTES],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((report, digest)) = saved else {
        return Ok(None);
    };
    let report = report.context("Oversized or missing workload stop observation")?;
    let observation: Observation = serde_json::from_str(&report)?;
    ensure!(
        receipt.receipt.state == "unknown"
            && confirms_stop(db, &scope, &observation)?
            && digest.as_deref() == Some(checksum(&scope, &observation)?.as_str())
            && serde_json::to_string(&observation)? == report,
        "Unconfirmed retained workload stop observation"
    );
    Ok(Some(observation))
}
