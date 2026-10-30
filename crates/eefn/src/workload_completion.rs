//! Historical completion in the existing journal. No source, lease, renewal,
//! execution or recovery authority enters the final worker.
use super::*;
use anyhow::ensure;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub grant_sha256: String,
    pub adapter_sha256: String,
    pub source_observations: u64,
    pub source_frames: u64,
    pub event_count: u64,
    pub event_bytes: usize,
    pub event_prefix_sha256: String,
    pub lease_sequence: u64,
    pub recorded_at_ms: u64,
    pub owned_process_exit_observed: bool,
    pub physical_effects_stopped: bool,
}
impl Report {
    pub(super) fn validate(&self, receipt: &Receipt) -> Result<()> {
        let crate::workload_leases::definition::Source::MicrophoneEnergy {
            parameters,
            adapter_sha256,
            ..
        } = &receipt.definition.source;
        ensure!(
            self.schema_version == 1
                && self.grant_sha256 == receipt.definition.scope()?.grant_sha256
                && self.adapter_sha256 == *adapter_sha256
                && self.source_observations == u64::from(parameters.max_blocks)
                && self.source_frames
                    == self.source_observations * u64::from(parameters.block_frames)
                && self.source_frames <= receipt.definition.limits.max_source_frames
                && self.event_count > 0
                && self.event_count <= receipt.definition.events.max_events
                && self.event_bytes <= receipt.definition.events.max_total_bytes
                && self.event_bytes >= usize::try_from(self.event_count)?.saturating_mul(128)
                && crate::request_result::valid_digest(&self.event_prefix_sha256)
                && self.lease_sequence > 0
                && self.owned_process_exit_observed
                && !self.physical_effects_stopped
                && self.recorded_at_ms >= receipt.receipt.accepted_at_ms
                && self.recorded_at_ms <= receipt.receipt.updated_at_ms,
            "Damaged workload completion evidence; retained without repair"
        );
        Ok(())
    }
}

// Constructible only from the private prepared result and a supervisor's local
// finish boundary. It contains bounded metadata; the owned OS witness is dropped
// before this payload is submitted. Neither component can be deserialized.
pub(super) struct Evidence {
    contract: Contract,
    boundary: crate::workload_leases::FinishBoundary,
    adapter_sha256: String,
    observations: u64,
    frames: u64,
    events: crate::workload_events::EventSeal,
}
impl Evidence {
    pub(super) fn from_observed(
        contract: Contract,
        boundary: crate::workload_leases::FinishBoundary,
        observed: crate::workload_runtime::ObservedWorkload,
    ) -> Result<Self> {
        boundary.validate(&contract)?;
        let (source, events) = observed.into_parts();
        ensure!(
            source.summary().stream_id == contract.scope.grant_id,
            "Observed source belongs to another consumed workload"
        );
        Ok(Self {
            contract,
            boundary,
            adapter_sha256: source.source_digest().into(),
            observations: source.summary().observations,
            frames: source.summary().source_frames,
            events,
        })
    }
}

