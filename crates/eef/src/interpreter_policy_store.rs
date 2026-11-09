//! Append-only owner service-policy generations in the existing job journal.
use super::*;
use eefn::interpreter_policy::{
    Authorization, Command, MAX_RECORD_BYTES, REVOCATION_RESERVE, Report, Revocation, Summary,
};
use serde_json::Value;

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS interpreter_service_policies (
        policy_id TEXT PRIMARY KEY,generation INTEGER NOT NULL UNIQUE CHECK(generation>0),
        record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB))<=16384),
        reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes IN (0,1024)),
        revocation_receipt TEXT UNIQUE,
        CHECK((revocation_receipt IS NULL AND reserved_bytes=1024) OR (revocation_receipt IS NOT NULL AND reserved_bytes=0)));")?;
    Ok(())
}
pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let value:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM interpreter_service_policies",[],|row|row.get(0))?;
    Ok(usize::try_from(value)?)
}
pub(super) fn records(connection: &Connection) -> Result<Vec<Authorization>> {
    let mut statement=connection.prepare("SELECT policy_id,generation,length(CAST(record_json AS BLOB)),record_json,reserved_bytes,revocation_receipt FROM interpreter_service_policies ORDER BY generation")?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    let mut receipts = std::collections::HashSet::new();
    while let Some(row) = rows.next()? {
        let length: i64 = row.get(2)?;
        if !(1..=MAX_RECORD_BYTES as i64).contains(&length) {
            bail!("Oversized interpreter policy; retain without activation")
        }
        let (id, generation, raw, reserved, revocation): (
            String,
            i64,
            String,
            i64,
            Option<String>,
        ) = (
            row.get(0)?,
            row.get(1)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        );
        let record: Authorization =
            serde_json::from_str(&raw).context("Damaged or future interpreter policy; retained")?;
        record.validate()?;
        if record.id != id
            || record.generation != u64::try_from(generation)?
            || record.generation != result.len() as u64 + 1
            || reserved
                != (if record.revocation.is_none() {
                    REVOCATION_RESERVE as i64
                } else {
                    0
                })
            || revocation != record.revocation.as_ref().map(|r| r.operation_id.clone())
            || !receipts.insert(record.id.clone())
        {
            bail!("Interpreter policy identity/generation/reserve mismatch")
        }
        if let Some(revocation) = &record.revocation {
            if !receipts.insert(revocation.operation_id.clone()) {
                bail!("Interpreter policy receipt reused across purposes")
            }
        }
        result.push(record);
    }
    Ok(result)
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    records(connection).map(|_| ())
}

