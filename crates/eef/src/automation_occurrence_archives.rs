//! Immutable terminal evidence. Archiving never erases a budget reservation.
use super::*;
use eefn::automation_occurrence::Archive;

pub(super) fn resolved(plan: &Plan) -> bool {
    match plan.status {
        PlanStatus::Completed => {
            !plan.tasks.is_empty()
                && plan.tasks.iter().all(|t| t.status == TaskStatus::Completed)
                && plan
                    .execution
                    .as_ref()
                    .is_some_and(|e| e.attempts.len() == plan.tasks.len())
        }
        PlanStatus::Cancelled => {
            plan.execution
                .as_ref()
                .is_none_or(|e| e.attempts.is_empty())
                && plan.tasks.iter().all(|t| {
                    t.status == TaskStatus::Cancelled
                        && t.attempts == 0
                        && t.assignments.is_empty()
                        && t.result.is_none()
                })
                && plan.results.is_empty()
        }
        _ => false,
    }
}
fn validate_record(connection: &Connection, archive: &Archive) -> Result<Plan> {
    archive.validate()?;
    let OccurrenceCommand::Archive {
        reservation,
        reservation_sha256,
        job,
        ..
    } = &archive.command
    else {
        unreachable!()
    };
    let record = row(connection, reservation)?.context("Archive reservation missing; no refund")?;
    let plan: Plan = serde_json::from_value(archive.terminal_job.clone())?;
    if record.reservation_sha256 != *reservation_sha256
        || json!(plan) != archive.terminal_job
        || archive.archived_at_ms < record.reserved_at_ms
        || archive.archived_at_ms < plan.updated_at_ms
        || !resolved(&plan)
    {
        bail!("Damaged/unresolved/future terminal evidence; no replay or refund");
    }
    validate_retained_review(&plan)?;
    live_matches(&record, &plan)?;
    if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM durable_jobs WHERE job_id=?1)",
        [job],
        |r| r.get::<_, bool>(0),
    )? {
        bail!("Archived occurrence unexpectedly has a live job; retain without replay");
    }
    Ok(plan)
}
fn read_archive(connection: &Connection, id: &str) -> Result<Option<Archive>> {
    let saved: Option<(String,String,String)> = connection.query_row(
        "SELECT reservation_id,job_id,record_json FROM automation_occurrence_archives WHERE archive_id=?1",
        [id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((reservation, job, raw)) = saved else {
        return Ok(None);
    };
    let archive: Archive =
        serde_json::from_str(&raw).context("Damaged/future archive retained without migration")?;
    validate_record(connection, &archive)?;
    if archive.id != id
        || !matches!(&archive.command, OccurrenceCommand::Archive { reservation:r,job:j,.. } if *r==reservation && *j==job)
    {
        bail!("Archive journal identity differs");
    }
    Ok(Some(archive))
}
fn all(connection: &Connection) -> Result<Vec<Archive>> {
    let mut statement = connection
        .prepare("SELECT archive_id FROM automation_occurrence_archives ORDER BY archive_id")?;
    statement
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|id| read_archive(connection, &id?)?.context("Archive disappeared"))
        .collect()
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    all(connection)?;
    Ok(())
}
pub(super) fn retained_plan(connection: &Connection, record: &Reservation) -> Result<Plan> {
    let id: Option<String> = connection
        .query_row(
            "SELECT archive_id FROM automation_occurrence_archives WHERE reservation_id=?1",
            [&record.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = id {
        let archive = read_archive(connection, &id)?.context("Archive missing")?;
        return Ok(serde_json::from_value(archive.terminal_job)?);
    }
    let raw: String = connection
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [record.approved_job["plan_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .context("Occurrence job and terminal proof missing; no replacement or refund")?;
    let plan: Plan = serde_json::from_str(&raw)?;
    validate_retained_review(&plan)?;
    live_matches(record, &plan)?;
    Ok(plan)
}
pub(super) fn refuse_request(
    connection: &Connection,
    origin: &eefn::context::RequestContext,
) -> Result<()> {
    for archive in all(connection)? {
        let plan: Plan = serde_json::from_value(archive.terminal_job)?;
        if plan.goal.request_context.as_ref().is_some_and(|c| {
            c.origin_node() == origin.origin_node() && c.request_id() == origin.request_id()
        }) {
            bail!(
                "Operation belongs to archived occurrence {}; inspect owner automation-occurrences archive-show, never recreate it",
                archive.id
            );
        }
    }
    Ok(())
}
pub(super) fn checkpoint(
    store: &JobStore,
    connection: &Connection,
    command: &OccurrenceCommand,
    runtime: &str,
    clock: impl Fn() -> u64,
) -> Result<serde_json::Value> {
    let (archive, reused) = match command {
        OccurrenceCommand::ArchiveShow { id } => (
            read_archive(connection, id)?.context("Archive unavailable")?,
            false,
        ),
        OccurrenceCommand::Archive {
            reservation,
            reservation_sha256,
            job,
            revision,
            operation_id,
        } => {
            if let Some(archive) = read_archive(connection, operation_id)? {
                if archive.command != *command {
                    bail!("Archive receipt conflicts; inspect original archive");
                }
                (archive, true)
            } else {
                if row(connection, operation_id)?.is_some()
                    || automation_authorizations::read(connection, operation_id)?.is_some()
                {
                    bail!("Archive receipt is already an authority identity");
                }
                let record = row(connection, reservation)?.context("Reservation unavailable")?;
                let plan = read(connection, job)?;
                if record.reservation_sha256 != *reservation_sha256
                    || record.approved_job["plan_id"] != *job
                    || plan.revision != *revision
                    || !resolved(&plan)
                {
                    bail!(
                        "Archive needs exact proven completed or unissued cancelled occurrence revision"
                    );
                }
                let now = clock();
                let mut archive = Archive {
                    schema_version: 1,
                    id: operation_id.clone(),
                    command: command.clone(),
                    runtime_id: runtime.into(),
                    archived_at_ms: now,
                    terminal_job: json!(plan),
                    terminal_job_sha256: eefn::grants::definition_digest(&json!(plan))?,
                    archive_sha256: String::new(),
                };
                archive.archive_sha256 = archive.digest()?;
                archive.validate()?;
                let raw = serde_json::to_string(&archive)?;
                let old_cost: i64 = connection.query_row("SELECT length(CAST(plan_json AS BLOB))+reserved_bytes FROM durable_jobs WHERE job_id=?1", [job], |r|r.get(0))?;
                let transaction = connection.unchecked_transaction()?;
                transaction.execute("INSERT INTO automation_occurrence_archives(archive_id,reservation_id,job_id,record_json) VALUES (?1,?2,?3,?4)", params![archive.id,reservation,job,raw])?;
                if transaction.execute("DELETE FROM durable_jobs WHERE job_id=?1", [job])? != 1 {
                    bail!("Terminal job changed before archive checkpoint");
                }
                if raw.len() > usize::try_from(old_cost)? {
                    let jobs: i64 = transaction.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs", [], |r|r.get(0))?;
                    let total = usize::try_from(jobs)?
                        .checked_add(automations::definition_bytes(&transaction)?)
                        .context("Archive storage overflow")?;
                    if total > store.max_bytes {
                        bail!("Archive metadata exceeds storage quota; terminal job retained");
                    }
                }
                // One archive replaces one job, with no new count allowance.
                validate_record(&transaction, &archive)?;
                super::validate_all(&transaction)?;
                if clock() < now {
                    bail!("Clock moved backwards before archive commit; job retained");
                }
                transaction.commit()?;
                (archive, false)
            }
        }
        _ => bail!("Expected archive command"),
    };
    let mut value =
        crate::automation_occurrence::report(runtime, command, None, vec![], None, reused);
    value["archive"] = json!(archive);
    Ok(value)
}
