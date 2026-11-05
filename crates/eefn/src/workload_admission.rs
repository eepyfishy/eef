//! Consumption persistence on the existing bounded native journal worker.
//! The original lease proof and any prepared source stay on the async caller.
use super::*;
use std::sync::atomic::Ordering;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

// This packet is historical metadata plus the ORIGINAL final reservation. It
// cannot authorize a source. Losing even a successful reply records uncertainty.
struct Committed {
    ledger: Arc<GrantLedger>,
    scope: stop::Scope,
    stop: Option<stop::Watch>,
    final_slot: Option<OwnedSemaphorePermit>,
}
impl Drop for Committed {
    fn drop(&mut self) {
        if let Some(slot) = self.final_slot.take() {
            let _ = finalization::start(&self.ledger, self.scope.clone(), slot);
        }
    }
}

fn still_waiting(
    ledger: &GrantLedger,
    reply: &oneshot::Sender<Result<Committed>>,
    end: Instant,
) -> Result<()> {
    ensure!(
        !reply.is_closed() && Instant::now() < end,
        "Workload admission abandoned or expired; no source authority"
    );
    ensure!(
        !ledger.final_reporting_failed.load(Ordering::Acquire),
        "Workload final reporting failed before admission"
    );
    Ok(())
}

fn save(
    ledger: &GrantLedger,
    definition: &Definition,
    scope: &stop::Scope,
    reply: &oneshot::Sender<Result<Committed>>,
    end: Instant,
    attempted: &mut bool,
) -> Result<()> {
    still_waiting(ledger, reply, end)?;
    let encoded = serde_json::to_string(definition)?;
    let mut db = ledger
        .connection
        .lock()
        .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
    still_waiting(ledger, reply, end)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    still_waiting(ledger, reply, end)?;
    check_attempt_slot(&tx, scope)?;
    upgrade(&tx, &ledger.node_id)?;
    check_receipt_budget(
        &tx,
        &ledger.policy,
        definition.events.max_total_bytes,
        encoded
            .len()
            .checked_add(event_journal::reservation(&tx, definition)?)
            .context("Workload reservation overflow")?,
        false,
    )?;
    still_waiting(ledger, reply, end)?;
    let now = i64::try_from(crate::protocol::now_ms().max(1))?;
    *attempted = true;
    tx.execute("INSERT INTO execution_receipts(issuer_id,job_id,task_id,attempt,grant_id,grant_digest,state,accepted_ms,updated_ms,max_result_bytes,report,report_sha256,receipt_kind,workload_definition) VALUES(?1,?2,?3,?4,?5,?6,'accepted',?7,?7,?8,NULL,NULL,'workload',?9)",
        params![definition.issuer_id,definition.job_id,definition.task_id,definition.attempt,definition.grant_id,scope.grant_sha256,now,i64::try_from(definition.events.max_total_bytes)?,encoded])?;
    event_journal::initialize(&tx, definition)?;
    still_waiting(ledger, reply, end)?;
    tx.commit()?;
    Ok(())
}

// Run only on the native worker, after the failed transaction has unwound. No
// row is turned into authority. Missing means rolled back; retained exact active
// consumption becomes unknown. Damaged evidence denies further admission.
fn settle_failed_save(ledger: &GrantLedger, scope: &stop::Scope) -> Result<()> {
    let retained = {
        let db = ledger
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable after admission failure"))?;
        let tx = db.unchecked_transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM execution_receipts WHERE grant_id=?1)",
            [&scope.grant_id],
            |r| r.get(0),
        )?;
        let retained = if exists {
            Some(read(&tx, &ledger.node_id, None, &scope.grant_id)?)
        } else {
            None
        };
        tx.commit()?;
        retained
    };
    if let Some(receipt) = retained {
        if receipt.definition.scope()? == *scope && receipt.receipt.state == "accepted" {
            ledger.mark_workload_unknown(scope)?;
        }
    }
    Ok(())
}

