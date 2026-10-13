//! Atomic budget consumption and delegated approval in the existing journal.
use super::*;
use eefn::automation_occurrence::{Delegation, OccurrenceCommand, Reservation};
#[path = "automation_occurrence_archives.rs"]
pub(super) mod archives;

pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let n: i64 = connection.query_row(
        "SELECT (SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrences)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_occurrence_archives)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM automation_schedules)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM automation_triggers)",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n)?)
}
fn matching_scope(plan: &Plan, policy: &eefn::automation_policy::Authorization) -> Result<()> {
    let mut scope = crate::automation_policy::fixed_scope(plan)?;
    if scope.owner_context.origin_node() != policy.scope.owner_context.origin_node()
        || scope.owner_context.origin_area() != policy.scope.owner_context.origin_area()
        || scope.owner_context.request_id() == policy.scope.owner_context.request_id()
    {
        bail!("Occurrence needs a fresh request identity with the exact approved node/area origin");
    }
    scope.owner_context = policy.scope.owner_context.clone();
    if scope != policy.scope {
        bail!("Occurrence operations/input/bindings differ from fixed approval scope");
    }
    Ok(())
}
fn validate_record(connection: &Connection, record: &Reservation) -> Result<()> {
    record.validate()?;
    let policy = automation_authorizations::read(connection, &record.delegation.policy_id)?
        .context("Missing retained occurrence policy")?;
    let eefn::automation_policy::PolicyCommand::Approve {
        definition,
        max_runs,
        ..
    } = &policy.creation
    else {
        unreachable!()
    };
    let source = automations::read_definition(connection, definition)?;
    let eefn::automation::AutomationCommand::Define { from_job, .. } = &source.creation else {
        unreachable!()
    };
    let snapshot: Plan = serde_json::from_value(record.approved_job.clone())?;
    if json!(snapshot) != record.approved_job {
        bail!("Future/noncanonical approved occurrence snapshot; retain without migration");
    }
    validate_retained_review(&snapshot)?;
    automation_schedules::validate_delegation(connection, &snapshot)?;
    matching_scope(&snapshot, &policy)?;
    let approval = snapshot
        .approval
        .as_ref()
        .context("Reservation lacks its actual delegated approval")?;
    let allowance = policy
        .scope
        .run_allowance(&policy.creation.limits().unwrap())?;
    if snapshot.plan_id == *from_job
        || record.delegation.definition_id != *definition
        || record.delegation.policy_sha256 != policy.authorization_sha256
        || record.delegation.generation != policy.generation
        || record.delegation.runtime_id != policy.runtime_id
        || record.ordinal > *max_runs
        || approval.issuer_id != policy.issuer_id
        || approval.limits != policy.creation.limits().unwrap()
        || approval.expires_at_ms != policy.expires_at_ms
        || approval.delegation.as_ref() != Some(&record.delegation)
        || record.reserved_at_ms < policy.approved_at_ms
        || record.reserved_at_ms >= policy.expires_at_ms
        || (record.reserved_execution_ms, record.reserved_result_bytes) != allowance
        || record.reserved_execution_ms > policy.expires_at_ms - record.reserved_at_ms
    {
        bail!("Damaged occurrence lineage/bounds; retain without activation");
    }
    Ok(())
}
pub(super) fn row(connection: &Connection, id: &str) -> Result<Option<Reservation>> {
    let value: Option<(String, String)> = connection
        .query_row(
            "SELECT job_id,record_json FROM automation_occurrences WHERE reservation_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((job_id, raw)) = value else {
        return Ok(None);
    };
    let record: Reservation =
        serde_json::from_str(&raw).context("Damaged/future occurrence retained without replay")?;
    validate_record(connection, &record)?;
    if record.id != id || record.approved_job["plan_id"] != job_id {
        bail!("Occurrence row identity differs");
    }
    Ok(Some(record))
}
fn live_matches(record: &Reservation, plan: &Plan) -> Result<()> {
    let snapshot: Plan = serde_json::from_value(record.approved_job.clone())?;
    if plan.plan_id != snapshot.plan_id
        || plan.revision < snapshot.revision
        || plan.goal.request_context != snapshot.goal.request_context
        || json!(plan.approval) != json!(snapshot.approval)
        || crate::automation_policy::fixed_scope(plan)?
            != crate::automation_policy::fixed_scope(&snapshot)?
        || plan.planning.as_ref().unwrap().plan_sha256
            != snapshot.planning.as_ref().unwrap().plan_sha256
    {
        bail!("Current job differs from retained occurrence authority");
    }
    Ok(())
}
pub(super) fn validate_job(connection: &Connection, plan: &Plan) -> Result<()> {
    automation_schedules::validate_delegation(connection, plan)?;
    if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM automation_occurrence_archives WHERE job_id=?1)",
        [&plan.plan_id],
        |r| r.get::<_, bool>(0),
    )? {
        bail!("Archived job unexpectedly live; no replay or replacement");
    }
    let id: Option<String> = connection
        .query_row(
            "SELECT reservation_id FROM automation_occurrences WHERE job_id=?1",
            [&plan.plan_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = id {
        live_matches(&row(connection, &id)?.context("Missing occurrence")?, plan)?;
    } else if plan
        .approval
        .as_ref()
        .is_some_and(|a| a.delegation.is_some())
    {
        bail!("Delegated job has no durable reservation");
    }
    Ok(())
}
fn records(connection: &Connection) -> Result<Vec<Reservation>> {
    automation_schedules::validate_all(connection)?;
    archives::validate_all(connection)?;
    let mut statement = connection
        .prepare("SELECT reservation_id FROM automation_occurrences ORDER BY reservation_id")?;
    let mut result = Vec::new();
    for id in statement.query_map([], |r| r.get::<_, String>(0))? {
        let record = row(connection, &id?)?.context("Missing reservation")?;
        archives::retained_plan(connection, &record)?;
        result.push(record);
    }
    let mut groups = std::collections::BTreeMap::<String, Vec<&Reservation>>::new();
    for record in &result {
        groups
            .entry(record.delegation.policy_id.clone())
            .or_default()
            .push(record);
    }
    for (id, mut group) in groups {
        group.sort_by_key(|r| r.ordinal);
        let policy = automation_authorizations::read(connection, &id)?
            .context("Missing reservation policy")?;
        let eefn::automation_policy::PolicyCommand::Approve {
            max_runs,
            max_reserved_execution_ms,
            max_reserved_result_bytes,
            ..
        } = policy.creation
        else {
            unreachable!()
        };
        let (mut duration, mut results) = (0u64, 0u64);
        for (index, record) in group.iter().enumerate() {
            if record.ordinal as usize != index + 1 {
                bail!("Occurrence budget history has a gap or duplicate");
            }
            duration = duration
                .checked_add(record.reserved_execution_ms)
                .context("Budget overflow")?;
            results = results
                .checked_add(record.reserved_result_bytes)
                .context("Budget overflow")?;
        }
        if group.len() > max_runs as usize
            || duration > max_reserved_execution_ms
            || results > max_reserved_result_bytes
        {
            bail!("Occurrence reservations exceed retained approval budget");
        }
    }
    Ok(result)
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    records(connection)?;
    Ok(())
}
pub(super) fn definition_resolved(connection: &Connection, definition: &str) -> Result<bool> {
    for record in records(connection)?
        .iter()
        .filter(|r| r.delegation.definition_id == definition)
    {
        if !archives::resolved(&archives::retained_plan(connection, record)?) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn allowance_available(
    connection: &Connection,
    policy: &eefn::automation_policy::Authorization,
) -> Result<bool> {
    let retained = records(connection)?;
    allowance_from_records(&retained, policy)
}
fn allowance_from_records(
    retained: &[Reservation],
    policy: &eefn::automation_policy::Authorization,
) -> Result<bool> {
    let own = retained
        .iter()
        .filter(|r| r.delegation.policy_id == policy.id)
        .collect::<Vec<_>>();
    let eefn::automation_policy::PolicyCommand::Approve {
        max_runs,
        max_reserved_execution_ms,
        max_reserved_result_bytes,
        ..
    } = &policy.creation
    else {
        unreachable!()
    };
    let used_ms = own
        .iter()
        .try_fold(0u64, |n, r| n.checked_add(r.reserved_execution_ms))
        .context("Budget overflow")?;
    let used_bytes = own
        .iter()
        .try_fold(0u64, |n, r| n.checked_add(r.reserved_result_bytes))
        .context("Budget overflow")?;
    let (duration, bytes) = policy
        .scope
        .run_allowance(&policy.creation.limits().unwrap())?;
    Ok(own.len() < *max_runs as usize
        && duration <= max_reserved_execution_ms.saturating_sub(used_ms)
        && bytes <= max_reserved_result_bytes.saturating_sub(used_bytes))
}
/// Both admission gates inspect ONE freshly fully validated journal view under
/// the caller's existing connection lock. No cross-call cache, time extension,
/// authority, reservation, repair or mutation. The commit fence remains required.
pub(super) fn trigger_admission_state(
    connection: &Connection,
    definition: &str,
    policy: &eefn::automation_policy::Authorization,
) -> Result<(bool, bool)> {
    let retained = records(connection)?;
    for record in retained
        .iter()
        .filter(|r| r.delegation.definition_id == definition)
    {
        if !archives::resolved(&archives::retained_plan(connection, record)?) {
            return Ok((false, false));
        }
    }
    Ok((true, allowance_from_records(&retained, policy)?))
}
pub(super) fn refuse_archived_request(
    connection: &Connection,
    origin: &eefn::context::RequestContext,
) -> Result<()> {
    archives::refuse_request(connection, origin)
}
pub(super) fn fence(
    connection: &Connection,
    plan: &Plan,
    runtime: &str,
    issuer: &str,
    clock: impl FnOnce() -> u64,
) -> Result<()> {
    let Some(delegation) = plan.approval.as_ref().and_then(|a| a.delegation.as_ref()) else {
        return Ok(());
    };
    let retained = records(connection)?;
    let record = retained
        .iter()
        .find(|r| r.id == delegation.reservation_id)
        .context("Reservation missing")?;
    live_matches(record, plan)?;
    let policy = automation_authorizations::read(connection, &delegation.policy_id)?
        .context("Policy missing")?;
    let definition = automations::read_definition(connection, &delegation.definition_id)?;
    let generation = automation_authorizations::records(connection)?.iter().filter(|r| matches!(&r.creation, eefn::automation_policy::PolicyCommand::Approve { definition, .. } if definition == &delegation.definition_id)).map(|r|r.generation).max().unwrap_or(0);
    let now = clock();
    if !policy.current_at(runtime, issuer, generation, &definition, now) {
        bail!(
            "Delegated occurrence policy expired, revoked, superseded, removed or from an old runtime"
        );
    }
    automation_schedules::fence(connection, plan, runtime, now)?;
    Ok(())
}
impl JobStore {
    pub(crate) fn occurrence_source(
        &self,
        command: &OccurrenceCommand,
    ) -> Result<(Option<Reservation>, Option<Plan>)> {
        command.validate()?;
        let OccurrenceCommand::Reserve {
            operation_id, job, ..
        } = command
        else {
            bail!("Expected reservation command");
        };
        let connection = self.connection.lock().expect("job journal");
        if let Some(record) = row(&connection, operation_id)? {
            if record.command != *command {
                bail!("Occurrence receipt conflicts; inspect original reservation");
            }
            return Ok((Some(record), None));
        }
        Ok((None, Some(read(&connection, job)?)))
    }
    pub fn automation_occurrence(&self, id: &str) -> Result<Option<Reservation>> {
        eefn::automation_occurrence::uuid(id)?;
        row(&self.connection.lock().expect("job journal"), id)
    }
    pub(crate) fn occurrence_checkpoint_now(
        &self,
        command: OccurrenceCommand,
        runtime: &str,
        issuer: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.occurrence_checkpoint_clock(command, runtime, issuer, eefn::protocol::now_ms)
    }
    #[cfg(test)]
    pub(crate) fn occurrence_checkpoint(
        &self,
        command: OccurrenceCommand,
        runtime: &str,
        issuer: Option<&str>,
        now: u64,
    ) -> Result<serde_json::Value> {
        self.occurrence_checkpoint_clock(command, runtime, issuer, || now)
    }
    fn occurrence_checkpoint_clock(
        &self,
        command: OccurrenceCommand,
        runtime: &str,
        issuer: Option<&str>,
        clock: impl Fn() -> u64,
    ) -> Result<serde_json::Value> {
        command.validate()?;
        eefn::automation_occurrence::uuid(runtime)?;
        let connection = self.connection.lock().expect("job journal");
        let retained = records(&connection)?;
        let (mut reservation, mut items, mut next, mut reused) = (None, Vec::new(), None, false);
        match &command {
            OccurrenceCommand::Archive { .. } | OccurrenceCommand::ArchiveShow { .. } => {
                return archives::checkpoint(self, &connection, &command, runtime, clock);
            }
            OccurrenceCommand::Show { id } => {
                reservation = Some(row(&connection, id)?.context("Occurrence unavailable")?);
            }
            OccurrenceCommand::List {
                policy,
                limit,
                after,
            } => {
                for record in &retained {
                    if policy
                        .as_ref()
                        .is_some_and(|p| p != &record.delegation.policy_id)
                        || after.as_ref().is_some_and(|a| record.id <= *a)
                    {
                        continue;
                    }
                    if items.len() == *limit as usize {
                        next = items
                            .last()
                            .and_then(|v: &serde_json::Value| v["id"].as_str())
                            .map(str::to_owned);
                        continue;
                    }
                    items.push(record.summary());
                }
            }
            OccurrenceCommand::Reserve {
                policy,
                policy_sha256,
                job,
                revision,
                plan_sha256,
                operation_id,
            } => {
                if let Some(old) = retained.iter().find(|r| r.id == *operation_id) {
                    if old.command != command {
                        bail!("Occurrence receipt conflicts; inspect original reservation");
                    }
                    reservation = Some(old.clone());
                    reused = true;
                } else {
                    let now = clock();
                    let issuer = issuer.context(
                        "Reservation needs independently checked current providers/issuer",
                    )?;
                    let policy_record = automation_authorizations::read(&connection, policy)?
                        .context("Policy unavailable")?;
                    let eefn::automation_policy::PolicyCommand::Approve {
                        definition,
                        max_runs,
                        max_reserved_execution_ms,
                        max_reserved_result_bytes,
                        ..
                    } = &policy_record.creation
                    else {
                        unreachable!()
                    };
                    let source = automations::read_definition(&connection, definition)?;
                    let generation = automation_authorizations::records(&connection)?.iter().filter(|r| matches!(&r.creation,eefn::automation_policy::PolicyCommand::Approve { definition:d, .. } if d==definition)).map(|r|r.generation).max().unwrap_or(0);
                    if policy_record.authorization_sha256 != *policy_sha256
                        || !policy_record.current_at(runtime, issuer, generation, &source, now)
                    {
                        bail!("Exact current reusable approval is unavailable");
                    }
                    let mut plan = read(&connection, job)?;
                    if plan.revision != *revision
                        || plan.status != PlanStatus::AwaitingApproval
                        || plan.approval.is_some()
                        || plan.execution.is_some()
                        || plan
                            .planning
                            .as_ref()
                            .is_none_or(|p| p.plan_sha256 != *plan_sha256)
                    {
                        bail!(
                            "Occurrence needs exact unapproved independently compiled job revision/digest"
                        );
                    }
                    matching_scope(&plan, &policy_record)?;
                    let eefn::automation::AutomationCommand::Define { from_job, .. } =
                        &source.creation
                    else {
                        unreachable!()
                    };
                    if job == from_job || retained.iter().any(|r| r.approved_job["plan_id"] == *job)
                    {
                        bail!("Original/already-reserved job cannot become a new occurrence");
                    }
                    for previous in retained
                        .iter()
                        .filter(|r| r.delegation.definition_id == *definition)
                    {
                        let p = archives::retained_plan(&connection, previous)?;
                        if !archives::resolved(&p) {
                            bail!(
                                "Previous occurrence is unfinished/uncertain; no overlap or replacement"
                            );
                        }
                    }
                    let group = retained
                        .iter()
                        .filter(|r| r.delegation.policy_id == *policy)
                        .collect::<Vec<_>>();
                    let (duration, result_bytes) = policy_record
                        .scope
                        .run_allowance(&policy_record.creation.limits().unwrap())?;
                    let used_duration = group
                        .iter()
                        .try_fold(0u64, |n, r| n.checked_add(r.reserved_execution_ms))
                        .context("Budget overflow")?;
                    let used_results = group
                        .iter()
                        .try_fold(0u64, |n, r| n.checked_add(r.reserved_result_bytes))
                        .context("Budget overflow")?;
                    if group.len() >= *max_runs as usize
                        || duration > max_reserved_execution_ms.saturating_sub(used_duration)
                        || result_bytes > max_reserved_result_bytes.saturating_sub(used_results)
                        || duration > policy_record.expires_at_ms - now
                    {
                        bail!("Occurrence run/duration/result/remaining-time budget exhausted");
                    }
                    let delegation = Delegation {
                        schema_version: 1,
                        reservation_id: operation_id.clone(),
                        policy_id: policy.clone(),
                        policy_sha256: policy_sha256.clone(),
                        definition_id: definition.clone(),
                        generation,
                        runtime_id: runtime.into(),
                        trigger: automation_schedules::reservation_link(
                            &connection,
                            &plan,
                            &policy_record,
                            operation_id,
                            now,
                        )?,
                    };
                    let mut approval = crate::request_approval::ApprovalRecord {
                        schema_version: 1,
                        operation_id: operation_id.clone(),
                        source_revision: *revision,
                        plan_sha256: plan_sha256.clone(),
                        issuer_id: issuer.into(),
                        approved_at_ms: now,
                        valid_for_ms: policy_record.expires_at_ms - now,
                        expires_at_ms: policy_record.expires_at_ms,
                        limits: policy_record.creation.limits().unwrap(),
                        approval_sha256: String::new(),
                        delegation: Some(delegation.clone()),
                    };
                    approval.approval_sha256 = crate::request_approval::digest(&approval)?;
                    plan.approval = Some(approval);
                    plan.status = PlanStatus::Approved;
                    stamp(
                        &mut plan,
                        "Occurrence authority reserved atomically; no timer, grant or dispatch",
                    )?;
                    validate_retained_review(&plan)?;
                    let snapshot = json!(plan);
                    let mut record = Reservation {
                        schema_version: 1,
                        id: operation_id.clone(),
                        command: command.clone(),
                        delegation,
                        ordinal: u16::try_from(group.len() + 1)?,
                        reserved_at_ms: now,
                        reserved_execution_ms: duration,
                        reserved_result_bytes: result_bytes,
                        approved_job_sha256: eefn::grants::definition_digest(&snapshot)?,
                        approved_job: snapshot,
                        reservation_sha256: String::new(),
                    };
                    record.reservation_sha256 = record.digest()?;
                    validate_record(&connection, &record)?;
                    let encoded = serde_json::to_string(&plan)?;
                    let raw = serde_json::to_string(&record)?;
                    let transaction = connection.unchecked_transaction()?;
                    self.check_size(&transaction, job, encoded.len(), reserve(&plan))?;
                    transaction.execute("UPDATE durable_jobs SET plan_json=?2,updated_ms=?3,reserved_bytes=?4 WHERE job_id=?1", params![job, encoded, i64::try_from(plan.updated_at_ms)?, i64::try_from(reserve(&plan))?])?;
                    if automations::count(&transaction)? >= self.max_count {
                        bail!("Job/automation history is full; reservation not saved");
                    }
                    let job_bytes: i64 = transaction.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs", [], |r|r.get(0))?;
                    let used = usize::try_from(job_bytes)?
                        .checked_add(automations::definition_bytes(&transaction)?)
                        .context("Storage overflow")?;
                    if raw.len() > self.max_bytes.saturating_sub(used) {
                        bail!(
                            "Job/automation storage is full; approval and reservation rolled back"
                        );
                    }
                    transaction.execute("INSERT INTO automation_occurrences(reservation_id,job_id,record_json) VALUES (?1,?2,?3)", params![record.id, job, raw])?;
                    validate_job(&transaction, &plan)?;
                    validate_all(&transaction)?;
                    let commit_time = clock();
                    if commit_time < now
                        || !policy_record.current_at(
                            runtime,
                            issuer,
                            generation,
                            &source,
                            commit_time,
                        )
                        || duration > policy_record.expires_at_ms.saturating_sub(commit_time)
                    {
                        bail!(
                            "Clock changed or policy expired before reservation commit; rolled back"
                        );
                    }
                    transaction.commit()?;
                    reservation = Some(record);
                }
            }
        }
        Ok(crate::automation_occurrence::report(
            runtime,
            &command,
            reservation,
            items,
            next,
            reused,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskEngine;
    use eefn::automation_policy::{Authorization, PolicyCommand};
    fn setup(store: Arc<JobStore>) -> (Arc<TaskEngine>, Plan, Authorization) {
        setup_with_validity(store, 10000)
    }
    fn setup_with_validity(
        store: Arc<JobStore>,
        valid_for_ms: u64,
    ) -> (Arc<TaskEngine>, Plan, Authorization) {
        let (engine, source, _definition, policy) =
            super::super::automation_authorizations::tests::fixture_with_validity(
                store.clone(),
                valid_for_ms,
            );
        store
            .automation_policy_checkpoint(
                policy.creation.clone(),
                Some(policy.clone()),
                &policy.runtime_id,
                policy.approved_at_ms,
            )
            .unwrap();
        (engine, source, policy)
    }
    fn fresh(engine: &TaskEngine) -> Plan {
        crate::request_approval::tests::compiled(engine)
    }
    fn command(policy: &Authorization, plan: &Plan) -> OccurrenceCommand {
        OccurrenceCommand::Reserve {
            policy: policy.id.clone(),
            policy_sha256: policy.authorization_sha256.clone(),
            job: plan.plan_id.clone(),
            revision: plan.revision,
            plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
            operation_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn at(policy: &Authorization, plan: &Plan) -> u64 {
        policy
            .approved_at_ms
            .max(plan.planning.as_ref().unwrap().compiled_at_ms)
            + 1
    }
    fn save(
        store: &JobStore,
        policy: &Authorization,
        plan: &Plan,
        command: &OccurrenceCommand,
    ) -> serde_json::Value {
        store
            .occurrence_checkpoint(
                command.clone(),
                &policy.runtime_id,
                Some(&policy.issuer_id),
                at(policy, plan),
            )
            .unwrap()
    }
    fn used(value: &serde_json::Value) -> usize {
        [
            "payload_bytes",
            "reserved_bytes",
            "automation_bytes",
            "authorization_bytes",
            "authorization_reserved_bytes",
            "occurrence_bytes",
            "occurrence_archive_bytes",
            "schedule_bytes",
            "schedule_reserved_bytes",
            "trigger_bytes",
        ]
        .iter()
        .map(|k| value[k].as_u64().unwrap() as usize)
        .sum()
    }
    fn archive_command(record: &serde_json::Value, plan: &Plan) -> OccurrenceCommand {
        OccurrenceCommand::Archive {
            reservation: record["id"].as_str().unwrap().into(),
            reservation_sha256: record["reservation_sha256"].as_str().unwrap().into(),
            job: plan.plan_id.clone(),
            revision: plan.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn archive(
        store: &JobStore,
        command: OccurrenceCommand,
        policy: &Authorization,
    ) -> serde_json::Value {
        store
            .occurrence_checkpoint(command, &policy.runtime_id, None, policy.expires_at_ms + 1)
            .unwrap()
    }
    #[test]
    fn terminal_archive_removes_only_live_history_and_never_refunds_or_recreates() {
        let store = JobStore::in_memory().unwrap();
        // Archive/no-refund behavior is independent of short expiry timing.
        let (engine, source, policy) = setup_with_validity(store.clone(), 60000);
        let original = json!(store.get(&source.plan_id).unwrap());
        for ordinal in 1..=3 {
            let job = fresh(&engine);
            let reserve_cmd = command(&policy, &job);
            let saved = save(&store, &policy, &job, &reserve_cmd);
            assert_eq!(saved["reservation"]["ordinal"], ordinal);
            let cancelled = engine.cancel(&job.plan_id).unwrap();
            let cmd = archive_command(&saved["reservation"], &cancelled);
            let result = archive(&store, cmd.clone(), &policy);
            assert!(eefn::automation_occurrence::confirmed(
                &result,
                &cmd,
                &policy.runtime_id
            ));
            assert_eq!(result["archive"]["terminal_job"], json!(cancelled));
            assert!(store.get(&job.plan_id).is_err());
            let usage = store.usage().unwrap();
            assert_eq!(usage["count"], 1);
            assert_eq!(usage["occurrence_count"], ordinal);
            assert_eq!(usage["occurrence_archive_count"], ordinal);
            assert_eq!(archive(&store, cmd.clone(), &policy)["reused"], true);
            assert_eq!(store.usage().unwrap(), usage);
            assert_eq!(
                archive(&store, reserve_cmd, &policy)["reservation"],
                saved["reservation"]
            );
            let show = OccurrenceCommand::ArchiveShow {
                id: cmd.receipt().unwrap().into(),
            };
            let inspected = archive(&store, show.clone(), &policy);
            assert!(eefn::automation_occurrence::confirmed(
                &inspected,
                &show,
                &policy.runtime_id
            ));
            assert_eq!(inspected["archive"], result["archive"]);
            let ctx = job.goal.request_context.as_ref().unwrap();
            let draft = job.request_review.as_ref().unwrap();
            let replay = crate::request_drafts::open(
                &engine,
                json!({"schema_version":1,
                "original_text":draft.original_text,"proposal":draft.proposals[0].proposal}),
                ctx,
                &["system.info".into()],
            );
            assert!(
                replay
                    .unwrap_err()
                    .to_string()
                    .contains("archived occurrence")
            );
            let mut conflict = cmd;
            if let OccurrenceCommand::Archive { revision, .. } = &mut conflict {
                *revision += 1;
            }
            assert!(
                store
                    .occurrence_checkpoint(
                        conflict,
                        &policy.runtime_id,
                        None,
                        policy.expires_at_ms + 1
                    )
                    .is_err()
            );
        }
        let fourth = fresh(&engine);
        assert!(
            store
                .occurrence_checkpoint(
                    command(&policy, &fourth),
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &fourth)
                )
                .unwrap_err()
                .to_string()
                .contains("budget exhausted")
        );
        assert_eq!(json!(store.get(&source.plan_id).unwrap()), original);
    }
    #[test]
    fn archive_write_delete_and_clock_faults_leave_the_job_and_budget_unchanged() {
        for fault in ["insert", "delete", "clock"] {
            let store = JobStore::in_memory().unwrap();
            let (engine, _source, policy) = setup(store.clone());
            let job = fresh(&engine);
            let saved = save(&store, &policy, &job, &command(&policy, &job));
            let cancelled = engine.cancel(&job.plan_id).unwrap();
            let cmd = archive_command(&saved["reservation"], &cancelled);
            let before = store.usage().unwrap();
            if fault != "clock" {
                let trigger = if fault == "insert" {
                    "BEFORE INSERT ON automation_occurrence_archives"
                } else {
                    "BEFORE DELETE ON durable_jobs"
                };
                store.connection.lock().unwrap().execute_batch(&format!("CREATE TRIGGER archive_fault {trigger} BEGIN SELECT RAISE(FAIL,'isolated archive fault'); END;")).unwrap();
            }
            let calls = std::cell::Cell::new(0u8);
            let now = policy.expires_at_ms + 1;
            let result =
                store.occurrence_checkpoint_clock(cmd.clone(), &policy.runtime_id, None, || {
                    let count = calls.get();
                    calls.set(count + 1);
                    if fault == "clock" && count > 0 {
                        now - 1
                    } else {
                        now
                    }
                });
            assert!(result.is_err(), "{fault}");
            assert_eq!(json!(store.get(&job.plan_id).unwrap()), json!(cancelled));
            assert_eq!(store.usage().unwrap(), before);
            if fault != "clock" {
                store
                    .connection
                    .lock()
                    .unwrap()
                    .execute_batch("DROP TRIGGER archive_fault")
                    .unwrap();
            }
            assert_eq!(archive(&store, cmd, &policy)["reused"], false);
        }
    }
    #[test]
    fn exact_terminal_revision_digest_and_proof_are_required_before_archiving() {
        let store = JobStore::in_memory().unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        let saved = save(&store, &policy, &job, &command(&policy, &job));
        let approved = store.get(&job.plan_id).unwrap();
        let cmd = archive_command(&saved["reservation"], &approved);
        assert!(
            store
                .occurrence_checkpoint(cmd, &policy.runtime_id, None, policy.expires_at_ms + 1)
                .is_err()
        );
        let cancelled = engine.cancel(&job.plan_id).unwrap();
        for field in ["revision", "reservation_sha256", "job", "reservation"] {
            let mut bad = json!(archive_command(&saved["reservation"], &cancelled));
            bad[field] = match field {
                "revision" => json!(cancelled.revision - 1),
                "reservation_sha256" => json!("a".repeat(64)),
                "job" => json!("a".repeat(32)),
                _ => json!(uuid::Uuid::new_v4().to_string()),
            };
            assert!(
                store
                    .occurrence_checkpoint(
                        serde_json::from_value(bad).unwrap(),
                        &policy.runtime_id,
                        None,
                        policy.expires_at_ms + 1
                    )
                    .is_err()
            );
        }
        assert_eq!(store.usage().unwrap()["occurrence_archive_count"], 0);
        // Status alone is not terminal evidence.
        let mut forged = cancelled;
        forged.status = PlanStatus::Completed;
        for t in &mut forged.tasks {
            t.status = TaskStatus::Completed;
        }
        assert!(!archives::resolved(&forged));
        for status in [
            PlanStatus::Failed,
            PlanStatus::Interrupted,
            PlanStatus::Abandoned,
            PlanStatus::Stopping,
        ] {
            forged.status = status;
            assert!(!archives::resolved(&forged));
        }
    }
    #[test]
    fn archive_quota_failure_is_atomic_and_replacement_preserves_shared_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        let saved = save(&store, &policy, &job, &command(&policy, &job));
        let cancelled = engine.cancel(&job.plan_id).unwrap();
        let cmd = archive_command(&saved["reservation"], &cancelled);
        let before = store.usage().unwrap();
        let count = automations::count(&store.connection.lock().unwrap()).unwrap();
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, count, used(&before)).unwrap();
        assert!(
            store
                .occurrence_checkpoint(
                    cmd.clone(),
                    &policy.runtime_id,
                    None,
                    policy.expires_at_ms + 1
                )
                .unwrap_err()
                .to_string()
                .contains("quota")
        );
        assert_eq!(store.usage().unwrap()["occurrence_archive_count"], 0);
        assert_eq!(json!(store.get(&job.plan_id).unwrap()), json!(cancelled));
        drop(store);
        let store = JobStore::open(&path, count, used(&before) + 4096).unwrap();
        archive(&store, cmd.clone(), &policy);
        assert_eq!(
            automations::count(&store.connection.lock().unwrap()).unwrap(),
            count
        );
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        let new_runtime = uuid::Uuid::new_v4().to_string();
        let reused = store
            .occurrence_checkpoint(cmd, &new_runtime, None, policy.expires_at_ms + 100)
            .unwrap();
        assert_eq!(reused["reused"], true);
        assert_eq!(reused["archive"]["runtime_id"], policy.runtime_id);
        assert!(store.get(&job.plan_id).is_err());
    }
    #[test]
    fn missing_future_or_reintroduced_terminal_evidence_fails_closed_without_repair() {
        for fault in ["missing", "future", "live"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            Connection::open(&path).unwrap();
            let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
            let (engine, _source, policy) = setup(store.clone());
            let job = fresh(&engine);
            let saved = save(&store, &policy, &job, &command(&policy, &job));
            let terminal = engine.cancel(&job.plan_id).unwrap();
            let result = archive(
                &store,
                archive_command(&saved["reservation"], &terminal),
                &policy,
            );
            {
                let connection = store.connection.lock().unwrap();
                match fault {
                    "missing" => {
                        connection
                            .execute("DELETE FROM automation_occurrence_archives", [])
                            .unwrap();
                    }
                    "future" => {
                        let mut raw = result["archive"].clone();
                        raw["schema_version"] = json!(2);
                        connection
                            .execute(
                                "UPDATE automation_occurrence_archives SET record_json=?1",
                                [serde_json::to_string(&raw).unwrap()],
                            )
                            .unwrap();
                    }
                    _ => {
                        connection.execute("INSERT INTO durable_jobs(job_id,plan_json,updated_ms,reserved_bytes) VALUES(?1,?2,?3,0)",params![terminal.plan_id,serde_json::to_string(&terminal).unwrap(),terminal.updated_at_ms as i64]).unwrap();
                    }
                }
            }
            drop(engine);
            drop(store);
            assert!(JobStore::open(&path, 20, 1024 * 1024).is_err(), "{fault}");
            let raw = Connection::open(&path).unwrap();
            assert_eq!(
                raw.query_row("SELECT count(*) FROM automation_occurrences", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
    #[test]
    fn joint_trigger_gate_is_fresh_read_only_and_preserves_overlap_and_no_refund() {
        let store = JobStore::in_memory().unwrap();
        let (engine, _, policy) = setup_with_validity(store.clone(), 60000);
        let PolicyCommand::Approve { definition, .. } = &policy.creation else {
            unreachable!()
        };
        let inspect = |expected| {
            let db = store.connection.lock().unwrap();
            let changes = db.total_changes();
            let combined = trigger_admission_state(&db, definition, &policy).unwrap();
            assert_eq!(combined, expected);
            assert_eq!(combined.0, definition_resolved(&db, definition).unwrap());
            if combined.0 {
                assert_eq!(combined.1, allowance_available(&db, &policy).unwrap());
            }
            assert_eq!(
                db.total_changes(),
                changes,
                "inspection cannot mutate, repair or refund"
            );
        };
        inspect((true, true));
        for ordinal in 1..=3 {
            let job = fresh(&engine);
            let saved = save(&store, &policy, &job, &command(&policy, &job));
            inspect((false, false));
            let cancelled = engine.cancel(&job.plan_id).unwrap();
            inspect((true, ordinal < 3));
            archive(
                &store,
                archive_command(&saved["reservation"], &cancelled),
                &policy,
            );
            inspect((true, ordinal < 3));
        }
        let db = store.connection.lock().unwrap();
        db.execute(
            "UPDATE automation_occurrences SET record_json='{}' WHERE json_extract(record_json,'$.ordinal')=1",
            [],
        )
        .unwrap();
        assert!(
            trigger_admission_state(&db, definition, &policy).is_err(),
            "a previous validated view cannot hide later damage"
        );
    }

    #[test]
    fn atomic_reservation_is_new_job_approval_not_source_mutation_or_dispatch() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, policy) = setup(store.clone());
        let original = json!(store.get(&source.plan_id).unwrap());
        let job = fresh(&engine);
        let cmd = command(&policy, &job);
        let result = save(&store, &policy, &job, &cmd);
        assert!(eefn::automation_occurrence::confirmed(
            &result,
            &cmd,
            &policy.runtime_id
        ));
        let approved = store.get(&job.plan_id).unwrap();
        assert_eq!(approved.status, PlanStatus::Approved);
        assert_eq!(approved.revision, job.revision + 1);
        assert!(approved.execution.is_none());
        assert_eq!(
            approved.request_review.as_ref().unwrap().proposals.len(),
            job.request_review.as_ref().unwrap().proposals.len()
        );
        assert_ne!(approved.goal.request_context, source.goal.request_context);
        assert_eq!(
            approved.planning.as_ref().unwrap().plan_sha256,
            job.planning.as_ref().unwrap().plan_sha256
        );
        assert_eq!(json!(store.get(&source.plan_id).unwrap()), original);
        let ordinary = crate::request_approval::tests::command(&job);
        assert!(!approved.approval.as_ref().unwrap().confirms(&ordinary));
        let reused = store
            .occurrence_checkpoint(
                cmd.clone(),
                &uuid::Uuid::new_v4().to_string(),
                None,
                policy.expires_at_ms + 1,
            )
            .unwrap();
        assert_eq!(reused["reused"], true);
        assert_eq!(reused["reservation"], result["reservation"]);
        assert_eq!(store.usage().unwrap()["occurrence_count"], 1);
        assert_eq!(store.usage().unwrap()["count"], 2);
        engine.cancel(&job.plan_id).unwrap();
        assert!(
            store
                .remove_finished(&job.plan_id)
                .unwrap_err()
                .to_string()
                .contains("lineage")
        );
        assert_eq!(
            store
                .occurrence_checkpoint(cmd, &policy.runtime_id, None, policy.expires_at_ms + 1)
                .unwrap()["reused"],
            true
        );
    }
    #[test]
    fn no_overlap_or_refunds_and_run_budget_survives_unissued_cancellation() {
        let store = JobStore::in_memory().unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let first = fresh(&engine);
        save(&store, &policy, &first, &command(&policy, &first));
        let second = fresh(&engine);
        let second_cmd = command(&policy, &second);
        assert!(
            store
                .occurrence_checkpoint(
                    second_cmd.clone(),
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &second)
                )
                .unwrap_err()
                .to_string()
                .contains("overlap")
        );
        assert!(store.get(&second.plan_id).unwrap().approval.is_none());
        engine.cancel(&first.plan_id).unwrap();
        let r = save(&store, &policy, &second, &second_cmd);
        assert_eq!(r["reservation"]["ordinal"], 2);
        engine.cancel(&second.plan_id).unwrap();
        let third = fresh(&engine);
        save(&store, &policy, &third, &command(&policy, &third));
        engine.cancel(&third.plan_id).unwrap();
        let fourth = fresh(&engine);
        assert!(
            store
                .occurrence_checkpoint(
                    command(&policy, &fourth),
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &fourth)
                )
                .unwrap_err()
                .to_string()
                .contains("budget exhausted")
        );
        assert!(store.get(&fourth.plan_id).unwrap().approval.is_none());
        assert_eq!(store.usage().unwrap()["occurrence_count"], 3);
    }
    #[test]
    fn aggregate_duration_and_result_allowances_are_consumed_without_refund() {
        for result_budget in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, _source, _definition, mut policy) =
                super::super::automation_authorizations::tests::fixture(store.clone());
            if let PolicyCommand::Approve {
                max_reserved_execution_ms,
                max_reserved_result_bytes,
                ..
            } = &mut policy.creation
            {
                if result_budget {
                    *max_reserved_result_bytes = 1024;
                } else {
                    *max_reserved_execution_ms = 1000;
                }
            }
            policy.authorization_sha256 = policy.digest().unwrap();
            store
                .automation_policy_checkpoint(
                    policy.creation.clone(),
                    Some(policy.clone()),
                    &policy.runtime_id,
                    policy.approved_at_ms,
                )
                .unwrap();
            let first = fresh(&engine);
            save(&store, &policy, &first, &command(&policy, &first));
            engine.cancel(&first.plan_id).unwrap();
            let second = fresh(&engine);
            assert!(
                store
                    .occurrence_checkpoint(
                        command(&policy, &second),
                        &policy.runtime_id,
                        Some(&policy.issuer_id),
                        at(&policy, &second)
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("budget exhausted")
            );
            assert_eq!(store.usage().unwrap()["occurrence_count"], 1);
        }
    }
    #[test]
    fn current_policy_fence_rejects_time_runtime_issuer_revocation_and_supersession() {
        let store = JobStore::in_memory().unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        save(&store, &policy, &job, &command(&policy, &job));
        let baseline = json!(store.get(&job.plan_id).unwrap());
        for (runtime, issuer, now) in [
            (
                uuid::Uuid::new_v4().to_string(),
                policy.issuer_id.clone(),
                at(&policy, &job),
            ),
            (policy.runtime_id.clone(), "b".repeat(64), at(&policy, &job)),
            (
                policy.runtime_id.clone(),
                policy.issuer_id.clone(),
                policy.expires_at_ms,
            ),
            (
                policy.runtime_id.clone(),
                policy.issuer_id.clone(),
                policy.approved_at_ms - 1,
            ),
        ] {
            let connection = store.connection.lock().unwrap();
            let current = read(&connection, &job.plan_id).unwrap();
            assert!(fence(&connection, &current, &runtime, &issuer, || now).is_err());
            drop(connection);
            assert_eq!(json!(store.get(&job.plan_id).unwrap()), baseline);
        }
        store
            .automation_policy_checkpoint(
                PolicyCommand::Revoke {
                    id: policy.id.clone(),
                    revision: 1,
                    operation_id: uuid::Uuid::new_v4().to_string(),
                },
                None,
                &policy.runtime_id,
                at(&policy, &job),
            )
            .unwrap();
        assert!(
            store
                .edit_with_policy_fence(
                    &job.plan_id,
                    "revoked",
                    &policy.runtime_id,
                    &policy.issuer_id,
                    |_| Ok(false)
                )
                .is_err()
        );
        let mut next = policy.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        next.generation = 2;
        if let PolicyCommand::Approve {
            operation_id,
            generation,
            ..
        } = &mut next.creation
        {
            *operation_id = next.id.clone();
            *generation = 1;
        }
        next.authorization_sha256 = next.digest().unwrap();
        store
            .automation_policy_checkpoint(
                next.creation.clone(),
                Some(next.clone()),
                &next.runtime_id,
                next.approved_at_ms,
            )
            .unwrap();
        assert!(
            store
                .edit_with_policy_fence(
                    &job.plan_id,
                    "superseded",
                    &policy.runtime_id,
                    &policy.issuer_id,
                    |_| Ok(false)
                )
                .is_err()
        );
    }
    #[test]
    fn source_and_changed_scope_cannot_reserve_or_replace_an_existing_receipt() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, policy) = setup(store.clone());
        assert!(
            store
                .occurrence_checkpoint(
                    command(&policy, &source),
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &source)
                )
                .is_err()
        );
        let job = fresh(&engine);
        let mut cmd = command(&policy, &job);
        if let OccurrenceCommand::Reserve { plan_sha256, .. } = &mut cmd {
            *plan_sha256 = "b".repeat(64);
        }
        assert!(
            store
                .occurrence_checkpoint(
                    cmd,
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &job)
                )
                .is_err()
        );
        let cmd = command(&policy, &job);
        save(&store, &policy, &job, &cmd);
        let mut conflicting = cmd;
        if let OccurrenceCommand::Reserve { revision, .. } = &mut conflicting {
            *revision += 1;
        }
        assert!(
            store
                .occurrence_checkpoint(conflicting, &policy.runtime_id, None, at(&policy, &job))
                .is_err()
        );
        assert_eq!(store.usage().unwrap()["occurrence_count"], 1);
    }
    #[test]
    fn insertion_fault_rolls_back_job_approval_and_all_budget_consumption() {
        let store = JobStore::in_memory().unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        let original = json!(store.get(&job.plan_id).unwrap());
        let usage = store.usage().unwrap();
        let cmd = command(&policy, &job);
        store.connection.lock().unwrap().execute_batch("CREATE TRIGGER fixture_occurrence_fault BEFORE INSERT ON automation_occurrences BEGIN SELECT RAISE(FAIL,'isolated reservation insert fault'); END;").unwrap();
        assert!(
            store
                .occurrence_checkpoint(
                    cmd.clone(),
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &job)
                )
                .is_err()
        );
        assert_eq!(json!(store.get(&job.plan_id).unwrap()), original);
        assert_eq!(store.usage().unwrap(), usage);
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fixture_occurrence_fault")
            .unwrap();
        assert_eq!(
            save(&store, &policy, &job, &cmd)["reservation"]["ordinal"],
            1
        );
    }
    #[test]
    fn restart_retains_reservation_without_activation_and_future_rows_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        let result = save(&store, &policy, &job, &command(&policy, &job));
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        assert_eq!(
            json!(
                store
                    .automation_occurrence(result["reservation"]["id"].as_str().unwrap())
                    .unwrap()
                    .unwrap()
            ),
            result["reservation"]
        );
        assert!(
            store
                .edit_with_policy_fence(
                    &job.plan_id,
                    "old runtime",
                    &uuid::Uuid::new_v4().to_string(),
                    &policy.issuer_id,
                    |_| Ok(false)
                )
                .is_err()
        );
        drop(store);
        let db = Connection::open(&path).unwrap();
        let mut raw = result["reservation"].clone();
        raw["schema_version"] = json!(99);
        let raw = serde_json::to_string(&raw).unwrap();
        db.execute("UPDATE automation_occurrences SET record_json=?1", [&raw])
            .unwrap();
        drop(db);
        assert!(JobStore::open(&path, 20, 1024 * 1024).is_err());
        let db = Connection::open(&path).unwrap();
        let retained: String = db
            .query_row("SELECT record_json FROM automation_occurrences", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(retained, raw);
    }
    #[test]
    fn clock_change_or_expiry_before_commit_rolls_back_both_writes() {
        for expire in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, _source, policy) = setup(store.clone());
            let job = fresh(&engine);
            let cmd = command(&policy, &job);
            let now = at(&policy, &job);
            let before = json!(store.get(&job.plan_id).unwrap());
            let calls = std::cell::Cell::new(0);
            let result = store.occurrence_checkpoint_clock(
                cmd.clone(),
                &policy.runtime_id,
                Some(&policy.issuer_id),
                || {
                    let call = calls.get();
                    calls.set(call + 1);
                    if call == 0 {
                        now
                    } else if expire {
                        policy.expires_at_ms
                    } else {
                        now - 1
                    }
                },
            );
            assert!(result.unwrap_err().to_string().contains("rolled back"));
            assert_eq!(json!(store.get(&job.plan_id).unwrap()), before);
            assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
            assert_eq!(
                save(&store, &policy, &job, &cmd)["reservation"]["ordinal"],
                1
            );
        }
    }
    #[test]
    fn second_phase_byte_quota_failure_rolls_back_approval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
        let (engine, _source, policy) = setup(store.clone());
        let job = fresh(&engine);
        let cmd = command(&policy, &job);
        let before = store.usage().unwrap();
        let original = json!(store.get(&job.plan_id).unwrap());
        let (raw, updated, reserved): (String, i64, i64) = store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT plan_json,updated_ms,reserved_bytes FROM durable_jobs WHERE job_id=?1",
                [&job.plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let saved = save(&store, &policy, &job, &cmd);
        let record_bytes = serde_json::to_string(&saved["reservation"]).unwrap().len();
        let only_job_fits = used(&store.usage().unwrap()) - record_bytes;
        assert!(only_job_fits > used(&before));
        drop(engine);
        drop(store);
        // Rewind only this isolated test database to measure the second write's quota boundary.
        let db = Connection::open(&path).unwrap();
        db.execute("DELETE FROM automation_occurrences", [])
            .unwrap();
        db.execute(
            "UPDATE durable_jobs SET plan_json=?2,updated_ms=?3,reserved_bytes=?4 WHERE job_id=?1",
            params![job.plan_id, raw, updated, reserved],
        )
        .unwrap();
        drop(db);
        let store = JobStore::open(&path, 20, only_job_fits).unwrap();
        assert!(
            store
                .occurrence_checkpoint(
                    cmd,
                    &policy.runtime_id,
                    Some(&policy.issuer_id),
                    at(&policy, &job)
                )
                .unwrap_err()
                .to_string()
                .contains("rolled back")
        );
        assert_eq!(json!(store.get(&job.plan_id).unwrap()), original);
        assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
    }
}
