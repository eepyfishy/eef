//! Explicit owner approval in the existing job journal. No issuance or dispatch.
use crate::{
    request_planning::PlanningCommand,
    task::{Plan, PlanStatus},
};
use anyhow::{Context, Result, bail};
use eefn::grants::{ExecutionGrant, GrantLimits, GrantPolicy};
use serde::{Deserialize, Serialize};

/// A protocol bound on reusable authorization, not an automatic retry policy.
pub const MAX_APPROVAL_VALIDITY_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRecord {
    pub schema_version: u32,
    pub operation_id: String,
    pub source_revision: u64,
    pub plan_sha256: String,
    pub issuer_id: String,
    pub approved_at_ms: u64,
    pub valid_for_ms: u64,
    pub expires_at_ms: u64,
    pub limits: GrantLimits,
    pub approval_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<eefn::automation_occurrence::Delegation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<eefn::workload_leases::definition::Template>,
}
pub(crate) fn validate_options(
    plan_sha256: &str,
    valid_for_ms: u64,
    limits: &GrantLimits,
) -> Result<()> {
    if !hex_digest(plan_sha256)
        || !(1..=MAX_APPROVAL_VALIDITY_MS).contains(&valid_for_ms)
        || !(1..=eefn::grants::MAX_DURATION_MS).contains(&limits.max_execution_ms)
        || !(1024..=1024 * 1024).contains(&limits.max_result_bytes)
        || valid_for_ms < limits.max_execution_ms
    {
        bail!("Invalid approval digest, validity or execution/result bounds")
    }
    Ok(())
}
fn hex_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn digest(record: &ApprovalRecord) -> Result<String> {
    let mut value = serde_json::to_value(record)?;
    value.as_object_mut().unwrap().remove("approval_sha256");
    eefn::grants::definition_digest(&value)
}
impl ApprovalRecord {
    /// Local state/time only. Dispatch must additionally revalidate issuer,
    /// providers, permissions and durable issuance; this does not reserve work.
    pub fn active_at(&self, plan: &Plan, now: u64) -> bool {
        plan.status == PlanStatus::Approved
            && now >= self.approved_at_ms
            && now < self.expires_at_ms
    }
    pub(crate) fn confirms(&self, command: &PlanningCommand) -> bool {
        self.schema_version == 1
            && self.workload.is_none()
            && self.delegation.is_none()
            && matches!(command, PlanningCommand::Approve { operation_id, expected_revision, plan_sha256, valid_for_ms, limits, .. }
            if self.operation_id == *operation_id && self.source_revision == *expected_revision
            && self.plan_sha256 == *plan_sha256 && self.valid_for_ms == *valid_for_ms && self.limits == *limits)
    }
}