impl JobStore {
    pub(crate) fn interpreter_policy_receipt(&self, id: &str) -> Result<Option<Authorization>> {
        eefn::interpreter_policy::uuid(id)?;
        Ok(records(&self.connection.lock().expect("job journal"))?
            .into_iter()
            .find(|record| record.id == id))
    }
    /// Trusted local-owner command path only; no dispatch, workers or inference.
    pub(crate) fn interpreter_policy_command(
        &self,
        command: Command,
        runtime: &str,
        issuer: Option<&str>,
        now: u64,
    ) -> Result<Value> {
        command.validate()?;
        eefn::interpreter_policy::uuid(runtime)?;
        let connection = self.connection.lock().expect("job journal");
        let retained = records(&connection)?;
        let current = retained.last().map_or(0, |record| record.generation);
        let mut report = Report {
            schema_version: 1,
            report_type: "coordinator_interpreter_policy".into(),
            success: true,
            runtime_id: runtime.into(),
            command: command.clone(),
            current_generation: current,
            authorization: None,
            items: vec![],
            next_after: None,
            reused: false,
            changed: false,
            grants_issued: false,
            model_inference_requested: false,
            automatically_retried: false,
            allowance_reservation_available: false,
            automatic_delegation_available: false,
        };
        let receipt_used = |id: &str| {
            retained
                .iter()
                .any(|r| r.id == id || r.revocation.as_ref().is_some_and(|v| v.operation_id == id))
        };
        match &command {
            Command::Show { id } => {
                report.authorization = match id {
                    Some(id) => Some(
                        retained
                            .iter()
                            .find(|record| record.id == *id)
                            .context("Interpreter policy unavailable")?
                            .clone(),
                    ),
                    None => retained.last().cloned(),
                }
            }
            Command::List {
                limit,
                after_generation,
            } => {
                let mut items = retained
                    .iter()
                    .filter(|record| record.generation > after_generation.unwrap_or(0));
                for record in items.by_ref().take(*limit as usize) {
                    report.items.push(Summary {
                        id: record.id.clone(),
                        generation: record.generation,
                        revision: record.revision,
                        revoked: record.revocation.is_some(),
                    });
                }
                if items.next().is_some() {
                    report.next_after = report.items.last().map(|item| item.generation);
                }
            }
            Command::Approve {
                operation_id,
                generation,
                valid_for_ms,
                ..
            } => {
                if let Some(old) = retained.iter().find(|record| record.id == *operation_id) {
                    if !old.creation.same_approval(&command) {
                        bail!("Interpreter approval receipt conflicts; no replacement")
                    }
                    report.authorization = Some(old.clone());
                    report.reused = true;
                } else {
                    if receipt_used(operation_id)
                        || current != *generation
                        || current >= i64::MAX as u64
                    {
                        bail!(
                            "Interpreter policy receipt/generation changed or exhausted; inspect before approval"
                        )
                    }
                    let issuer=issuer.context("Apply private coordinator credentials before approving interpreter sharing")?;
                    eefn::request_interpretation::digest(issuer)?;
                    if now == 0 {
                        bail!("Invalid interpreter approval time")
                    }
                    let mut record = Authorization {
                        schema_version: 1,
                        id: operation_id.clone(),
                        generation: current
                            .checked_add(1)
                            .context("Interpreter policy generation overflow")?,
                        creation: command.clone(),
                        runtime_id: runtime.into(),
                        issuer_id: issuer.into(),
                        approved_at_ms: now,
                        expires_at_ms: valid_for_ms
                            .map(|n| {
                                now.checked_add(n)
                                    .context("Interpreter policy time overflow")
                            })
                            .transpose()?,
                        authorization_sha256: String::new(),
                        revision: 1,
                        revocation: None,
                    };
                    record.authorization_sha256 = record.digest()?;
                    record.validate()?;
                    let encoded = serde_json::to_string(&record)?;
                    let cost = encoded
                        .len()
                        .checked_add(REVOCATION_RESERVE)
                        .context("Interpreter policy size overflow")?;
                    let jobs:i64=connection.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs",[],|r|r.get(0))?;
                    let used = usize::try_from(jobs)?
                        .checked_add(automations::definition_bytes(&connection)?)
                        .context("Interpreter policy storage overflow")?;
                    if automations::count(&connection)? >= self.max_count
                        || cost > self.max_bytes.saturating_sub(used)
                    {
                        bail!("Shared job/policy history is full; no interpreter approval saved")
                    }
                    connection.execute("INSERT INTO interpreter_service_policies(policy_id,generation,record_json,reserved_bytes) VALUES (?1,?2,?3,?4)",
                        params![record.id,i64::try_from(record.generation)?,encoded,i64::try_from(REVOCATION_RESERVE)?])?;
                    report.current_generation = record.generation;
                    report.authorization = Some(record);
                    report.changed = true;
                }
            }
            Command::Revoke {
                id,
                revision,
                operation_id,
            } => {
                let mut record = retained
                    .iter()
                    .find(|record| record.id == *id)
                    .context("Interpreter policy unavailable")?
                    .clone();
                if let Some(old) = &record.revocation {
                    if receipt_used(operation_id) && old.operation_id != *operation_id {
                        bail!("Interpreter revocation receipt conflicts")
                    }
                    report.reused = true;
                } else {
                    if receipt_used(operation_id) || *revision != record.revision || now == 0 {
                        bail!("Interpreter revocation receipt/revision changed")
                    }
                    let before = serde_json::to_vec(&record)?.len() + REVOCATION_RESERVE;
                    record.revocation = Some(Revocation {
                        operation_id: operation_id.clone(),
                        source_revision: *revision,
                        runtime_id: runtime.into(),
                        at_ms: now,
                    });
                    record.revision = 2;
                    record.validate()?;
                    let encoded = serde_json::to_string(&record)?;
                    if encoded.len() > before {
                        bail!("Interpreter revocation exceeds retained recovery reserve")
                    }
                    if connection.execute("UPDATE interpreter_service_policies SET record_json=?2,reserved_bytes=0,revocation_receipt=?3 WHERE policy_id=?1 AND reserved_bytes=1024",
                        params![record.id,encoded,operation_id])?!=1 {bail!("Interpreter policy changed before revocation")}
                    report.changed = true;
                }
                report.authorization = Some(record);
            }
        }
        let value = json!(report);
        if !eefn::interpreter_policy::confirmed(&value, &command, runtime) {
            bail!("Interpreter policy reply unconfirmed; inspect retained receipt")
        }
        Ok(value)
    }
}

#[cfg(test)]
#[path = "interpreter_policy_store_tests.rs"]
mod tests;