pub(super) fn decode(db: &Connection, id: &str, state: &str) -> Result<Option<Report>> {
    let (size, raw, checksum): (Option<i64>, Option<String>, Option<String>) = db.query_row(
        "SELECT length(CAST(report AS BLOB)),CASE WHEN length(CAST(report AS BLOB))<=?2 THEN report ELSE NULL END,CASE WHEN length(CAST(report_sha256 AS BLOB))=64 THEN report_sha256 ELSE NULL END FROM execution_receipts WHERE grant_id=?1",
        params![id, i64::try_from(REPORT_HEADROOM)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let has_checksum: bool = db.query_row(
        "SELECT report_sha256 IS NOT NULL FROM execution_receipts WHERE grant_id=?1",
        [id],
        |r| r.get(0),
    )?;
    if state != "completed" {
        ensure!(
            size.is_none() && !has_checksum,
            "Unexpected workload final report"
        );
        return Ok(None);
    }
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        version == 5 && size.is_some_and(|size| (1..=REPORT_HEADROOM as i64).contains(&size)),
        "Missing, oversized or downgraded workload completion report"
    );
    let raw = raw.context("Workload completion unavailable")?;
    let report: Report = serde_json::from_str(&raw)?;
    ensure!(
        serde_json::to_string(&report)? == raw
            && checksum.as_deref() == Some(hex::encode(Sha256::digest(raw.as_bytes())).as_str()),
        "Workload completion checksum or canonical payload changed"
    );
    Ok(Some(report))
}
pub(super) fn audit_prefix(db: &Connection, receipt: &Receipt) -> Result<()> {
    if let Some(report) = &receipt.completion {
        let (count, bytes, prefix, last, sequence) = event_journal::completion_prefix(db, receipt)?;
        ensure!(
            report.event_count == count
                && report.event_bytes == bytes
                && report.event_prefix_sha256 == prefix
                && last.source_sequence <= report.source_observations
                && last.source_frames <= report.source_frames
                && sequence <= report.lease_sequence,
            "Workload completion differs from its retained event prefix"
        );
    }
    Ok(())
}
fn upgrade(db: &Connection, node: &str) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        matches!(version, 4 | 5),
        "Completion requires the current event journal"
    );
    if version == 5 {
        return Ok(());
    }
    // Validate all retained kinds before changing the marker. No row is repaired,
    // rewritten, replayed or assigned invented completion evidence.
    let ids = db
        .prepare("SELECT grant_id FROM execution_receipts")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        if super::is_workload(db, &id)? {
            super::read(db, node, None, &id)?;
        } else {
            crate::request_result::read_in_snapshot(db, node, None, &id)?;
        }
    }
    stop::validate_all(db, node)?;
    db.execute_batch("PRAGMA user_version=5")?;
    super::validate_schema(db, 5)
}
fn uncertain(db: &Connection, scope: &stop::Scope) -> Result<()> {
    db.execute("UPDATE execution_receipts SET state='unknown',updated_ms=max(updated_ms,accepted_ms,?3) WHERE grant_id=?1 AND grant_digest=?2 AND state='accepted' AND receipt_kind='workload'",
        params![scope.grant_id,scope.grant_sha256,i64::try_from(crate::protocol::now_ms().max(1))?])?;
    Ok(())
}
// Inner refusal is a confirmed uncertain outcome (for example, stop intent), not
// failed storage. Actual storage/corruption errors remain outer errors and latch
// new workload admission denial in the common final worker.
pub(super) fn save(
    ledger: &GrantLedger,
    evidence: Evidence,
    stopped: tokio::sync::watch::Receiver<bool>,
) -> Result<Result<Receipt>> {
    evidence.boundary.validate(&evidence.contract)?;
    let scope = &evidence.contract.scope;
    let mut db = ledger
        .connection
        .lock()
        .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut receipt = super::read(
        &tx,
        &ledger.node_id,
        Some(&scope.issuer_id),
        &scope.grant_id,
    )?;
    ensure!(
        receipt.definition.scope()? == *scope,
        "Finished workload scope changed"
    );
    if receipt.receipt.state != "accepted" || *stopped.borrow() || stop::recorded_scope(&tx, scope)?
    {
        uncertain(&tx, scope)?;
        tx.commit()?;
        return Ok(Err(anyhow::anyhow!(
            "Workload completion refused; inspect the retained receipt"
        )));
    }
    let crate::workload_leases::definition::Source::MicrophoneEnergy {
        parameters,
        adapter_sha256,
        ..
    } = &receipt.definition.source;
    ensure!(
        evidence.adapter_sha256 == *adapter_sha256
            && evidence.observations == u64::from(parameters.max_blocks)
            && evidence.frames == evidence.observations * u64::from(parameters.block_frames),
        "Observed workload source differs from its consumed definition"
    );
    let (count, bytes, prefix, last, sequence) = event_journal::completion_prefix(&tx, &receipt)?;
    ensure!(
        evidence.events.confirms(count, bytes, &last) && sequence <= evidence.boundary.sequence(),
        "Finished event prefix differs from filtered source"
    );
    upgrade(&tx, &ledger.node_id)?;
    let recorded = crate::protocol::now_ms().max(receipt.receipt.updated_at_ms);
    receipt.schema_version = 3;
    receipt.receipt.state = "completed".into();
    receipt.receipt.updated_at_ms = recorded;
    receipt.completion = Some(Report {
        schema_version: 1,
        grant_sha256: scope.grant_sha256.clone(),
        adapter_sha256: evidence.adapter_sha256,
        source_observations: evidence.observations,
        source_frames: evidence.frames,
        event_count: count,
        event_bytes: bytes,
        event_prefix_sha256: prefix,
        lease_sequence: evidence.boundary.sequence(),
        recorded_at_ms: recorded,
        owned_process_exit_observed: true,
        physical_effects_stopped: false,
    });
    receipt.validate()?;
    let raw = serde_json::to_string(receipt.completion.as_ref().unwrap())?;
    ensure!(
        raw.len() <= REPORT_HEADROOM,
        "Workload completion exceeds reserved report headroom"
    );
    if *stopped.borrow() || stop::recorded_scope(&tx, scope)? {
        uncertain(&tx, scope)?;
        tx.commit()?;
        return Ok(Err(anyhow::anyhow!(
            "Workload stopped before completion commit; consumption remains uncertain"
        )));
    }
    let changed = tx.execute("UPDATE execution_receipts SET state='completed',updated_ms=?3,report=?4,report_sha256=?5 WHERE grant_id=?1 AND grant_digest=?2 AND state='accepted' AND receipt_kind='workload' AND report IS NULL AND report_sha256 IS NULL",
        params![scope.grant_id,scope.grant_sha256,i64::try_from(recorded)?,raw,hex::encode(Sha256::digest(raw.as_bytes()))])?;
    ensure!(
        changed == 1,
        "Workload completion consumption changed before commit"
    );
    tx.commit()?;
    // A raced stop cannot acknowledge success. The durable record, if committed,
    // remains historical evidence of the source that had already exited.
    if *stopped.borrow() || stop::recorded_scope(&db, scope)? {
        return Ok(Err(anyhow::anyhow!(
            "Stop raced with completion; inspect the retained receipt"
        )));
    }
    Ok(Ok(receipt))
}

#[cfg(test)]
#[path = "workload_completion_tests.rs"]
mod tests;
