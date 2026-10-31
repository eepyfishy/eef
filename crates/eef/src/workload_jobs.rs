//! Reviewed session issuance in the existing JobStore/AttemptJournal model.
//! Planning and conditional approval have owner command callers. Issuance remains
//! internal until authenticated source execution and termination are admitted.
use crate::{
    jobs::{Assignment, JobStore},
    request_approval::ApprovalRecord,
    request_execution::{AttemptState, ExecutionRecord},
    task::{Plan, PlanStatus, TaskStatus},
};
use anyhow::{Context, Result, bail, ensure};
use eefn::{
    grants::GrantLimits,
    request_execution::ExecuteCommand,
    workload_leases::{
        IssuerWindow, Policy, Query, Reply,
        definition::{CAPABILITY, Definition, TEMPLATE, Template},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[path = "workload_job_completion.rs"]
mod completion;
pub use completion::Completion;
pub(crate) const COMPLETION_RESERVE: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalCommand {
    pub id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub plan_sha256: String,
    pub valid_for_ms: u64,
}
impl ApprovalCommand {
    pub fn validate(&self) -> Result<()> {
        eefn::request_drafts::validate_id(&self.id)?;
        eefn::automation_occurrence::uuid(&self.operation_id)?;
        ensure!(
            self.expected_revision > 0
                && self.expected_revision < u64::MAX
                && hex(&self.plan_sha256)
                && (1..=crate::request_approval::MAX_APPROVAL_VALIDITY_MS)
                    .contains(&self.valid_for_ms),
            "Invalid exact session approval revision, digest or validity"
        );
        Ok(())
    }
    pub(crate) fn confirms(&self, approval: &ApprovalRecord) -> bool {
        approval.schema_version == 2
            && approval.operation_id == self.operation_id
            && approval.source_revision == self.expected_revision
            && approval.plan_sha256 == self.plan_sha256
            && approval.valid_for_ms == self.valid_for_ms
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub definition: Definition,
    pub issued_at_ms: u64,
    pub state: AttemptState,
    pub acknowledgement: Option<eefn::grants::workload::Receipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<Completion>,
}
pub(crate) struct Snapshot {
    immutable: serde_json::Value,
    acknowledgement: Option<serde_json::Value>,
    completion: Option<serde_json::Value>,
    state: AttemptState,
}
pub(crate) fn snapshot(plan: &Plan) -> Option<Snapshot> {
    let e = plan.execution.as_ref()?;
    let w = e.workload.as_ref()?;
    Some(Snapshot {
        immutable: json!({"definition":w.definition,"issued_at_ms":w.issued_at_ms,
        "command":e.command,"started_at_ms":e.started_at_ms}),
        acknowledgement: w.acknowledgement.as_ref().map(|a| json!(a)),
        completion: w.completion.as_ref().map(|c| json!(c)),
        state: w.state,
    })
}
pub(crate) fn preserve(old: &Option<Snapshot>, plan: &Plan) -> Result<()> {
    let Some(old) = old else { return Ok(()) };
    let new = snapshot(plan).context("Retained workload issuance cannot be erased")?;
    ensure!(
        old.immutable == new.immutable
            && (old.acknowledgement.is_none() || old.acknowledgement == new.acknowledgement)
            && (old.completion.is_none() || old.completion == new.completion),
        "Retained workload identity/ACK/completion is immutable"
    );
    ensure!(
        match old.state {
            AttemptState::Issued => matches!(
                new.state,
                AttemptState::Issued | AttemptState::Accepted | AttemptState::Unknown
            ),
            AttemptState::Accepted => matches!(
                new.state,
                AttemptState::Accepted | AttemptState::Completed | AttemptState::Unknown
            ),
            AttemptState::Unknown => new.state == AttemptState::Unknown,
            AttemptState::Completed => new.state == AttemptState::Completed,
            _ => false,
        },
        "Retired workload cannot return to active state"
    );
    Ok(())
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn template(plan: &Plan) -> Result<Template> {
    ensure!(
        plan.goal.template == TEMPLATE && plan.tasks.len() == 1,
        "Not one reviewed workload session"
    );
    Template::from_params(&plan.goal.params)
}
fn mirror(w: &Template) -> GrantLimits {
    // These schema-two fields mirror the short window/reporting budget for
    // compatibility of the common record. scope() refuses them as v1 authority.
    GrantLimits {
        max_execution_ms: w.limits.max_lease_ms,
        max_result_bytes: w.events.max_total_bytes,
    }
}
fn unique_operation(plan: &Plan, id: &str) -> Result<()> {
    eefn::automation_occurrence::uuid(id)?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Workload lacks review")?;
    ensure!(
        !draft.proposals.iter().any(|p| p.operation_id == id)
            && !draft.answers.iter().any(|a| a.operation_id == id)
            && plan.planning.as_ref().is_none_or(|p| p.operation_id != id),
        "Workload operation identity reused"
    );
    Ok(())
}
pub(crate) fn validate_approval(plan: &Plan) -> Result<()> {
    let a = plan
        .approval
        .as_ref()
        .context("Workload approval unavailable")?;
    let p = plan
        .planning
        .as_ref()
        .context("Workload lacks concrete planning")?;
    let w = template(plan)?;
    unique_operation(plan, &a.operation_id)?;
    ensure!(
        a.schema_version == 2
            && a.workload.as_ref() == Some(&w)
            && a.delegation.is_none()
            && p.schema_version == 2
            && a.plan_sha256 == p.plan_sha256
            && hex(&a.issuer_id)
            && a.limits == mirror(&w)
            && a.source_revision > p.source_revision
            && a.source_revision < plan.revision
            && a.approved_at_ms >= p.compiled_at_ms
            && a.approved_at_ms > 0
            && (w.limits.max_session_ms..=crate::request_approval::MAX_APPROVAL_VALIDITY_MS)
                .contains(&a.valid_for_ms)
            && a.approved_at_ms.checked_add(a.valid_for_ms) == Some(a.expires_at_ms)
            && crate::request_approval::digest(a)? == a.approval_sha256
            && (plan.execution.is_some()
                || matches!(plan.status, PlanStatus::Approved | PlanStatus::Cancelled)),
        "Damaged workload approval; no v1 fallback or activation"
    );
    Ok(())
}
pub(crate) fn validate_execution(plan: &Plan) -> Result<()> {
    let e = plan
        .execution
        .as_ref()
        .context("Missing workload execution")?;
    let w = e.workload.as_ref().context("Missing workload issuance")?;
    let a = plan
        .approval
        .as_ref()
        .context("Missing workload approval")?;
    let t = template(plan)?;
    let task = &plan.tasks[0];
    w.definition.validate()?;
    e.command.validate()?;
    unique_operation(plan, &e.command.operation_id)?;
    let expected_schema = if w.completion.is_some() { 3 } else { 2 };
    ensure!(
        e.schema_version == expected_schema
            && e.attempts.is_empty()
            && e.peer.is_none()
            && e.abandonment.is_none()
            && e.stop_requested_at_ms.is_none()
            && e.command.id == plan.plan_id
            && e.command.approval_sha256 == a.approval_sha256
            && e.command.operation_id != a.operation_id
            && e.command.expected_revision > a.source_revision
            && e.command.expected_revision < plan.revision
            && e.started_at_ms == w.issued_at_ms
            && w.issued_at_ms >= a.approved_at_ms
            && w.issued_at_ms
                .checked_add(t.limits.max_session_ms)
                .is_some_and(|end| end <= a.expires_at_ms)
            && w.definition.template() == t
            && w.definition.plan_sha256 == a.plan_sha256
            && w.definition.issuer_id == a.issuer_id
            && w.definition.job_id == plan.plan_id
            && w.definition.task_id == task.task_id
            && w.definition.attempt == 1
            && Some(&w.definition.request_context) == plan.goal.request_context.as_ref()
            && w.definition.grant_id != e.command.operation_id
            && w.definition.grant_id != a.operation_id
            && task.capability == CAPABILITY
            && task.attempts == 1
            && task.result.is_none()
            && plan.results.is_empty()
            && task.assignments.len() == 1,
        "Workload issuance differs from its exact approved plan"
    );
    unique_operation(plan, &w.definition.grant_id)?;
    let assignment = &task.assignments[0];
    ensure!(
        assignment.attempt == 1
            && assignment.node_id == t.executor_node
            && assignment.resource_id.as_deref() == Some(t.resource_id())
            && assignment.model_id.is_none()
            && assignment.backend.is_none()
            && assignment.assigned_at_ms == w.issued_at_ms,
        "Workload assignment changed"
    );
    if let Some(ack) = &w.acknowledgement {
        ack.validate()?;
        ensure!(
            ack.definition == w.definition && ack.receipt.state == "accepted",
            "Wrong workload consumption ACK"
        );
    }
    if let Some(completion) = &w.completion {
        ensure!(
            completion.recorded_at_ms <= plan.updated_at_ms,
            "Completion is newer than its coordinator checkpoint"
        );
        completion.validate(
            &w.definition,
            w.acknowledgement
                .as_ref()
                .context("Completion lacks consumed ACK")?,
            w.issued_at_ms,
        )?;
    }
    ensure!(
        match w.state {
            AttemptState::Issued =>
                w.acknowledgement.is_none()
                    && w.completion.is_none()
                    && plan.status == PlanStatus::Running
                    && task.status == TaskStatus::Running,
            AttemptState::Accepted =>
                w.acknowledgement.is_some()
                    && w.completion.is_none()
                    && plan.status == PlanStatus::Running
                    && task.status == TaskStatus::Running,
            AttemptState::Unknown =>
                w.completion.is_none()
                    && plan.status == PlanStatus::Interrupted
                    && task.status == TaskStatus::Interrupted,
            AttemptState::Completed =>
                w.acknowledgement.is_some()
                    && w.completion.is_some()
                    && plan.status == PlanStatus::Completed
                    && task.status == TaskStatus::Completed,
            _ => false,
        },
        "Invalid workload lifecycle; retained evidence cannot resume work"
    );
    Ok(())
}

/// Internal review compiler only. Actual runtime compilation must additionally
/// verify disclosed, fresh authenticated node/resource metadata before using it.
pub(crate) fn compile(
    engine: &crate::task::TaskEngine,
    command: &crate::request_planning::PlanningCommand,
) -> Result<Plan> {
    let store = &engine.store;
    use crate::request_planning::{PlannedBinding, PlanningCommand, PlanningRecord};
    let PlanningCommand::Compile {
        id,
        expected_revision,
        operation_id,
        input,
    } = command
    else {
        bail!("Expected workload compile")
    };
    command.validate()?;
    ensure!(input.template == TEMPLATE, "Not a workload template");
    let saved = store.get(id)?;
    if let Some(p) = &saved.planning {
        ensure!(
            p.operation_id == *operation_id
                && p.source_revision == *expected_revision
                && p.input == *input,
            "Conflicting retained workload plan"
        );
        return Ok(saved);
    }
    ensure!(
        saved.revision == *expected_revision
            && saved.status == PlanStatus::Review
            && saved
                .request_review
                .as_ref()
                .is_some_and(|r| r.current().workload
                    == eefn::interpretation::Workload::Continuous
                    && !r.current().needs_clarification),
        "Workload review/revision changed"
    );
    let mut compiled = crate::request_planning::unbound_plan(&saved, input)?;
    let w = Template::from_params(&input.params)?;
    compiled.plan_id = saved.plan_id.clone();
    compiled.request_review = saved.request_review.clone();
    compiled.status = PlanStatus::AwaitingApproval;
    compiled.revision = saved
        .revision
        .checked_add(1)
        .context("Revision exhausted")?;
    let mut record = PlanningRecord {
        schema_version: 2,
        operation_id: operation_id.clone(),
        source_revision: *expected_revision,
        proposal_index: saved.request_review.as_ref().unwrap().proposals.len() - 1,
        compiled_at_ms: eefn::protocol::now_ms(),
        input: input.clone(),
        bindings: vec![PlannedBinding {
            task_id: compiled.tasks[0].task_id.clone(),
            node_id: w.executor_node.clone(),
            resource_id: Some(w.resource_id().into()),
            model_id: None,
            backend: None,
        }],
        plan_sha256: String::new(),
        preparation: None,
    };
    record.plan_sha256 = crate::request_planning::digest(&compiled, &record)?;
    compiled.planning = Some(record);
    engine.edit_review_checkpoint(
        id,
        "Exact continuous source plan retained; no approval or dispatch",
        |plan| {
            ensure!(
                plan.revision == *expected_revision
                    && plan.status == PlanStatus::Review
                    && plan.goal.request_context == saved.goal.request_context
                    && json!(plan.request_review) == json!(saved.request_review),
                "Workload review changed before checkpoint"
            );
            plan.goal = compiled.goal;
            plan.tasks = compiled.tasks;
            plan.planning = compiled.planning;
            plan.status = PlanStatus::AwaitingApproval;
            Ok(true)
        },
    )
}
pub(crate) fn approve(
    engine: &crate::task::TaskEngine,
    command: &ApprovalCommand,
    issuer: &str,
) -> Result<Plan> {
    command.validate()?;
    let saved = engine.store.get(&command.id)?;
    let w = template(&saved)?;
    if let Some(a) = &saved.approval {
        ensure!(
            command.confirms(a) && a.issuer_id == issuer,
            "Conflicting workload approval; no implicit renewal"
        );
        return Ok(saved);
    }
    unique_operation(&saved, &command.operation_id)?;
    ensure!(
        hex(issuer)
            && saved.status == PlanStatus::AwaitingApproval
            && saved.revision == command.expected_revision
            && saved
                .planning
                .as_ref()
                .is_some_and(|p| p.plan_sha256 == command.plan_sha256)
            && (w.limits.max_session_ms..=crate::request_approval::MAX_APPROVAL_VALIDITY_MS)
                .contains(&command.valid_for_ms),
        "Workload approval exceeds or differs from its reviewed definition"
    );
    let now = eefn::protocol::now_ms();
    let mut a = ApprovalRecord {
        schema_version: 2,
        operation_id: command.operation_id.clone(),
        source_revision: command.expected_revision,
        plan_sha256: command.plan_sha256.clone(),
        issuer_id: issuer.into(),
        approved_at_ms: now,
        valid_for_ms: command.valid_for_ms,
        expires_at_ms: now
            .checked_add(command.valid_for_ms)
            .context("Approval time overflow")?,
        limits: mirror(&w),
        approval_sha256: String::new(),
        delegation: None,
        workload: Some(w),
    };
    a.approval_sha256 = crate::request_approval::digest(&a)?;
    engine.edit_review_checkpoint(
        &command.id,
        "Explicit workload session approval saved; no issuance",
        |plan| {
            ensure!(
                plan.status == PlanStatus::AwaitingApproval
                    && plan.revision == command.expected_revision
                    && plan
                        .planning
                        .as_ref()
                        .is_some_and(|p| p.plan_sha256 == command.plan_sha256)
                    && plan.approval.is_none(),
                "Workload approval source changed"
            );
            plan.approval = Some(a);
            plan.status = PlanStatus::Approved;
            Ok(true)
        },
    )
}

/// Original monotonic issuer ownership. Never serialized or reconstructed from
/// a retained job. Dropping it retains uncertainty and cannot resume a session.
pub struct Issued {
    definition: Definition,
    store: Arc<JobStore>,
    window: IssuerWindow,
    issued: Instant,
    issued_wall: u64,
    deadline: Instant,
    acknowledged: bool,
    retired: bool,
}
fn mark_unknown(store: &JobStore, definition: &Definition) -> Result<()> {
    store.edit(
        &definition.job_id,
        "Workload issuer retired; effects unknown, no replay",
        |plan| {
            let e = plan
                .execution
                .as_mut()
                .context("Missing workload issuance")?;
            let w = e.workload.as_mut().context("Missing workload issuance")?;
            ensure!(
                w.definition == *definition,
                "Workload issuance scope changed"
            );
            if w.state == AttemptState::Unknown {
                return Ok(false);
            }
            ensure!(
                matches!(w.state, AttemptState::Issued | AttemptState::Accepted)
                    && plan.status == PlanStatus::Running,
                "Workload no longer owns an active issuer"
            );
            w.state = AttemptState::Unknown;
            plan.status = PlanStatus::Interrupted;
            plan.tasks[0].status = TaskStatus::Interrupted;
            Ok(true)
        },
    )?;
    Ok(())
}
impl Drop for Issued {
    fn drop(&mut self) {
        self.retire();
    }
}
impl Issued {
    pub fn definition(&self) -> &Definition {
        &self.definition
    }
    fn retire(&mut self) {
        if self.retired {
            return;
        }
        // Latch before synchronous persistence. A failed uncertainty checkpoint
        // must not restore authority, and Drop must not retry that write.
        self.retired = true;
        self.window.stop();
        let _ = mark_unknown(&self.store, &self.definition);
    }
    fn checked_live(&mut self) -> Result<Plan> {
        let result = self.live();
        if result.is_err() {
            self.retire();
        }
        result
    }
    fn live(&self) -> Result<Plan> {
        ensure!(!self.retired, "Workload issuer permanently retired");
        let expected = self
            .issued_wall
            .saturating_add(self.issued.elapsed().as_millis().min(u64::MAX as u128) as u64);
        ensure!(
            Instant::now() < self.deadline && eefn::protocol::now_ms().abs_diff(expected) <= 1000,
            "Workload issuer expired or local clock changed"
        );
        let saved = self.store.get(&self.definition.job_id)?;
        let w = saved
            .execution
            .as_ref()
            .and_then(|e| e.workload.as_ref())
            .context("Workload issuance missing")?;
        ensure!(
            saved.status == PlanStatus::Running
                && matches!(w.state, AttemptState::Issued | AttemptState::Accepted)
                && w.definition == self.definition,
            "Workload issuance retired or changed"
        );
        Ok(saved)
    }
    /// The caller must supply identities from the pinned authenticated peer,
    /// not from fields copied out of this untrusted probe.
    pub fn lease(
        &mut self,
        query: &Query,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Reply> {
        self.checked_live()?;
        let reply = self.window.reply(query, issuer, session, node)?;
        self.checked_live()?;
        Ok(reply)
    }
    /// Called only with a report received through that same authenticated node
    /// session. Durable ACK precedes renewal authority; no broadcast event hook.
    pub fn acknowledge(&mut self, ack: eefn::grants::workload::Receipt) -> Result<()> {
        self.checked_live()?;
        ack.validate()?;
        ensure!(
            ack.definition == self.definition && ack.receipt.state == "accepted",
            "Wrong workload ACK scope/state"
        );
        if self.acknowledged {
            let saved = self.checked_live()?;
            ensure!(
                saved
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .acknowledgement
                    .as_ref()
                    .is_some_and(|prior| json!(prior) == json!(ack)),
                "Conflicting retained workload ACK"
            );
            return Ok(());
        }
        let result = (|| -> Result<()> {
            self.window.check_consumed(&self.definition.scope()?)?;
            self.store.change(
                &self.definition.job_id,
                "Exact workload consumption ACK saved before renewals",
                |plan| {
                    let w = plan
                        .execution
                        .as_mut()
                        .and_then(|e| e.workload.as_mut())
                        .context("Workload issuance missing")?;
                    ensure!(
                        plan.status == PlanStatus::Running
                            && w.state == AttemptState::Issued
                            && w.definition == self.definition
                            && w.acknowledgement.is_none(),
                        "Workload state changed before ACK checkpoint"
                    );
                    w.acknowledgement = Some(ack);
                    w.state = AttemptState::Accepted;
                    Ok(())
                },
            )?;
            self.checked_live()?;
            self.window
                .acknowledge_consumed(&self.definition.scope()?)?;
            self.acknowledged = true;
            Ok(())
        })();
        if result.is_err() {
            self.retire();
        }
        result
    }
}
/// Internal issuance only. Product callers must hold the existing engine's
/// admission/quiescence gate and verify current authenticated node capabilities.
/// No route invokes this API yet and it sends no grant/probe by itself.
#[allow(dead_code)]
pub(crate) fn issue(
    store: &Arc<JobStore>,
    command: &ExecuteCommand,
    session: &str,
    policy: &Policy,
) -> Result<Issued> {
    command.validate()?;
    eefn::automation_occurrence::uuid(session)?;
    let saved = store.get(&command.id)?;
    let w = template(&saved)?;
    let a = saved
        .approval
        .as_ref()
        .context("Workload approval missing")?;
    validate_approval(&saved)?;
    unique_operation(&saved, &command.operation_id)?;
    let issued = Instant::now();
    let now = eefn::protocol::now_ms();
    ensure!(
        saved.execution.is_none()
            && saved.status == PlanStatus::Approved
            && saved.revision == command.expected_revision
            && a.approval_sha256 == command.approval_sha256
            && command.operation_id != a.operation_id
            && a.active_at(&saved, now)
            && now
                .checked_add(w.limits.max_session_ms)
                .is_some_and(|end| end <= a.expires_at_ms),
        "Workload source/approval/validity changed before issuance"
    );
    let deadline = issued
        .checked_add(Duration::from_millis(w.limits.max_session_ms))
        .context("Session deadline overflow")?;
    let definition = Definition {
        schema_version: 2,
        grant_id: uuid::Uuid::new_v4().to_string(),
        issuer_id: a.issuer_id.clone(),
        session_id: session.into(),
        executor_node: w.executor_node.clone(),
        job_id: saved.plan_id.clone(),
        task_id: saved.tasks[0].task_id.clone(),
        attempt: 1,
        plan_sha256: a.plan_sha256.clone(),
        request_context: saved
            .goal
            .request_context
            .clone()
            .context("Missing origin")?,
        priority: w.priority,
        source: w.source.clone(),
        limits: w.limits.clone(),
        events: w.events.clone(),
    };
    let contract = definition.lease_contract()?;
    // Validate policy before any durable mutation; keep the ORIGINAL deadline.
    let _ = IssuerWindow::new(contract.clone(), policy, deadline)?;
    store.change(
        &command.id,
        "Exact workload definition issued durably before any network offer",
        |plan| {
            ensure!(
                plan.status == PlanStatus::Approved
                    && plan.execution.is_none()
                    && plan.revision == command.expected_revision
                    && plan
                        .approval
                        .as_ref()
                        .is_some_and(|r| r.approval_sha256 == command.approval_sha256),
                "Concurrent workload issuance/source change"
            );
            plan.execution = Some(ExecutionRecord {
                schema_version: 2,
                command: command.clone(),
                started_at_ms: now,
                attempts: vec![],
                abandonment: None,
                peer: None,
                stop_requested_at_ms: None,
                workload: Some(Execution {
                    definition: definition.clone(),
                    issued_at_ms: now,
                    state: AttemptState::Issued,
                    acknowledgement: None,
                    completion: None,
                }),
            });
            plan.status = PlanStatus::Running;
            let task = &mut plan.tasks[0];
            task.status = TaskStatus::Running;
            task.attempts = 1;
            task.assignments.push(Assignment {
                attempt: 1,
                node_id: w.executor_node.clone(),
                resource_id: Some(w.resource_id().into()),
                model_id: None,
                backend: None,
                assigned_at_ms: now,
            });
            Ok(())
        },
    )?;
    let window = match IssuerWindow::new(contract, policy, deadline) {
        Ok(window) => window,
        Err(error) => {
            let _ = mark_unknown(store, &definition);
            return Err(error);
        }
    };
    let mut token = Issued {
        definition,
        store: store.clone(),
        window,
        issued,
        issued_wall: now,
        deadline,
        acknowledged: false,
        retired: false,
    };
    token.checked_live()?;
    Ok(token)
}

#[cfg(test)]
#[path = "workload_jobs_tests.rs"]
pub(crate) mod tests;
