//! Continuous consumption in the existing node grant journal. No second job
//! store, source launch or restart authority. No command/network route calls the
//! mutator yet; authenticated admission and the coordinator journal remain open.
use super::*;
use crate::workload_leases::{Contract, LeaseProof, Policy, definition::Definition};

const MAX_DEFINITION_BYTES: usize = 64 * 1024;

pub(crate) fn validate_schema(db: &Connection, version: i64) -> Result<()> {
    let fields = db.prepare("SELECT name,type,\"notnull\",dflt_value FROM pragma_table_info('execution_receipts') WHERE name IN ('receipt_kind','workload_definition') ORDER BY name")?
        .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if version == 3 {
        if fields
            != [
                (
                    "receipt_kind".into(),
                    "TEXT".into(),
                    1,
                    Some("'operation'".into()),
                ),
                ("workload_definition".into(), "TEXT".into(), 0, None),
            ]
        {
            bail!("Incomplete workload receipt schema; retained without repair")
        }
    } else if !fields.is_empty() {
        bail!("Unversioned workload receipt columns; retained without migration")
    }
    Ok(())
}
pub(crate) fn is_workload(db: &Connection, id: &str) -> Result<bool> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    validate_schema(db, version)?;
    if version != 3 {
        return Ok(false);
    }
    let row: Option<(String,bool)> = db.query_row(
        "SELECT receipt_kind,workload_definition IS NULL FROM execution_receipts WHERE grant_id=?1",
        [id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).optional()?;
    match row {
        None => Ok(false),
        Some((kind, true)) if kind == "operation" => Ok(false),
        Some((kind, false)) if kind == "workload" => Ok(true),
        _ => bail!("Damaged receipt kind or definition; retained without repair"),
    }
}
fn upgrade(db: &Connection, node: &str) -> Result<()> {
    crate::request_receipt::validate_owner(db, node)?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 3 {
        return Ok(());
    }
    if version != 2 {
        bail!("Workload consumption requires the current grant journal")
    }
    // Called inside the same IMMEDIATE transaction as insertion. Validate old
    // evidence before ALTER, including writes made through an independent handle.
    let ids = db
        .prepare("SELECT grant_id FROM execution_receipts")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        crate::request_result::read_in_snapshot(db, node, None, &id)?;
    }
    stop::validate_all(db, node)?;
    db.execute_batch("ALTER TABLE execution_receipts ADD COLUMN receipt_kind TEXT NOT NULL DEFAULT 'operation' CHECK(receipt_kind IN ('operation','workload')); ALTER TABLE execution_receipts ADD COLUMN workload_definition TEXT; PRAGMA user_version=3;")?;
    validate_schema(db, 3)
}