/// Validate the *shape* of each fixed operation with the node's exact contract.
/// Temporary validation values are never persisted, sent or returned as grants.
pub(crate) fn scope(plan: &Plan, index: usize, record: &ApprovalRecord) -> Result<ExecutionGrant> {
    if record.schema_version != 1 || record.workload.is_some() {
        bail!("Workload approval cannot authorize a v1 operation")
    }
    let task = &plan.tasks[index];
    let binding = &plan
        .planning
        .as_ref()
        .context("Missing compiled plan")?
        .bindings[index];
    let value = ExecutionGrant {
        schema_version: 1,
        grant_id: record.operation_id.clone(),
        session_id: record.operation_id.clone(),
        issuer_id: record.issuer_id.clone(),
        executor_node: binding.node_id.clone(),
        job_id: plan.plan_id.clone(),
        task_id: task.task_id.clone(),
        attempt: 1,
        plan_sha256: record.plan_sha256.clone(),
        request_context: plan
            .goal
            .request_context
            .clone()
            .context("Missing request origin")?,
        capability: task.capability.clone(),
        action: task.action.clone(),
        params: task.params.clone(),
        resource_id: binding.resource_id.clone(),
        model_id: binding.model_id.clone(),
        backend: binding.backend.clone(),
        model_role: task.constraints["model_role"].as_str().map(str::to_owned),
        priority: 0,
        limits: record.limits.clone(),
        reporting: "acceptance_and_completion".into(),
        issued_at_ms: record.approved_at_ms,
        expires_at_ms: record
            .approved_at_ms
            .checked_add(record.limits.max_execution_ms)
            .context("Approval time overflow")?,
    };
    if record.limits.max_execution_ms > task.timeout_ms {
        bail!("Approval exceeds compiled task deadline")
    }
    value.validate()?;
    Ok(value)
}
pub(crate) fn validate_retained(plan: &Plan) -> Result<()> {
    let Some(record) = &plan.approval else {
        if plan.status == PlanStatus::Approved {
            bail!("Approved state lacks an owner receipt")
        }
        return Ok(());
    };
    if record.schema_version == 2 || record.workload.is_some() {
        return crate::workload_jobs::validate_approval(plan);
    }
    let planning = plan
        .planning
        .as_ref()
        .context("Approval lacks a concrete plan")?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Approval lacks a request review")?;
    validate_options(&record.plan_sha256, record.valid_for_ms, &record.limits)?;
    eefn::job_commands::validate_operation_id(&record.operation_id)?;
    if let Some(delegation) = &record.delegation {
        delegation.validate()?;
        if delegation.reservation_id != record.operation_id {
            bail!("Delegation receipt differs from approval");
        }
    }
    if uuid::Uuid::parse_str(&record.operation_id)?.is_nil()
        || record.schema_version != 1
        || !hex_digest(&record.issuer_id)
        || (plan.execution.is_none()
            && !matches!(plan.status, PlanStatus::Approved | PlanStatus::Cancelled))
        || record.source_revision <= planning.source_revision
        || record.source_revision >= plan.revision
        || record.approved_at_ms < planning.compiled_at_ms
        || record.approved_at_ms == 0
        || record.approved_at_ms.checked_add(record.valid_for_ms) != Some(record.expires_at_ms)
        || record.plan_sha256 != planning.plan_sha256
        || record.operation_id == planning.operation_id
        || draft
            .proposals
            .iter()
            .any(|p| p.operation_id == record.operation_id)
        || draft
            .answers
            .iter()
            .any(|a| a.operation_id == record.operation_id)
        || !hex_digest(&record.approval_sha256)
        || digest(record)? != record.approval_sha256
    {
        bail!("Damaged owner approval; retained without activation")
    }
    for index in 0..plan.tasks.len() {
        scope(plan, index, record)?;
    }
    Ok(())
}

/// Pure checkpoint mutation, called under the journal/engine admission gates.
fn checkpoint(
    plan: &mut Plan,
    command: &PlanningCommand,
    record: ApprovalRecord,
    now: u64,
) -> Result<bool> {
    command.validate()?;
    if !record.confirms(command) {
        bail!("Approval receipt does not confirm this command")
    }
    if let Some(existing) = &plan.approval {
        if existing.confirms(command) {
            return Ok(false);
        }
        bail!("An owner approval is already retained; inspect/cancel it, never implicitly renew")
    }
    if plan.status != PlanStatus::AwaitingApproval
        || plan.revision != record.source_revision
        || plan
            .planning
            .as_ref()
            .is_none_or(|p| p.plan_sha256 != record.plan_sha256)
        || now < record.approved_at_ms
        || now >= record.expires_at_ms
    {
        bail!("Plan state/digest/revision or approval time changed before checkpoint")
    }
    plan.approval = Some(record);
    plan.status = PlanStatus::Approved;
    Ok(true)
}

