//! Continuous consumption in the existing node grant journal. No second job
//! store, source launch or restart authority. No command/network route calls the
//! mutator yet; authenticated admission and the coordinator journal remain open.
use super::*;
use crate::workload_leases::{Contract, LeaseProof, Policy, definition::Definition};

const MAX_DEFINITION_BYTES: usize = 64 * 1024;
#[path = "workload_completion.rs"]
mod completion;
#[path = "workload_event_journal.rs"]
mod event_journal;
#[path = "workload_finalization.rs"]
mod finalization;
pub use completion::Report as CompletionReport;
pub(crate) fn event_reserved(db: &Connection) -> Result<usize> {
    event_journal::reserved(db)
}
pub(crate) fn validate_event_orphans(db: &Connection) -> Result<()> {
    event_journal::audit_orphans(db)
}

pub(crate) fn validate_schema(db: &Connection, version: i64) -> Result<()> {
    let fields = db.prepare("SELECT name,type,\"notnull\",dflt_value FROM pragma_table_info('execution_receipts') WHERE name IN ('receipt_kind','workload_definition') ORDER BY name")?
        .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if matches!(version, 3 | 4 | 5) {
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
    event_journal::schema(db, version)
}
pub(crate) fn is_workload(db: &Connection, id: &str) -> Result<bool> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    validate_schema(db, version)?;
    if version < 3 {
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
    if matches!(version, 3 | 4 | 5) {
        return event_journal::upgrade(db, node);
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
    validate_schema(db, 3)?;
    event_journal::upgrade(db, node)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionReport>,
}
impl Receipt {
    pub fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        let scope = self.definition.scope()?;
        let outcome_valid = match (&self.completion, self.receipt.state.as_str()) {
            (None, "accepted" | "unknown") => self.schema_version == 2,
            (Some(report), "completed") => {
                report.validate(self)?;
                self.schema_version == 3
            }
            _ => false,
        };
        if !outcome_valid
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
        {
            bail!("Damaged workload consumption evidence; no recovery or replay")
        }
        Ok(())
    }
}
pub(crate) fn read(db: &Connection, node: &str, issuer: Option<&str>, id: &str) -> Result<Receipt> {
    let result = read_core(db, node, issuer, id)?;
    event_journal::audit(db, &result)?;
    Ok(result)
}
fn read_core(db: &Connection, node: &str, issuer: Option<&str>, id: &str) -> Result<Receipt> {
    crate::request_receipt::validate_owner(db, node)?;
    if !is_workload(db, id)? {
        bail!("Typed workload receipt unavailable")
    }
    let receipt = crate::request_receipt::read_consumed(db, node, issuer, id)?;
    let (size,raw,limit): (i64,Option<String>,i64) = db.query_row(
        "SELECT length(CAST(workload_definition AS BLOB)),CASE WHEN length(CAST(workload_definition AS BLOB))<=?2 THEN workload_definition ELSE NULL END,max_result_bytes FROM execution_receipts WHERE grant_id=?1",
        params![id,i64::try_from(MAX_DEFINITION_BYTES)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    )?;
    if !(1..=MAX_DEFINITION_BYTES as i64).contains(&size) {
        bail!("Damaged or oversized workload receipt; retained without repair")
    }
    let raw = raw.context("Workload definition unavailable")?;
    let definition: Definition = serde_json::from_str(&raw)?;
    if serde_json::to_string(&definition)? != raw
        || usize::try_from(limit)? != definition.events.max_total_bytes
    {
        bail!("Workload definition or reporting reservation changed")
    }
    let completion = completion::decode(db, id, &receipt.state)?;
    let result = Receipt {
        schema_version: if completion.is_some() { 3 } else { 2 },
        receipt,
        definition,
        execution_authorized: false,
        restart_authorized: false,
        completion,
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
    final_slot: Option<tokio::sync::OwnedSemaphorePermit>,
}

// Explicit destruction order also covers cancellation of the whole async task.
// Final journal I/O must never keep an owned source future alive.
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
    pub(crate) async fn retire(mut self) -> Result<()> {
        self.reporting.retire();
        let slot = self
            .final_slot
            .take()
            .context("Workload final checkpoint already delegated")?;
        finalization::start(&self.ledger, self.contract.scope.clone(), slot)?
            .wait()
            .await
    }
    pub(crate) fn event_writer(&self) -> Result<EventWriter> {
        let receipt = self.acknowledgement()?;
        Ok(EventWriter {
            ledger: self.ledger.clone(),
            definition: receipt.definition,
            stop: self.stop.receiver.clone(),
        })
    }
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
    /// An operation result is not a durable completion receipt. Source retirement
    /// precedes bounded asynchronous uncertainty persistence, including Drop.
    #[allow(dead_code)]
    pub(crate) async fn supervise<F, T>(
        self,
        renewals: crate::workload_leases::Renewals,
        operation: F,
    ) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let (consumed, finished) = self.drive(renewals, operation).await?;
        consumed.retire().await?;
        Ok(finished.value)
    }
    pub(crate) async fn supervise_observed<F>(
        self,
        renewals: crate::workload_leases::Renewals,
        operation: F,
    ) -> Result<Receipt>
    where
        F: std::future::Future<Output = Result<crate::workload_runtime::ObservedWorkload>>,
    {
        let (mut consumed, finished) = self.drive(renewals, operation).await?;
        let evidence = completion::Evidence::from_observed(
            consumed.contract.clone(),
            finished.boundary,
            finished.value,
        )?;
        consumed.reporting.retire();
        let slot = consumed
            .final_slot
            .take()
            .context("Workload final checkpoint already delegated")?;
        let stopped = consumed.stop.receiver.clone();
        finalization::start_with(&consumed.ledger, slot, move |journal| {
            completion::save(journal, evidence, stopped)
        })?
        .wait()
        .await?
    }
    async fn drive<F, T>(
        self,
        renewals: crate::workload_leases::Renewals,
        operation: F,
    ) -> Result<(Self, crate::workload_leases::Finished<T>)>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let mut running = Running {
            operation: Some(Box::pin(operation)),
            consumption: Some(self),
        };
        let result = {
            let consumed = running.consumption.as_mut().unwrap();
            consumed.acknowledgement()?;
            let initial = consumed
                .initial
                .take()
                .context("Workload authority already transferred")?;
            let contract = consumed.contract.clone();
            let policy = consumed.policy.clone();
            let mut stop = consumed.stop.receiver.clone();
            let operation = crate::workload_leases::supervise_finished(
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
                    break Err(anyhow::anyhow!("Workload stop requested"));
                }
                tokio::select! {
                    biased;
                    changed=stop.changed()=> {
                        if let Err(error)=changed {break Err(anyhow::Error::from(error).context("Workload stop control unavailable"));}
                    },
                    result=&mut operation=> {
                        if *stop.borrow() { break Err(anyhow::anyhow!("Workload stop requested before result")); }
                        break result;
                    },
                }
            }
        };
        // Drop the exact source and reporting window before starting any save.
        drop(running.operation.take());
        let consumed = running.consumption.take().unwrap();
        match result {
            Err(error) => {
                let _ = consumed.retire().await;
                Err(error)
            }
            Ok(finished) => Ok((consumed, finished)),
        }
    }
}
impl Drop for Consumption {
    fn drop(&mut self) {
        // Retire in memory before delegating any potentially blocked checkpoint.
        // Outstanding reporting handles cannot keep this lifetime active.
        self.reporting.retire();
        // No automatic retry after a failed write. An accepted row retained
        // after a crash/I/O failure remains consumed and uncertain, never live.
        if let Some(slot) = self.final_slot.take() {
            let _ = finalization::start(&self.ledger, self.contract.scope.clone(), slot);
        }
    }
}
impl GrantLedger {
    #[cfg(test)]
    pub(crate) fn workload_reporting_ready(&self) -> bool {
        self.final_slots.available_permits() == 1
            && !self
                .final_reporting_failed
                .load(std::sync::atomic::Ordering::Acquire)
    }
    /// Read-only bounded historical events. No live source, completion or replay
    /// authority is reconstructed from a retained prefix.
    pub fn workload_events(
        &self,
        issuer: &str,
        id: &str,
        after: u64,
        limit: usize,
    ) -> Result<Value> {
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let tx = db.unchecked_transaction()?;
        let receipt = read_core(&tx, &self.node_id, Some(issuer), id)?;
        let value = event_journal::inspect(&tx, &receipt, after, limit)?;
        tx.commit()?;
        Ok(value)
    }
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
        if self
            .final_reporting_failed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            bail!(
                "Workload final reporting failed; restart and inspect the journal before admission"
            )
        }
        if node != self.node_id || !contract.scope.matches_session(issuer, session, node) {
            bail!("Workload consumption differs from pinned session/node")
        }
        initial.initial_deadline(&contract, policy)?;
        if definition.events.max_total_bytes > self.policy.max_result_bytes {
            return Err(GrantDenied("grant_owner_limits_exceeded").into());
        }
        if self.final_slots.available_permits() == 0 {
            bail!("Workload final reporting capacity is busy; no consumption")
        }
        let encoded = serde_json::to_string(&definition)?;
        let final_slot;
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
                encoded
                    .len()
                    .checked_add(event_journal::reservation(&tx, &definition)?)
                    .context("Workload reservation overflow")?,
                false,
            )?;
            final_slot = self.final_slots.clone().try_acquire_owned().map_err(|_| {
                anyhow::anyhow!("Workload final reporting capacity is busy; no consumption")
            })?;
            if self
                .final_reporting_failed
                .load(std::sync::atomic::Ordering::Acquire)
            {
                bail!("Workload final reporting failed before admission")
            }
            initial.initial_deadline(&contract, policy)?;
            let now = i64::try_from(crate::protocol::now_ms().max(1))?;
            tx.execute("INSERT INTO execution_receipts(issuer_id,job_id,task_id,attempt,grant_id,grant_digest,state,accepted_ms,updated_ms,max_result_bytes,report,report_sha256,receipt_kind,workload_definition) VALUES(?1,?2,?3,?4,?5,?6,'accepted',?7,?7,?8,NULL,NULL,'workload',?9)",
                params![definition.issuer_id,definition.job_id,definition.task_id,definition.attempt,definition.grant_id,contract.scope.grant_sha256,now,i64::try_from(definition.events.max_total_bytes)?,encoded])?;
            event_journal::initialize(&tx, &definition)?;
            tx.commit()?;
        }
        let stop = match self.watch_stop_scope(&contract.scope) {
            Ok(watch) => watch,
            Err(error) => {
                let _ = finalization::start(self, contract.scope.clone(), final_slot);
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
            final_slot: Some(final_slot),
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

pub(crate) struct EventWriter {
    ledger: Arc<GrantLedger>,
    definition: Definition,
    stop: tokio::sync::watch::Receiver<bool>,
}
impl EventWriter {
    pub(crate) async fn append(
        &self,
        event: crate::workload_events::Event,
        checkpoint: Arc<crate::workload_leases::Checkpoint>,
    ) -> Result<event_journal::AppendReceipt> {
        let contract = self.definition.lease_contract()?;
        checkpoint.check(&contract)?;
        let slot = self
            .ledger
            .event_workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                anyhow::anyhow!("Workload journal worker capacity exhausted; source must stop")
            })?;
        let ledger = self.ledger.clone();
        let definition = self.definition.clone();
        let stop = self.stop.clone();
        let authority = checkpoint.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.ledger.submit_workload_job(Box::new(move || {
            // Cancellation cannot release this slot while a started SQLite save
            // is still running. No unbounded worker queue or automatic retry.
            let result = event_journal::append(&ledger, &definition, event, &authority, &stop);
            drop(slot);
            let _ = sender.send(result);
        }))?;
        checkpoint
            .supervise(&contract, async {
                receiver
                    .await
                    .context("Workload journal worker unavailable")?
            })
            .await
    }
}

#[cfg(test)]
#[path = "workload_receipts_tests.rs"]
mod tests;