/// Read-only consumed definition, not a running-source attestation, permission,
/// successful output, lease renewal or authorization to resume after restart.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema_version: u32,
    pub receipt: crate::request_receipt::ConsumedReceipt,
    pub definition: Definition,
    pub execution_authorized: bool,
    pub restart_authorized: bool,
}
impl Receipt {
    pub fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        let scope = self.definition.scope()?;
        if self.schema_version != 2
            || self.execution_authorized
            || self.restart_authorized
            || self.receipt.node_id != scope.executor_node
            || self.receipt.issuer_id != scope.issuer_id
            || self.receipt.grant_id != scope.grant_id
            || self.receipt.grant_sha256 != scope.grant_sha256
            || self.receipt.job_id != scope.job_id
            || self.receipt.task_id != scope.task_id
            || self.receipt.attempt != scope.attempt
            || self.receipt.pipeline_progress.is_some()
            || !matches!(self.receipt.state.as_str(), "accepted" | "unknown")
        {
            bail!("Damaged workload consumption evidence; no recovery or replay")
        }
        Ok(())
    }
}
pub(crate) fn read(db: &Connection, node: &str, issuer: Option<&str>, id: &str) -> Result<Receipt> {
    crate::request_receipt::validate_owner(db, node)?;
    if !is_workload(db, id)? {
        bail!("Typed workload receipt unavailable")
    }
    let receipt = crate::request_receipt::read_consumed(db, node, issuer, id)?;
    let (size,raw,limit,empty_report): (i64,Option<String>,i64,bool) = db.query_row(
        "SELECT length(CAST(workload_definition AS BLOB)),CASE WHEN length(CAST(workload_definition AS BLOB))<=?2 THEN workload_definition ELSE NULL END,max_result_bytes,report IS NULL AND report_sha256 IS NULL FROM execution_receipts WHERE grant_id=?1",
        params![id,i64::try_from(MAX_DEFINITION_BYTES)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    )?;
    if !(1..=MAX_DEFINITION_BYTES as i64).contains(&size) || !empty_report {
        bail!("Damaged or oversized workload receipt; retained without repair")
    }
    let raw = raw.context("Workload definition unavailable")?;
    let definition: Definition = serde_json::from_str(&raw)?;
    if serde_json::to_string(&definition)? != raw
        || usize::try_from(limit)? != definition.events.max_total_bytes
    {
        bail!("Workload definition or reporting reservation changed")
    }
    let result = Receipt {
        schema_version: 2,
        receipt,
        definition,
        execution_authorized: false,
        restart_authorized: false,
    };
    result.validate()?;
    Ok(result)
}

/// Owned consumption only. Not serializable/cloneable and contains no source
/// factory. Dropping it retains uncertainty; it never records successful work.
pub struct Consumption {
    contract: Contract,
    policy: Policy,
    initial: Option<LeaseProof>,
    ledger: Arc<GrantLedger>,
    stop: stop::Watch,
    reporting: crate::workload_leases::ReportingAuthority,
}

// Explicit destruction order also covers cancellation of the whole async task.
// Final synchronous journal I/O must never keep an owned source future alive.
struct Running<F> {
    operation: Option<std::pin::Pin<Box<F>>>,
    consumption: Option<Consumption>,
}
impl<F> Drop for Running<F> {
    fn drop(&mut self) {
        drop(self.operation.take());
        drop(self.consumption.take());
    }
}
impl Consumption {
    #[allow(dead_code)]
    pub(crate) fn reporting_authority(&self) -> crate::workload_leases::ReportingAuthority {
        self.reporting.clone()
    }
    pub(crate) fn session_deadline(&self) -> Result<Instant> {
        let proof = self
            .initial
            .as_ref()
            .context("Initial workload authority already transferred")?;
        proof.initial_deadline(&self.contract, &self.policy)?;
        Ok(proof.session_deadline())
    }
    pub fn acknowledgement(&self) -> Result<Receipt> {
        let initial = self
            .initial
            .as_ref()
            .context("Initial workload authority already transferred")?;
        initial.initial_deadline(&self.contract, &self.policy)?;
        if *self.stop.receiver.borrow() {
            bail!("Workload stop already requested")
        }
        let result = self.ledger.workload_receipt(
            &self.contract.scope.issuer_id,
            &self.contract.scope.grant_id,
        )?;
        if result.receipt.state != "accepted" || result.definition.scope()? != self.contract.scope {
            bail!("Workload consumption changed before acknowledgement")
        }
        initial.initial_deadline(&self.contract, &self.policy)?;
        if *self.stop.receiver.borrow() {
            bail!("Workload stop requested during acknowledgement")
        }
        Ok(result)
    }

    /// Drive one lazy, already-prepared source after the authenticated caller
    /// writes the consumed ACK. This does not acquire resources, check listening
    /// or prepare/authorize source code; typed source admission must do those.
    /// An operation result is not a durable completion receipt. Drop still marks
    /// uncertainty until bounded event/completion persistence is integrated.
    #[allow(dead_code)]
    pub(crate) async fn supervise<F, T>(
        self,
        renewals: crate::workload_leases::Renewals,
        operation: F,
    ) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let mut running = Running {
            operation: Some(Box::pin(operation)),
            consumption: Some(self),
        };
        let consumed = running.consumption.as_mut().unwrap();
        consumed.acknowledgement()?;
        let initial = consumed
            .initial
            .take()
            .context("Workload authority already transferred")?;
        let contract = consumed.contract.clone();
        let policy = consumed.policy.clone();
        let mut stop = consumed.stop.receiver.clone();
        let operation = crate::workload_leases::supervise_reporting(
            &contract,
            &policy,
            initial,
            renewals,
            running.operation.as_mut().unwrap().as_mut(),
            consumed.reporting.clone(),
        );
        tokio::pin!(operation);
        loop {
            if *stop.borrow() {
                bail!("Workload stop requested")
            }
            tokio::select! {
                biased;
                changed=stop.changed()=> {
                    changed.context("Workload stop control unavailable")?;
                },
                result=&mut operation=> {
                    if *stop.borrow() { bail!("Workload stop requested before result"); }
                    return result;
                },
            }
        }
    }
}
impl Drop for Consumption {
    fn drop(&mut self) {
        // Retire in memory before any potentially blocked journal checkpoint.
        // Outstanding reporting handles cannot keep this lifetime active.
        self.reporting.retire();
        // No automatic retry after a failed write. An accepted row retained
        // after a crash/I/O failure remains consumed and uncertain, never live.
        let _ = self.ledger.mark_workload_unknown(&self.contract.scope);
    }
}
impl GrantLedger {
    pub fn workload_receipt(&self, issuer: &str, id: &str) -> Result<Receipt> {
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let tx = db.unchecked_transaction()?;
        let result = read(&tx, &self.node_id, Some(issuer), id)?;
        tx.commit()?;
        Ok(result)
    }
    /// Reserved for the eventual authenticated node admission path. This writes
    /// consumption only: it does not check microphone consent/permissions or
    /// acquire resources. The caller must do those before admission/effects.
    #[allow(dead_code)]
    pub(crate) fn consume_workload(
        self: &Arc<Self>,
        definition: Definition,
        policy: &Policy,
        initial: LeaseProof,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Consumption> {
        let contract = definition.lease_contract()?;
        let reporting = crate::workload_leases::ReportingAuthority::new(&contract)?;
        if node != self.node_id || !contract.scope.matches_session(issuer, session, node) {
            bail!("Workload consumption differs from pinned session/node")
        }
        initial.initial_deadline(&contract, policy)?;
        if definition.events.max_total_bytes > self.policy.max_result_bytes {
            return Err(GrantDenied("grant_owner_limits_exceeded").into());
        }
        let encoded = serde_json::to_string(&definition)?;
        {
            let mut db = self
                .connection
                .lock()
                .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            check_attempt_slot(&tx, &contract.scope)?;
            upgrade(&tx, &self.node_id)?;
            check_receipt_budget(
                &tx,
                &self.policy,
                definition.events.max_total_bytes,
                encoded.len(),
                false,
            )?;
            initial.initial_deadline(&contract, policy)?;
            let now = i64::try_from(crate::protocol::now_ms().max(1))?;
            tx.execute("INSERT INTO execution_receipts(issuer_id,job_id,task_id,attempt,grant_id,grant_digest,state,accepted_ms,updated_ms,max_result_bytes,report,report_sha256,receipt_kind,workload_definition) VALUES(?1,?2,?3,?4,?5,?6,'accepted',?7,?7,?8,NULL,NULL,'workload',?9)",
                params![definition.issuer_id,definition.job_id,definition.task_id,definition.attempt,definition.grant_id,contract.scope.grant_sha256,now,i64::try_from(definition.events.max_total_bytes)?,encoded])?;
            tx.commit()?;
        }
        let stop = match self.watch_stop_scope(&contract.scope) {
            Ok(watch) => watch,
            Err(error) => {
                let _ = self.mark_workload_unknown(&contract.scope);
                return Err(error);
            }
        };
        let consumed = Consumption {
            contract,
            policy: policy.clone(),
            initial: Some(initial),
            ledger: self.clone(),
            stop,
            reporting,
        };
        // I/O/stop registration consumes the original lease; no deadline reset.
        consumed.acknowledgement()?;
        Ok(consumed)
    }
    fn mark_workload_unknown(&self, scope: &stop::Scope) -> Result<()> {
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let tx = db.unchecked_transaction()?;
        let saved = read(&tx, &self.node_id, Some(&scope.issuer_id), &scope.grant_id)?;
        if saved.definition.scope()? != *scope {
            bail!("Workload consumption scope changed")
        }
        tx.execute("UPDATE execution_receipts SET state='unknown',updated_ms=max(updated_ms,accepted_ms,?3) WHERE grant_id=?1 AND grant_digest=?2 AND receipt_kind='workload' AND state='accepted'",
            params![scope.grant_id,scope.grant_sha256,i64::try_from(crate::protocol::now_ms().max(1))?])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "workload_receipts_tests.rs"]
mod tests;
