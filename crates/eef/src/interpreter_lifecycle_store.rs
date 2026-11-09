//! Interpreter cancellation uses the existing exact-grant stop contract.
use super::*;

fn retained(connection: &Connection, id: &str, binding: &str) -> Result<Record> {
    eefn::interpreter_policy::uuid(id)?;
    eefn::request_interpretation::digest(binding)?;
    let record = snapshot(connection)?
        .0
        .into_iter()
        .find(|r| r.id() == id)
        .context("Interpreter attempt unavailable")?;
    if record.binding_sha256 != binding {
        bail!("Interpreter continuation binding changed")
    }
    Ok(record)
}
fn increment(record: &mut Record) -> Result<()> {
    record.revision = record
        .revision
        .checked_add(1)
        .context("Interpreter revision overflow")?;
    Ok(())
}

impl JobStore {
    /// Retire only the exact in-process continuation. No report, fresh grant or
    /// proof of external stop is invented when a future is dropped.
    pub(crate) fn retire_interpreter_continuation(
        &self,
        id: &str,
        binding: &str,
    ) -> Result<Record> {
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old = retained(&transaction, id, binding)?;
        let mut record = old.clone();
        match record.phase {
            Phase::Reserved => record.phase = Phase::NotSent,
            Phase::DeliveryIntent | Phase::Accepted => record.phase = Phase::Interrupted,
            _ => {}
        }
        if record.phase != old.phase {
            increment(&mut record)?;
            update_record(&transaction, &old, &record)?;
        }
        retire_initial_claim(&transaction, &record)?;
        transaction.commit()?;
        Ok(record)
    }
    /// Save one send intent before the existing gateway's stop_grant call.
    /// Reading/retrying an old intent never returns another send continuation.
    pub(crate) fn interpreter_stop_intent(
        &self,
        id: &str,
        binding: &str,
    ) -> Result<Option<ExecutionGrant>> {
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old = retained(&transaction, id, binding)?;
        if old.stop.is_some() || !old.holds_slot() {
            return Ok(None);
        }
        let mut record = old.clone();
        if record.phase == Phase::Reserved {
            record.phase = Phase::NotSent;
            increment(&mut record)?;
            update_record(&transaction, &old, &record)?;
            retire_initial_claim(&transaction, &record)?;
            transaction.commit()?;
            return Ok(None);
        }
        let at = eefn::protocol::now_ms().max(
            record
                .delivery_at_ms
                .context("Missing interpreter delivery time")?,
        );
        record.stop = Some(crate::execution_stop::Delivery {
            send_attempted_at_ms: at,
            report: None,
            outcome_unknown: true,
        });
        increment(&mut record)?;
        update_record(&transaction, &old, &record)?;
        transaction.commit()?;
        Ok(Some(record.binding.grant))
    }
    pub(crate) fn interpreter_stop_observed(
        &self,
        id: &str,
        binding: &str,
        report: eefn::grants::stop::Report,
    ) -> Result<Record> {
        let connection = self.connection.lock().expect("job journal");
        let old = retained(&connection, id, binding)?;
        if !report.confirms(&old.binding.grant) {
            bail!("Interpreter stop report differs from the original grant")
        }
        let previous = old
            .stop
            .as_ref()
            .context("Interpreter stop has no saved send intent")?;
        if let Some(saved) = &previous.report {
            if saved != &report {
                bail!("Interpreter stop report conflicts with retained evidence")
            }
            return Ok(old);
        }
        let mut record = old.clone();
        let stop = record.stop.as_mut().expect("retained intent");
        stop.report = Some(report);
        stop.outcome_unknown = false;
        increment(&mut record)?;
        update_record(&connection, &old, &record)?;
        Ok(record)
    }
}