pub(crate) async fn approve(
    runtime: &crate::Runtime,
    saved: Plan,
    command: &PlanningCommand,
) -> Result<(Plan, bool)> {
    if saved.goal.template == eefn::workload_leases::definition::TEMPLATE {
        bail!("Continuous sessions require explicit version-two workload approval")
    }
    let PlanningCommand::Approve {
        expected_revision,
        operation_id,
        plan_sha256,
        valid_for_ms,
        limits,
        ..
    } = command
    else {
        bail!("Expected an explicit owner approval")
    };
    // Exact retained retries are inspection, even after expiry/cancellation or
    // disconnect. They never update timestamps, renew authority or dispatch.
    if let Some(existing) = &saved.approval {
        if existing.confirms(command) {
            return Ok((saved, true));
        }
        bail!("An approval is already retained; inspect before requesting different work")
    }
    if saved.status != PlanStatus::AwaitingApproval
        || saved.revision != *expected_revision
        || saved
            .planning
            .as_ref()
            .is_none_or(|p| p.plan_sha256 != *plan_sha256)
    {
        bail!("Approval must name the exact awaiting plan revision/digest")
    }
    let now = eefn::protocol::now_ms();
    let mut record = ApprovalRecord {
        schema_version: 1,
        operation_id: operation_id.clone(),
        source_revision: *expected_revision,
        plan_sha256: plan_sha256.clone(),
        issuer_id: String::new(),
        approved_at_ms: now,
        valid_for_ms: *valid_for_ms,
        expires_at_ms: now
            .checked_add(*valid_for_ms)
            .context("Approval time overflow")?,
        limits: limits.clone(),
        approval_sha256: String::new(),
        delegation: None,
        workload: None,
    };
    let driver = crate::request_execution::Driver::new(runtime);
    for index in 0..saved.tasks.len() {
        let session = driver.session(&saved, index).await?;
        let issuer = session["issuer_id"]
            .as_str()
            .context("Missing grant issuer")?;
        if record.issuer_id.is_empty() {
            record.issuer_id = issuer.into();
        }
        if record.issuer_id != issuer {
            bail!("Executors do not share this approval issuer")
        }
        let policy: GrantPolicy = serde_json::from_value(session["limits"].clone())?;
        policy.validate()?;
        if limits.max_execution_ms > policy.max_duration_ms
            || limits.max_result_bytes > policy.max_result_bytes
        {
            bail!("Approval exceeds the selected node's advertised grant bounds")
        }
        scope(&saved, index, &record)?;
    }
    record.approval_sha256 = digest(&record)?;
    let mut candidate = saved.clone();
    candidate.approval = Some(record.clone());
    candidate.status = PlanStatus::Approved;
    candidate.revision = saved
        .revision
        .checked_add(1)
        .context("Revision exhausted")?;
    crate::request_planning::validate_retained(&candidate)?;
    let mut reused = false;
    let result = runtime.engine.edit_review_checkpoint(
        &saved.plan_id,
        "Owner approval saved; no grant issued or work dispatched",
        |plan| {
            let changed = checkpoint(plan, command, record, eefn::protocol::now_ms())?;
            reused = !changed;
            Ok(changed)
        },
    )?;
    Ok((result, reused))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{event::EventBus, jobs::JobStore, request_planning, task::TaskEngine};
    use serde_json::{Value, json};
    pub(crate) fn compiled(engine: &TaskEngine) -> Plan {
        let saved = request_planning::tests::review(engine);
        let candidate = request_planning::tests::compiled(&saved);
        engine
            .edit_review_plan(&saved.plan_id, |p| {
                p.goal = candidate.goal;
                p.tasks = candidate.tasks;
                p.planning = candidate.planning;
                p.status = PlanStatus::AwaitingApproval;
                Ok(true)
            })
            .unwrap()
    }
    pub(crate) fn command(plan: &Plan) -> PlanningCommand {
        PlanningCommand::Approve {
            id: plan.plan_id.clone(),
            expected_revision: plan.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
            valid_for_ms: 60000,
            limits: GrantLimits {
                max_execution_ms: 1000,
                max_result_bytes: 65536,
            },
        }
    }
    pub(crate) fn record(command: &PlanningCommand) -> ApprovalRecord {
        let PlanningCommand::Approve {
            expected_revision,
            operation_id,
            plan_sha256,
            valid_for_ms,
            limits,
            ..
        } = command
        else {
            panic!()
        };
        let now = eefn::protocol::now_ms();
        let mut r = ApprovalRecord {
            schema_version: 1,
            operation_id: operation_id.clone(),
            source_revision: *expected_revision,
            plan_sha256: plan_sha256.clone(),
            issuer_id: "a".repeat(64),
            approved_at_ms: now,
            valid_for_ms: *valid_for_ms,
            expires_at_ms: now + valid_for_ms,
            limits: limits.clone(),
            approval_sha256: String::new(),
            delegation: None,
            workload: None,
        };
        r.approval_sha256 = digest(&r).unwrap();
        r
    }
    pub(crate) fn save(
        engine: &TaskEngine,
        plan: &Plan,
        cmd: &PlanningCommand,
        r: ApprovalRecord,
    ) -> Result<Plan> {
        engine.edit_review_checkpoint(
            &plan.plan_id,
            "Owner approval saved; no grant issued",
            |p| checkpoint(p, cmd, r, eefn::protocol::now_ms()),
        )
    }
    #[tokio::test]
    async fn approval_survives_restart_without_dispatch_and_origin_can_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let compiled = compiled(&engine);
        let cmd = command(&compiled);
        let r = record(&cmd);
        let p = save(&engine, &compiled, &cmd, r.clone()).unwrap();
        assert_eq!(p.status, PlanStatus::Approved);
        assert_eq!(p.revision, 3);
        assert_eq!(
            p.planning.as_ref().unwrap().plan_sha256,
            compiled.planning.as_ref().unwrap().plan_sha256
        );
        assert!(p.approval.as_ref().unwrap().active_at(&p, r.approved_at_ms));
        assert!(!r.active_at(&p, r.expires_at_ms));
        assert!(!r.active_at(&p, r.approved_at_ms - 1));
        assert!(engine.run_plan(p.clone()).await.is_err());
        assert!(engine.enqueue(p.clone()).is_err());
        assert!(engine.enqueue_request(p.clone()).is_err());
        assert!(engine.resume_job(&p.plan_id).is_err());
        assert!(engine.pause(&p.plan_id).is_err());
        let view = crate::jobs::view(&p, true);
        assert_eq!(view["planning_state"], "approved_no_grant");
        assert!(view.get("approval").is_none());
        assert!(view.get("planning").is_none());
        let opening = p.request_review.as_ref().unwrap();
        let repeated = crate::request_drafts::open(
            &engine,
            json!({"schema_version":1,"original_text":opening.original_text,
            "proposal":opening.proposals[0].proposal}),
            p.goal.request_context.as_ref().unwrap(),
            &["system.info".into()],
        )
        .unwrap();
        assert_eq!(repeated["reused"], true);
        assert_eq!(repeated["status"], "approved");
        assert_eq!(repeated["revision"], 3);
        engine.shutdown().await;
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(json!(store.get(&p.plan_id).unwrap()), json!(p));
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        assert!(save(&engine, &p, &cmd, r.clone()).is_ok());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(p));
        let cancelled = engine.cancel(&p.plan_id).unwrap();
        assert_eq!(cancelled.status, PlanStatus::Cancelled);
        assert!(
            cancelled
                .tasks
                .iter()
                .all(|t| t.status == crate::task::TaskStatus::Cancelled
                    && t.attempts == 0
                    && t.assignments.is_empty())
        );
        assert_eq!(cancelled.approval, p.approval);
        assert!(!r.active_at(&cancelled, r.approved_at_ms));
        assert_eq!(
            json!(save(&engine, &cancelled, &cmd, r).unwrap()),
            json!(cancelled)
        );
        let mut new = cmd.clone();
        if let PlanningCommand::Approve { operation_id, .. } = &mut new {
            *operation_id = uuid::Uuid::new_v4().to_string();
        }
        assert!(save(&engine, &cancelled, &new, record(&new)).is_err());
    }
    #[test]
    fn strict_options_and_receipts_cannot_widen_or_manufacture_authority() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = compiled(&engine);
        let cmd = command(&p);
        cmd.validate().unwrap();
        let value = json!(cmd);
        for (key, bad) in [
            ("valid_for_ms", json!(0)),
            ("valid_for_ms", json!(86400001)),
            ("valid_for_ms", json!(999)),
            ("plan_sha256", json!("not-a-digest")),
            ("expected_revision", json!(0)),
            ("expected_revision", json!(u64::MAX)),
            ("operation_id", json!(uuid::Uuid::nil().to_string())),
        ] {
            let mut v = value.clone();
            v[key] = bad;
            let c: PlanningCommand = serde_json::from_value(v).unwrap();
            assert!(c.validate().is_err());
        }
        for bad in [
            json!({"max_execution_ms":0,"max_result_bytes":65536}),
            json!({"max_execution_ms":30001,"max_result_bytes":65536}),
            json!({"max_execution_ms":1000,"max_result_bytes":1}),
            json!({"max_execution_ms":1000,"max_result_bytes":1048577}),
        ] {
            let mut v = value.clone();
            v["limits"] = bad;
            assert!(
                serde_json::from_value::<PlanningCommand>(v)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut v = value;
        v["execute"] = json!(true);
        assert!(serde_json::from_value::<PlanningCommand>(v).is_err());
        let mut r = json!(record(&cmd));
        r["permissions"] = json!("all");
        assert!(serde_json::from_value::<ApprovalRecord>(r).is_err());
    }
    #[test]
    fn approval_checkpoints_fail_on_stale_cancelled_expired_or_changed_definition() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = compiled(&engine);
        let cmd = command(&p);
        let r = record(&cmd);
        for status in [
            PlanStatus::Review,
            PlanStatus::Cancelled,
            PlanStatus::Pending,
        ] {
            let mut bad = p.clone();
            bad.status = status;
            assert!(checkpoint(&mut bad, &cmd, r.clone(), r.approved_at_ms).is_err());
        }
        let mut bad = p.clone();
        bad.revision += 1;
        assert!(checkpoint(&mut bad, &cmd, r.clone(), r.approved_at_ms).is_err());
        let mut bad = p.clone();
        bad.planning.as_mut().unwrap().plan_sha256 = "b".repeat(64);
        assert!(checkpoint(&mut bad, &cmd, r.clone(), r.approved_at_ms).is_err());
        let mut bad = p.clone();
        assert!(checkpoint(&mut bad, &cmd, r.clone(), r.expires_at_ms).is_err());
        assert!(checkpoint(&mut bad, &cmd, r.clone(), r.approved_at_ms - 1).is_err());
        let cancelled = engine.cancel(&p.plan_id).unwrap();
        assert!(save(&engine, &cancelled, &cmd, r).is_err());
    }
    #[test]
    fn saved_approval_mutations_and_attempts_are_rejected_without_rebinding() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = compiled(&engine);
        let cmd = command(&p);
        let approved = save(&engine, &p, &cmd, record(&cmd)).unwrap();
        for (key, bad) in [
            ("plan_sha256", json!("b".repeat(64))),
            ("issuer_id", json!("b".repeat(64))),
            ("expires_at_ms", json!(1)),
            ("valid_for_ms", json!(1001)),
            ("source_revision", json!(1)),
            (
                "operation_id",
                json!(p.planning.as_ref().unwrap().operation_id),
            ),
            ("approval_sha256", json!("b".repeat(64))),
            (
                "limits",
                json!({"max_execution_ms":1000,"max_result_bytes":65537}),
            ),
        ] {
            let mut bad_plan = json!(approved);
            bad_plan["approval"][key] = bad;
            let changed: Plan = serde_json::from_value(bad_plan).unwrap();
            assert!(crate::jobs::validate_retained_review(&changed).is_err());
        }
        let mut bad = approved.clone();
        bad.approval = None;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = approved.clone();
        bad.status = PlanStatus::Pending;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = approved.clone();
        bad.tasks[0].attempts = 1;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = approved.clone();
        bad.tasks[0].params["authority"] = json!(true);
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = approved.clone();
        bad.status = PlanStatus::AwaitingApproval;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        // Even with a recalculated approval digest, schema/receipt/time constraints remain mandatory.
        for change in [0, 1, 2] {
            let mut bad = approved.clone();
            let r = bad.approval.as_mut().unwrap();
            match change {
                0 => r.operation_id = p.planning.as_ref().unwrap().operation_id.clone(),
                1 => r.issuer_id = "invalid".into(),
                _ => r.approved_at_ms = 1,
            }
            r.approval_sha256 = digest(r).unwrap();
            assert!(crate::jobs::validate_retained_review(&bad).is_err());
        }
    }
    #[test]
    fn concurrent_same_receipt_is_one_checkpoint_and_different_receipts_do_not_renew() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = compiled(&engine);
        let cmd = command(&p);
        let r = record(&cmd);
        let gate = std::sync::Arc::new(std::sync::Barrier::new(24));
        let workers = (0..24)
            .map(|_| {
                let engine = engine.clone();
                let p = p.clone();
                let cmd = cmd.clone();
                let r = r.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    save(&engine, &p, &cmd, r).unwrap()
                })
            })
            .collect::<Vec<_>>();
        for w in workers {
            assert_eq!(w.join().unwrap().revision, 3);
        }
        assert_eq!(engine.store.get(&p.plan_id).unwrap().history.len(), 3);
        let another = command(&p);
        assert!(save(&engine, &p, &another, record(&another)).is_err());
        assert_eq!(engine.store.get(&p.plan_id).unwrap().revision, 3);
        // Stop and approval share admission, never leaving a runnable/revived plan.
        for _ in 0..16 {
            let p = compiled(&engine);
            let cmd = command(&p);
            let r = record(&cmd);
            let e = engine.clone();
            let id = p.plan_id.clone();
            let stop = std::thread::spawn(move || e.cancel(&id).unwrap());
            let _ = save(&engine, &p, &cmd, r);
            stop.join().unwrap();
            let cancelled = engine.store.get(&p.plan_id).unwrap();
            assert_eq!(cancelled.status, PlanStatus::Cancelled);
            assert!(
                cancelled
                    .tasks
                    .iter()
                    .all(|t| t.attempts == 0 && t.assignments.is_empty())
            );
        }
    }
    #[tokio::test]
    async fn failed_quota_or_shutdown_keeps_the_previous_plan_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let p = compiled(&engine);
        let cmd = command(&p);
        let r = record(&cmd);
        let used = store.usage().unwrap()["payload_bytes"].as_u64().unwrap() as usize + 1024;
        engine.shutdown().await;
        assert!(save(&engine, &p, &cmd, r.clone()).is_err());
        assert_eq!(json!(store.get(&p.plan_id).unwrap()), json!(p));
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 500, used).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        assert!(save(&engine, &p, &cmd, r).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(p));
    }
    #[test]
    fn damaged_approval_journal_is_retained_not_repaired_or_activated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let p = compiled(&engine);
        let cmd = command(&p);
        let approved = save(&engine, &p, &cmd, record(&cmd)).unwrap();
        drop(engine);
        drop(store);
        let db = rusqlite::Connection::open(&path).unwrap();
        let mut bad = json!(approved);
        bad["approval"]["limits"]["max_result_bytes"] = json!(999999);
        let raw = serde_json::to_string(&bad).unwrap();
        db.execute(
            "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
            rusqlite::params![p.plan_id, raw],
        )
        .unwrap();
        assert!(JobStore::open(&path, 500, 64 * 1024 * 1024).is_err());
        let unchanged: String = db
            .query_row(
                "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
                [&p.plan_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, raw);
    }
    #[test]
    fn confirmation_binds_original_revision_digest_limits_and_receipt() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = compiled(&engine);
        let cmd = command(&p);
        let approved = save(&engine, &p, &cmd, record(&cmd)).unwrap();
        let mut value = json!({"schema_version":1,"report_type":"request_planning","success":true,"runtime_id":"runtime","id":p.plan_id,
            "operation_id":approved.approval.as_ref().unwrap().operation_id,"plan":approved,"reused":false,"execution_authorized":false,
            "dispatched":false,"automatically_retried":false,"note":"No issuance or execution"});
        assert!(request_planning::confirmed_report(&value, "runtime", &cmd));
        for (key, bad) in [
            ("dispatched", json!(true)),
            ("execution_authorized", json!(true)),
            ("automatically_retried", json!(true)),
            ("operation_id", json!(uuid::Uuid::new_v4().to_string())),
        ] {
            let mut bad_report = value.clone();
            bad_report[key] = bad;
            assert!(!request_planning::confirmed_report(
                &bad_report,
                "runtime",
                &cmd
            ));
        }
        let mut wrong = json!(cmd);
        wrong["limits"]["max_result_bytes"] = json!(65535);
        let wrong: PlanningCommand = serde_json::from_value(wrong).unwrap();
        assert!(!request_planning::confirmed_report(
            &value, "runtime", &wrong
        ));
        value["plan"]["permission_override"] = Value::Bool(true);
        assert!(!request_planning::confirmed_report(&value, "runtime", &cmd));
    }
}