impl GrantLedger {
    /// Persist consumption before ACK/effects. The authenticated caller must
    /// first validate permissions/consent and reserve its exact prepared source.
    /// Nothing on the native worker owns a proof, source, runtime or factory.
    #[allow(dead_code, clippy::too_many_arguments)]
    pub(crate) async fn consume_workload(
        self: &Arc<Self>,
        definition: Definition,
        policy: &Policy,
        initial: LeaseProof,
        issuer: &str,
        session: &str,
        node: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Consumption> {
        let contract = definition.lease_contract()?;
        let reporting = crate::workload_leases::ReportingAuthority::new(&contract)?;
        ensure!(
            node == self.node_id && contract.scope.matches_session(issuer, session, node),
            "Workload consumption differs from pinned session/node"
        );
        let end = deadline.min(tokio::time::Instant::from_std(
            initial.initial_deadline(&contract, policy)?,
        ));
        ensure!(
            tokio::time::Instant::now() < end,
            "Workload admission deadline elapsed"
        );
        ensure!(
            !self.final_reporting_failed.load(Ordering::Acquire),
            "Workload final reporting failed; inspect retained evidence before admission"
        );
        if definition.events.max_total_bytes > self.policy.max_result_bytes {
            return Err(GrantDenied("grant_owner_limits_exceeded").into());
        }
        // Reserve before touching SQLite, so a pending final checkpoint refuses
        // immediately even while another native operation holds the mutex.
        let final_slot = self
            .final_slots
            .clone()
            .try_acquire_owned()
            .context("Workload final reporting capacity is busy; no consumption")?;
        let slot = self
            .event_workers
            .clone()
            .try_acquire_owned()
            .context("Workload admission worker capacity busy; no queued retry")?;
        let ledger = self.clone();
        let scope = contract.scope.clone();
        let (reply, receiver) = oneshot::channel();
        let native_end = end.into_std();
        self.submit_workload_job(Box::new(move || {
            #[cfg(test)]
            ledger.admission_started.fetch_add(1, Ordering::SeqCst);
            let mut attempted = false;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                save(
                    &ledger,
                    &definition,
                    &scope,
                    &reply,
                    native_end,
                    &mut attempted,
                )
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Workload admission worker panicked")));
            let result = match result {
                Ok(()) => {
                    let mut committed = Committed {
                        ledger: ledger.clone(),
                        scope,
                        stop: None,
                        final_slot: Some(final_slot),
                    };
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        committed.stop = Some(ledger.watch_stop_scope(&committed.scope)?);
                        Ok(committed)
                    }))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Workload stop registration failed")))
                }
                Err(error) => {
                    if attempted {
                        let settled =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                settle_failed_save(&ledger, &scope)
                            }));
                        if !matches!(settled, Ok(Ok(()))) {
                            ledger.final_reporting_failed.store(true, Ordering::Release);
                        }
                    }
                    drop(final_slot);
                    Err(error)
                }
            };
            drop(slot);
            drop(ledger);
            // A lost successful packet drops its own original final reservation.
            // The worker cannot reconstruct a proof from the committed row.
            let _ = reply.send(result);
        }))?;
        let mut committed = tokio::time::timeout_at(end, receiver)
            .await
            .context("Workload admission deadline elapsed; outcome unconfirmed")?
            .context("Workload admission worker unavailable; no fallback")??;
        initial.initial_deadline(&contract, policy)?;
        ensure!(
            tokio::time::Instant::now() < end,
            "Workload admission completed after its original deadline"
        );
        let mut consumed = Consumption {
            contract,
            policy: policy.clone(),
            initial: Some(initial),
            ledger: self.clone(),
            stop: committed
                .stop
                .take()
                .context("Workload stop watcher unavailable")?,
            reporting,
            final_slot: committed.final_slot.take(),
        };
        consumed.acknowledgement_before(end).await?;
        Ok(consumed)
    }
}
