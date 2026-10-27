//! Bounded one-shot control records in the existing job database, never a runner.
#[cfg(test)]
#[path = "setup_model_check_store_tests.rs"]
mod tests;
use super::*;
use crate::setup_model_check::{Definition, Outcome, RECORD_BYTES, Record, State};

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS setup_model_checks (check_id TEXT PRIMARY KEY,record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB))<=4096),reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes=4096));")?;
    Ok(())
}
pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let (count, reserved, maximum): (i64, i64, i64) = connection.query_row(
        "SELECT count(*),COALESCE(SUM(reserved_bytes),0),COALESCE(MAX(length(CAST(record_json AS BLOB))),0) FROM setup_model_checks",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let count = usize::try_from(count)?;
    let reserved = usize::try_from(reserved)?;
    if count.checked_mul(RECORD_BYTES) != Some(reserved)
        || !(0..=RECORD_BYTES as i64).contains(&maximum)
    {
        bail!("Unconfirmed setup-check quota metadata")
    }
    Ok(reserved)
}
fn read(connection: &Connection, node: &str, operation: &str) -> Result<Option<Record>> {
    let id = eefn::model_verification::receipt_id(node, operation, "source", None)?;
    let mut statement = connection.prepare("SELECT length(CAST(record_json AS BLOB)),reserved_bytes,record_json FROM setup_model_checks WHERE check_id=?1")?;
    let mut rows = statement.query([&id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let length: i64 = row.get(0)?;
    let reserved: i64 = row.get(1)?;
    if !(1..=RECORD_BYTES as i64).contains(&length) || reserved != RECORD_BYTES as i64 {
        bail!("Unconfirmed setup check; retain without repair")
    }
    let raw: String = row.get(2)?;
    let record: Record = serde_json::from_str(&raw)
        .context("Damaged or future setup check; retain without replay")?;
    record.validate()?;
    if record.definition.node_id != node
        || record.definition.operation_id != operation
        || record.definition.id()? != id
    {
        bail!("Setup-check row identity changed")
    }
    Ok(Some(record))
}
fn save(connection: &Connection, record: &Record, insert: bool) -> Result<()> {
    record.validate()?;
    let encoded = serde_json::to_string(record)?;
    if encoded.len() > RECORD_BYTES {
        bail!("Setup-check record exceeds its reserved bound")
    }
    let id = record.definition.id()?;
    if insert {
        connection.execute("INSERT INTO setup_model_checks(check_id,record_json,reserved_bytes) VALUES (?1,?2,4096)",params![id,encoded])?;
    } else if connection.execute(
        "UPDATE setup_model_checks SET record_json=?1 WHERE check_id=?2 AND reserved_bytes=4096",
        params![encoded, id],
    )? != 1
    {
        bail!("Setup-check update identity missing")
    }
    Ok(())
}

impl JobStore {
    pub(crate) fn setup_model_check(&self, node: &str, operation: &str) -> Result<Option<Record>> {
        read(
            &self.connection.lock().expect("job journal"),
            node,
            operation,
        )
    }
    /// The caller registers a worker only when this exact immutable consent is new.
    pub(crate) fn arm_setup_model_check(
        &self,
        definition: Definition,
        runtime: &str,
    ) -> Result<(Record, bool)> {
        definition.validate()?;
        eefn::automation_occurrence::uuid(runtime)?;
        let connection = self.connection.lock().expect("job journal");
        if let Some(record) = read(&connection, &definition.node_id, &definition.operation_id)? {
            if record.definition != definition {
                bail!("Setup-check consent conflicts; no replacement or retry")
            }
            return Ok((record, false));
        }
        if model_verification_intents::read(
            &connection,
            &definition.node_id,
            &definition.operation_id,
        )?
        .is_some()
        {
            bail!("Model-check history already exists; inspect it without arming new consent")
        }
        let used: i64 = connection.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs", [], |row| row.get(0))?;
        let used = usize::try_from(used)?
            .checked_add(automations::definition_bytes(&connection)?)
            .context("Storage size overflow")?;
        if automations::count(&connection)? >= self.max_count
            || RECORD_BYTES > self.max_bytes.saturating_sub(used)
        {
            bail!("Setup-check history is full; no inference consent was saved")
        }
        let record = Record {
            schema_version: 1,
            definition,
            coordinator_runtime_id: runtime.into(),
            created_at_ms: eefn::protocol::now_ms(),
            state: State::Waiting,
            outcome: None,
        };
        save(&connection, &record, true)?;
        Ok((record, true))
    }
    /// Atomic race with cancel-wait. No claim is reconstructed on restart.
    pub(crate) fn launch_setup_model_check(
        &self,
        definition: &Definition,
        runtime: &str,
    ) -> Result<bool> {
        let connection = self.connection.lock().expect("job journal");
        let mut record = read(&connection, &definition.node_id, &definition.operation_id)?
            .context("Missing setup-check consent")?;
        if record.definition != *definition || record.coordinator_runtime_id != runtime {
            bail!("Setup-check owner changed")
        }
        if record.state != State::Waiting {
            return Ok(false);
        }
        record.state = State::Launching;
        save(&connection, &record, false)?;
        Ok(true)
    }
    pub(crate) fn cancel_setup_model_check_wait(
        &self,
        node: &str,
        operation: &str,
    ) -> Result<bool> {
        let connection = self.connection.lock().expect("job journal");
        let Some(mut record) = read(&connection, node, operation)? else {
            return Ok(false);
        };
        if record.state != State::Waiting {
            return Ok(false);
        }
        record.state = State::Cancelled;
        save(&connection, &record, false)?;
        Ok(true)
    }
    pub(crate) fn finish_setup_model_check(
        &self,
        definition: &Definition,
        runtime: &str,
        outcome: Option<Outcome>,
        timed_out: bool,
    ) -> Result<()> {
        let connection = self.connection.lock().expect("job journal");
        let mut record = read(&connection, &definition.node_id, &definition.operation_id)?
            .context("Missing setup-check consent")?;
        if record.definition != *definition || record.coordinator_runtime_id != runtime {
            bail!("Setup-check owner changed")
        }
        record.state = match record.state {
            State::Waiting if timed_out => State::Expired,
            State::Launching
                if outcome.as_ref().is_some_and(|o| {
                    !o.outcome_unknown
                        && matches!(
                            o.state,
                            crate::model_verification::State::ReturnedText
                                | crate::model_verification::State::NoText
                                | crate::model_verification::State::Failed
                        )
                }) && !timed_out =>
            {
                State::Finished
            }
            State::Launching => State::Unknown,
            State::Waiting => State::Unknown,
            _ => return Ok(()),
        };
        record.outcome = outcome;
        save(&connection, &record, false)
    }
}
