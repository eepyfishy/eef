//! Reviewed plans use durable issuance and a direct journal observer, never legacy RPC.
use crate::{
    jobs::JobStore,
    task::{Plan, PlanStatus, TaskEngine, TaskResult, TaskStatus},
};
use anyhow::{Context, Result, bail};
use eefn::{
    grants::{ExecutionGrant, GrantObserver, GrantReport, ReceiptState},
    request_execution::{ExecuteCommand, ExecuteReport},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Issued,
    Accepted,
    Completed,
    Rejected,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantedAttempt {
    pub grant: ExecutionGrant,
    pub state: AttemptState,
    pub acknowledgement: Option<GrantReport>,
    pub report: Option<GrantReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<eefn::request_recovery::RecoveryRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<crate::execution_stop::Delivery>,
}
impl GrantedAttempt {
    pub(crate) fn final_report(&self) -> Option<&GrantReport> {
        self.recovery
            .as_ref()
            .and_then(|r| r.result.report.as_ref())
            .or(self.report.as_ref())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    pub schema_version: u32,
    pub command: ExecuteCommand,
    pub started_at_ms: u64,
    pub attempts: Vec<GrantedAttempt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandonment: Option<eefn::request_abandon::AbandonRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<crate::peer_jobs::Issuance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_requested_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<crate::workload_jobs::Execution>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: ExecuteCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptInspectionRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: eefn::request_receipt::ReceiptCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonmentRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: eefn::request_abandon::AbandonCommand,
}
pub(crate) fn abandon(
    p: &mut Plan,
    command: &eefn::request_abandon::AbandonCommand,
    origin: Option<&str>,
    unavailable: bool,
    now: u64,
) -> Result<bool> {
    command.validate()?;
    let attempt = receipt_attempt(
        p,
        &eefn::request_receipt::ReceiptCommand {
            id: command.id.clone(),
            grant_id: command.grant_id.clone(),
        },
        origin,
    )?;
    let actor = if origin.is_some() {
        eefn::request_abandon::AbandonActor::OriginNode
    } else {
        eefn::request_abandon::AbandonActor::CoordinatorOwner
    };
    let execution = p.execution.as_ref().context("No reviewed execution")?;
    if execution.peer.is_some() {
        bail!(
            "Paired tracking closure needs explicit joint uncertainty handling; inspect both receipts"
        )
    }
    if let Some(existing) = &execution.abandonment {
        if existing.command == *command && existing.actor == actor {
            return Ok(false);
        }
        bail!("Abandonment already retained; inspect it without replacing the receipt")
    }
    if unavailable
        || p.status != PlanStatus::Interrupted
        || p.revision != command.expected_revision
        || attempt.state != AttemptState::Unknown
        || now < p.updated_at_ms
        || now < attempt.grant.issued_at_ms
        || operation_used(p, &command.operation_id)
    {
        bail!(
            "Abandonment requires the exact idle interrupted revision/uncertain grant and a new receipt; no job change"
        )
    }
    p.execution.as_mut().unwrap().abandonment = Some(eefn::request_abandon::AbandonRecord {
        schema_version: 1,
        command: command.clone(),
        actor,
        decided_at_ms: now,
    });
    cancel_pending(p);
    p.status = PlanStatus::Abandoned;
    Ok(true)
}
pub(crate) fn operation_used(p: &Plan, id: &str) -> bool {
    p.execution.as_ref().is_some_and(|e| {
        e.command.operation_id == id || e.attempts.iter().any(|a| a.grant.grant_id == id)
    }) || p.approval.as_ref().is_some_and(|a| a.operation_id == id)
        || p.planning.as_ref().is_some_and(|a| a.operation_id == id)
        || p.request_review.as_ref().is_some_and(|r| {
            r.proposals.iter().any(|p| p.operation_id == id)
                || r.answers.iter().any(|a| a.operation_id == id)
        })
}
pub(crate) fn receipt_attempt<'a>(
    plan: &'a Plan,
    command: &eefn::request_receipt::ReceiptCommand,
    origin: Option<&str>,
) -> Result<&'a GrantedAttempt> {
    command.validate()?;
    if plan.plan_id != command.id
        || origin.is_some_and(|node| {
            plan.goal
                .request_context
                .as_ref()
                .is_none_or(|c| c.origin_node() != node)
        })
    {
        bail!("Request not found for this origin")
    }
    validate_retained(plan)?;
    plan.execution
        .as_ref()
        .and_then(|e| {
            e.attempts
                .iter()
                .find(|a| a.grant.grant_id == command.grant_id)
        })
        .context("Issued attempt not found for this request")
}
pub(crate) fn reserved_bytes(plan: &Plan) -> usize {
    if let Some(w) = plan.execution.as_ref().and_then(|e| e.workload.as_ref()) {
        return match w.state {
            AttemptState::Issued => w
                .definition
                .events
                .max_total_bytes
                .saturating_add(2 * 65536 + 8192 + crate::workload_jobs::COMPLETION_RESERVE),
            AttemptState::Accepted => w
                .definition
                .events
                .max_total_bytes
                .saturating_add(8192 + crate::workload_jobs::COMPLETION_RESERVE),
            _ => 0,
        };
    }
    plan.execution
        .as_ref()
        .into_iter()
        .flat_map(|e| &e.attempts)
        .filter(|a| matches!(a.state, AttemptState::Issued | AttemptState::Accepted))
        .map(|a| {
            a.grant.limits.max_result_bytes
                + if a.state == AttemptState::Issued {
                    32768
                } else {
                    16384
                }
        })
        .sum()
}
pub(crate) fn interrupt(plan: &mut Plan) {
    if let Some(e) = &mut plan.execution {
        if let Some(w) = &mut e.workload {
            if matches!(w.state, AttemptState::Issued | AttemptState::Accepted) {
                w.state = AttemptState::Unknown;
            }
        }
        for a in &mut e.attempts {
            if matches!(a.state, AttemptState::Issued | AttemptState::Accepted) {
                a.state = AttemptState::Unknown;
            }
        }
    }
}
pub(crate) fn can_settle_unissued_stop(plan: &Plan) -> bool {
    plan.status == PlanStatus::Stopping
        && plan.execution.as_ref().is_some_and(|e| {
            e.workload.is_none()
                && e.attempts
                    .iter()
                    .all(|a| matches!(a.state, AttemptState::Completed | AttemptState::Rejected))
        })
}
pub(crate) fn settle_unissued_stop(plan: &mut Plan) {
    plan.status = PlanStatus::Cancelled;
    cancel_pending(plan);
}
pub(crate) fn validate_retained(plan: &Plan) -> Result<()> {
    if plan
        .execution
        .as_ref()
        .is_some_and(|e| e.schema_version == 2 || e.workload.is_some())
    {
        return crate::workload_jobs::validate_execution(plan);
    }
    crate::execution_stop::validate(plan)?;
    let Some(e) = &plan.execution else {
        return Ok(());
    };
    let paired = crate::peer_jobs::validate_retained(plan)?;
    e.command.validate()?;
    let approval = plan
        .approval
        .as_ref()
        .context("Execution lacks owner approval")?;
    let planning = plan
        .planning
        .as_ref()
        .context("Execution lacks concrete plan")?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Execution lacks review")?;
    if e.schema_version != 1
        || e.command.id != plan.plan_id
        || e.command.approval_sha256 != approval.approval_sha256
        || e.command.expected_revision <= approval.source_revision
        || e.command.expected_revision >= plan.revision
        || e.started_at_ms < approval.approved_at_ms
        || e.started_at_ms >= approval.expires_at_ms
        || e.command.operation_id == approval.operation_id
        || e.command.operation_id == planning.operation_id
        || draft
            .proposals
            .iter()
            .any(|p| p.operation_id == e.command.operation_id)
        || draft
            .answers
            .iter()
            .any(|a| a.operation_id == e.command.operation_id)
        || !matches!(
            plan.status,
            PlanStatus::Running
                | PlanStatus::Stopping
                | PlanStatus::Completed
                | PlanStatus::Failed
                | PlanStatus::Interrupted
                | PlanStatus::Cancelled
                | PlanStatus::Abandoned
        )
        || e.attempts.len() > plan.tasks.len()
    {
        bail!("Damaged execution receipt; retained without replay")
    }
    let mut grants = BTreeSet::new();
    let mut results = BTreeMap::new();
    for (index, task) in plan.tasks.iter().enumerate() {
        let Some(a) = e.attempts.get(index) else {
            if !matches!(task.status, TaskStatus::Pending | TaskStatus::Cancelled)
                || task.attempts != 0
                || task.result.is_some()
                || !task.assignments.is_empty()
            {
                bail!("Unissued reviewed task has execution state")
            }
            continue;
        };
        let g = &a.grant;
        g.validate()?;
        let mut expected = crate::request_approval::scope(plan, index, approval)?;
        expected.grant_id = g.grant_id.clone();
        expected.session_id = g.session_id.clone();
        expected.issued_at_ms = g.issued_at_ms;
        expected.expires_at_ms = g.expires_at_ms;
        if *g != expected
            || g.issued_at_ms < e.started_at_ms
            || g.expires_at_ms > approval.expires_at_ms
            || g.expires_at_ms
                != g.issued_at_ms
                    .checked_add(g.limits.max_execution_ms)
                    .context("Grant time overflow")?
                    .min(approval.expires_at_ms)
            || !grants.insert(&g.grant_id)
            || g.grant_id == e.command.operation_id
            || g.grant_id == approval.operation_id
            || g.grant_id == planning.operation_id
            || task.attempts != 1
            || task.assignments.len() != 1
            || (!paired
                && index > 0
                && (e.attempts[index - 1].state != AttemptState::Completed
                    || e.attempts[index - 1]
                        .final_report()
                        .is_none_or(|r| !r.success)))
        {
            bail!("Grant differs from its fixed approved definition or predecessor")
        }
        let assignment = &task.assignments[0];
        if assignment.attempt != 1
            || assignment.node_id != g.executor_node
            || assignment.resource_id != g.resource_id
            || assignment.model_id != g.model_id
            || assignment.backend != g.backend
            || assignment.assigned_at_ms != g.issued_at_ms
        {
            bail!("Grant assignment changed")
        }
        if let Some(ack) = &a.acknowledgement {
            if ack.state != ReceiptState::Accepted || !ack.confirms(g) {
                bail!("Invalid durable grant acknowledgement")
            }
        }
        if let Some(r) = &a.report {
            if !r.confirms(g) || r.state == ReceiptState::Accepted {
                bail!("Invalid scoped completion")
            }
        }
        crate::request_recovery::validate_record(plan, a)?;
        let (status, report) = match a.state {
            AttemptState::Issued if a.acknowledgement.is_none() && a.report.is_none() => {
                (TaskStatus::Running, None)
            }
            AttemptState::Accepted if a.acknowledgement.is_some() && a.report.is_none() => {
                (TaskStatus::Running, None)
            }
            AttemptState::Completed
                if (a.acknowledgement.is_some() || a.recovery.is_some())
                    && a.final_report()
                        .is_some_and(|r| r.state == ReceiptState::Completed) =>
            {
                (
                    if a.final_report().unwrap().success {
                        TaskStatus::Completed
                    } else {
                        TaskStatus::Failed
                    },
                    a.final_report(),
                )
            }
            AttemptState::Rejected
                if a.acknowledgement.is_none()
                    && a.report
                        .as_ref()
                        .is_some_and(|r| r.state == ReceiptState::Rejected) =>
            {
                (TaskStatus::Failed, a.report.as_ref())
            }
            AttemptState::Unknown
                if a.report
                    .as_ref()
                    .is_none_or(|r| r.state == ReceiptState::Unknown)
                    && (a.report.is_none() || a.acknowledgement.is_some() || paired) =>
            {
                (TaskStatus::Interrupted, a.report.as_ref())
            }
            _ => bail!("Grant lifecycle is inconsistent"),
        };
        if task.status != status {
            bail!("Task does not match its grant lifecycle")
        }
        if let Some(report) = report {
            let expected = result(g, report);
            if task
                .result
                .as_ref()
                .is_none_or(|r| json!(r) != json!(expected))
            {
                bail!("Task result differs from the scoped receipt")
            }
            results.insert(task.task_id.clone(), expected);
        } else if task.result.is_some() {
            bail!("Unconfirmed task has a result")
        }
    }
    if json!(results) != json!(plan.results)
        || (!matches!(plan.status, PlanStatus::Running | PlanStatus::Stopping)
            && e.attempts
                .iter()
                .any(|a| matches!(a.state, AttemptState::Issued | AttemptState::Accepted)))
        || (plan.status == PlanStatus::Completed
            && plan.tasks.iter().any(|t| t.status != TaskStatus::Completed))
        || (plan.status == PlanStatus::Failed
            && !plan.tasks.iter().any(|t| t.status == TaskStatus::Failed))
        || (plan.status == PlanStatus::Cancelled
            && plan.tasks.iter().any(|t| {
                matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Running | TaskStatus::Interrupted
                )
            }))
        || (!paired
            && matches!(plan.status, PlanStatus::Running | PlanStatus::Stopping)
            && plan
                .tasks
                .iter()
                .any(|t| matches!(t.status, TaskStatus::Interrupted | TaskStatus::Failed)))
        || (e
            .attempts
            .iter()
            .filter(|a| matches!(a.state, AttemptState::Issued | AttemptState::Accepted))
            .count()
            > if paired { 2 } else { 1 })
    {
        bail!("Reviewed execution/result state is inconsistent")
    }
    if let Some(record) = &e.abandonment {
        record.command.validate()?;
        let a = e
            .attempts
            .last()
            .context("Abandonment lacks an issued attempt")?;
        if record.schema_version != 1
            || plan.status != PlanStatus::Abandoned
            || record.command.id != plan.plan_id
            || record.command.grant_id != a.grant.grant_id
            || record.command.expected_revision <= e.command.expected_revision
            || record.command.expected_revision.checked_add(1) != Some(plan.revision)
            || record.decided_at_ms < a.grant.issued_at_ms
            || record.decided_at_ms < e.started_at_ms
            || record.decided_at_ms > plan.updated_at_ms
            || operation_used(plan, &record.command.operation_id)
            || a.state != AttemptState::Unknown
            || plan.tasks.iter().any(|t| {
                matches!(
                    t.status,
                    TaskStatus::Pending | TaskStatus::Running | TaskStatus::Failed
                )
            })
        {
            bail!("Damaged abandonment receipt; retained without recovery or replay")
        }
    } else if plan.status == PlanStatus::Abandoned {
        bail!("Abandoned tracking lacks an explicit uncertainty acknowledgement")
    }
    Ok(())
}
pub(crate) fn result(g: &ExecutionGrant, r: &GrantReport) -> TaskResult {
    TaskResult {
        task_id: g.task_id.clone(),
        success: r.success,
        data: Value::Null,
        error: r.error_code.clone().unwrap_or_default(),
        error_type: if r.success {
            String::new()
        } else {
            "GrantExecution".into()
        },
        duration_ms: 0,
        attempts: 1,
    }
}
pub(crate) fn cancel_pending(plan: &mut Plan) {
    for task in &mut plan.tasks {
        if task.status == TaskStatus::Pending {
            task.status = TaskStatus::Cancelled;
        }
    }
}

pub(crate) struct AttemptJournal {
    pub store: Arc<JobStore>,
    pub job_id: String,
    pub grant_id: String,
}
impl GrantObserver for AttemptJournal {
    fn record(&self, report: &GrantReport) -> Result<()> {
        self.store.edit(
            &self.job_id,
            "Scoped node grant report durably saved",
            |p| {
                let e = p.execution.as_mut().context("Missing issuance")?;
                let a = e
                    .attempts
                    .iter_mut()
                    .find(|a| a.grant.grant_id == self.grant_id)
                    .context("Unknown issued grant")?;
                if !report.confirms(&a.grant) {
                    bail!("Report does not confirm issued authority")
                }
                if e.peer
                    .as_ref()
                    .is_some_and(|r| !r.confirms(&a.grant, report))
                {
                    bail!("Report belongs to another paired session")
                }
                if a.acknowledgement
                    .as_ref()
                    .is_some_and(|old| json!(old) == json!(report))
                    || a.report
                        .as_ref()
                        .is_some_and(|old| json!(old) == json!(report))
                {
                    return Ok(false);
                }
                if !matches!(p.status, PlanStatus::Running | PlanStatus::Stopping) {
                    bail!("No live reviewed attempt")
                }
                if report.state == ReceiptState::Accepted {
                    if a.state != AttemptState::Issued {
                        bail!("Attempt already acknowledged or unresolved")
                    }
                    a.state = AttemptState::Accepted;
                    a.acknowledgement = Some(report.clone());
                    return Ok(true);
                }
                if !(a.state == AttemptState::Issued && report.state == ReceiptState::Rejected
                    || e.peer.is_some()
                        && a.state == AttemptState::Issued
                        && report.state == ReceiptState::Unknown
                    || a.state == AttemptState::Accepted
                        && matches!(
                            report.state,
                            ReceiptState::Completed | ReceiptState::Unknown
                        ))
                {
                    bail!("Completion lacks the matching saved acceptance")
                }
                a.state = match report.state {
                    ReceiptState::Completed => AttemptState::Completed,
                    ReceiptState::Rejected => AttemptState::Rejected,
                    _ => AttemptState::Unknown,
                };
                a.report = Some(report.clone());
                let g = a.grant.clone();
                let task = p
                    .tasks
                    .iter_mut()
                    .find(|t| t.task_id == g.task_id)
                    .context("Unknown task")?;
                task.status = match report.state {
                    ReceiptState::Unknown => TaskStatus::Interrupted,
                    _ => {
                        if report.success {
                            TaskStatus::Completed
                        } else {
                            TaskStatus::Failed
                        }
                    }
                };
                let r = result(&g, report);
                task.result = Some(r.clone());
                p.results.insert(g.task_id.clone(), r);
                if p.execution.as_ref().unwrap().peer.is_some() {
                    crate::peer_jobs::settle(p);
                } else if report.state == ReceiptState::Unknown {
                    p.status = PlanStatus::Interrupted;
                } else if !report.success {
                    p.status = PlanStatus::Failed;
                    cancel_pending(p);
                } else if p.status == PlanStatus::Stopping {
                    p.status = PlanStatus::Cancelled;
                    cancel_pending(p);
                } else if p.tasks.iter().all(|t| t.status == TaskStatus::Completed) {
                    p.status = PlanStatus::Completed;
                }
                Ok(true)
            },
        )?;
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Driver {
    pub(crate) runtime_id: String,
    pub(crate) engine: Arc<TaskEngine>,
    pub(crate) server: eefn::NodeServer,
    pub(crate) registry: crate::capability::CapabilityRegistry,
    models: crate::model::ModelRegistry,
    tiers: BTreeMap<String, Vec<String>>,
}
impl Driver {
    #[cfg(test)]
    pub(crate) fn peer_fixture(
        engine: Arc<TaskEngine>,
        server: eefn::NodeServer,
        registry: crate::capability::CapabilityRegistry,
    ) -> Self {
        Self {
            runtime_id: server.runtime_id().to_owned(),
            engine,
            server,
            registry,
            models: Default::default(),
            tiers: Default::default(),
        }
    }
    pub(crate) fn new(runtime: &crate::Runtime) -> Self {
        Self {
            runtime_id: runtime.instance_id.clone(),
            engine: runtime.engine.clone(),
            server: runtime.node_server.clone(),
            registry: runtime.registry.clone(),
            models: runtime.model_registry.clone(),
            tiers: crate::runtime::model_tiers(&runtime.config),
        }
    }
    pub(crate) async fn session(&self, p: &Plan, index: usize) -> Result<Value> {
        let t = &p.tasks[index];
        let b = &p
            .planning
            .as_ref()
            .context("Missing compiled plan")?
            .bindings[index];
        let peer = crate::request_planning::live_peer(
            &self.server,
            &b.node_id,
            &t.capability,
            self.registry.scheduling_policy().freshness_seconds,
        )
        .await?;
        let session = if t.capability == "peer.transfer" {
            self.server.peer_grant_session(&b.node_id).await?
        } else {
            self.server.grant_session(&b.node_id).await?
        };
        if eefn::pipeline::is_compound(&t.capability) {
            let definition = crate::pipeline_planning::definition_for(
                &t.capability,
                &t.params,
                Some(&b.node_id),
            )?;
            crate::pipeline_planning::validate_peer_for(
                &t.capability,
                &definition,
                &peer,
                &self.registry,
                &self.models,
                t.request_context.as_ref(),
            )?;
        } else if b.model_id.is_some() {
            let candidates = self.models.route(
                &crate::model::ModelRequest {
                    modality: if t.capability == "vlm.analyze" {
                        crate::model::Modality::Vlm
                    } else {
                        crate::model::Modality::Text
                    },
                    tier: t.params["tier"].as_str().map(str::to_owned),
                    model_id: b.model_id.clone(),
                    constraints: t.constraints.clone(),
                },
                &self.tiers,
            );
            if !candidates.iter().any(|s| {
                s.node_id == b.node_id
                    && Some(&s.model_id) == b.model_id.as_ref()
                    && Some(&s.backend) == b.backend.as_ref()
                    && peer
                        .models
                        .iter()
                        .any(|m| crate::request_planning::current_model_matches(m, s))
            }) {
                bail!("Exact approved model/backend/role is not current")
            }
        } else {
            let provider = self
                .registry
                .find_best_with_context(
                    &t.capability,
                    &t.action,
                    &t.constraints,
                    t.request_context.as_ref(),
                )
                .context("Exact approved provider unavailable")?;
            if provider.node_id != b.node_id
                || provider.properties["selected_resource_id"].as_str() != b.resource_id.as_deref()
            {
                bail!("Approved resource changed")
            }
            crate::capability::CapabilityRegistry::validate_provider_params(&provider, &t.params)?;
        }
        Ok(session)
    }
    async fn drive(&self, id: &str) -> Result<()> {
        if crate::peer_jobs::definition(&self.engine.store.get(id)?)?.is_some() {
            return self.drive_peer(id).await;
        }
        loop {
            let p = self.engine.store.get(id)?;
            if p.status == PlanStatus::Stopping {
                self.engine.store.change(
                    id,
                    "Stop settled before further grant issuance",
                    |p| {
                        p.status = PlanStatus::Cancelled;
                        cancel_pending(p);
                        Ok(())
                    },
                )?;
                return Ok(());
            }
            if p.status != PlanStatus::Running {
                return Ok(());
            }
            let index = p
                .execution
                .as_ref()
                .context("Missing execution receipt")?
                .attempts
                .len();
            if index >= p.tasks.len() {
                bail!("No task remains but execution has no terminal checkpoint")
            }
            let session = self.session(&p, index).await?;
            let t = &p.tasks[index];
            let approval = p.approval.as_ref().unwrap();
            let policy: eefn::grants::GrantPolicy =
                serde_json::from_value(session["limits"].clone())?;
            policy.validate()?;
            if session["issuer_id"] != json!(approval.issuer_id)
                || approval.limits.max_execution_ms > policy.max_duration_ms
                || approval.limits.max_result_bytes > policy.max_result_bytes
            {
                bail!("Approved issuer or node bounds changed")
            }
            let dependencies = if eefn::pipeline::is_compound(&t.capability) {
                let node = &p.planning.as_ref().unwrap().bindings[index].node_id;
                crate::pipeline_planning::definition_for(&t.capability, &t.params, Some(node))?
                    .steps
                    .into_iter()
                    .map(|step| {
                        let mut constraints = json!({"node_id":node});
                        if let Some(resource) = step.resource_id {
                            constraints["resource_id"] = json!(resource);
                        }
                        (step.capability, step.action, constraints)
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let reservation = self
                .registry
                .reserve_bundle_with_context(
                    &t.capability,
                    &t.action,
                    &t.constraints,
                    t.request_context.as_ref(),
                    &dependencies,
                )
                .context("Approved provider has no scheduling capacity")?;
            if reservation.provider().node_id
                != p.planning.as_ref().unwrap().bindings[index].node_id
            {
                bail!("Reservation cannot rebind a reviewed grant")
            }
            let issued_mono = std::time::Instant::now();
            let now = eefn::protocol::now_ms();
            if now < approval.approved_at_ms || now >= approval.expires_at_ms {
                bail!("Owner approval expired or clock changed")
            }
            let mut grant = crate::request_approval::scope(&p, index, approval)?;
            grant.grant_id = uuid::Uuid::new_v4().to_string();
            grant.session_id = session["session_id"]
                .as_str()
                .context("No grant session")?
                .into();
            grant.issued_at_ms = now;
            grant.expires_at_ms = now
                .checked_add(grant.limits.max_execution_ms)
                .context("Grant time overflow")?
                .min(approval.expires_at_ms);
            grant.validate()?;
            let lease_deadline =
                issued_mono + Duration::from_millis(grant.expires_at_ms - grant.issued_at_ms);
            let revision = p.revision;
            self.engine.edit_review_policy_checkpoint(
                id,
                "Exact execution grant saved before dispatch",
                &self.runtime_id,
                &approval.issuer_id,
                |p| {
                    if p.status != PlanStatus::Running || p.revision != revision {
                        bail!("Execution state changed before issuance")
                    }
                    issue(p, index, grant.clone())?;
                    Ok(true)
                },
            )?;
            let observer = Arc::new(AttemptJournal {
                store: self.engine.store.clone(),
                job_id: id.into(),
                grant_id: grant.grant_id.clone(),
            });
            // This caller never retries an issued lease or falls back to ordinary RPC.
            let wait = Duration::from_millis(
                grant.expires_at_ms.saturating_sub(eefn::protocol::now_ms()) + 5000,
            );
            let reply = self
                .wait_with_stop(
                    id,
                    std::slice::from_ref(&grant),
                    self.server.invoke_grant_observed(
                        grant.clone(),
                        wait,
                        observer,
                        lease_deadline,
                    ),
                )
                .await;
            drop(reservation);
            let valid = reply
                .as_ref()
                .ok()
                .and_then(|v| serde_json::from_value::<GrantReport>(v["grant"].clone()).ok())
                .is_some_and(|r| r.confirms(&grant));
            if !valid {
                bail!(
                    "Issued work has no confirmed durable completion; inspect receipt/effects, never replay"
                )
            }
        }
    }
}
fn issue(p: &mut Plan, index: usize, g: ExecutionGrant) -> Result<()> {
    if crate::peer_jobs::definition(p)?.is_some() {
        bail!("Peer grants require one atomic paired issuance")
    }
    if p.status != PlanStatus::Running {
        bail!("Issuance requires the active reviewed execution")
    }
    let e = p.execution.as_mut().context("Missing execution receipt")?;
    if e.attempts.len() != index
        || p.tasks[index].status != TaskStatus::Pending
        || p.tasks[index].attempts != 0
        || (index > 0
            && e.attempts[index - 1]
                .report
                .as_ref()
                .is_none_or(|r| !r.success))
    {
        bail!("Task already issued or dependencies unresolved")
    }
    append_attempt(p, g);
    Ok(())
}
pub(crate) fn append_attempt(p: &mut Plan, g: ExecutionGrant) {
    let e = p.execution.as_mut().unwrap();
    let index = e.attempts.len();
    p.tasks[index].status = TaskStatus::Running;
    p.tasks[index].attempts = 1;
    p.tasks[index].assignments.push(crate::jobs::Assignment {
        attempt: 1,
        node_id: g.executor_node.clone(),
        resource_id: g.resource_id.clone(),
        model_id: g.model_id.clone(),
        backend: g.backend.clone(),
        assigned_at_ms: g.issued_at_ms,
    });
    e.attempts.push(GrantedAttempt {
        grant: g,
        state: AttemptState::Issued,
        acknowledgement: None,
        report: None,
        recovery: None,
        stop: None,
    });
}
pub(crate) fn start(p: &mut Plan, command: &ExecuteCommand, now: u64) -> Result<()> {
    command.validate()?;
    let a = p.approval.as_ref().context("Owner approval missing")?;
    if a.workload.is_some() || a.schema_version != 1 {
        bail!("Workload approval cannot enter the v1 execution path")
    }
    if p.execution.is_some()
        || p.plan_id != command.id
        || p.revision != command.expected_revision
        || a.approval_sha256 != command.approval_sha256
        || !a.active_at(p, now)
    {
        bail!("Execution state/approval changed")
    }
    p.execution = Some(ExecutionRecord {
        schema_version: 1,
        command: command.clone(),
        started_at_ms: now,
        attempts: vec![],
        abandonment: None,
        peer: None,
        stop_requested_at_ms: None,
        workload: None,
    });
    p.status = PlanStatus::Running;
    Ok(())
}
impl crate::Runtime {
    pub fn request_abandonment_command(&self, request: AbandonmentRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Abandonment schema/runtime changed")
        }
        self.abandon_request(request.command, None)
    }
    pub(crate) fn abandon_request(
        &self,
        command: eefn::request_abandon::AbandonCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        let (p, reused) = self.engine.abandon_reviewed(&command, origin)?;
        let abandonment = p
            .execution
            .as_ref()
            .and_then(|e| e.abandonment.clone())
            .context("No saved abandonment receipt")?;
        Ok(json!(eefn::request_abandon::AbandonReport {
            schema_version: 1, report_type: "coordinator_request_abandonment".into(), success: true,
            runtime_id: self.instance_id.clone(), id: p.plan_id, revision: p.revision, status: "abandoned".into(),
            request_context: p.goal.request_context.context("No origin")?, abandonment, reused, persisted: true,
            tracking_abandoned: true, remote_stop_confirmed: false, remote_effects_unknown: true,
            execution_requested: false, automatically_retried: false,
            note: "Local coordinator tracking closed by explicit uncertainty acknowledgement. Issued attempts/effects remain unknown, not successful or remotely stopped. No retry, migration, grant or output recovery authorized. Retain receipts for investigation.".into(),
        }))
    }
    pub async fn request_receipt_command(
        &self,
        request: ReceiptInspectionRequest,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Receipt query schema/runtime changed")
        }
        self.inspect_request_receipt(request.command, None).await
    }
    pub(crate) async fn inspect_request_receipt(
        &self,
        command: eefn::request_receipt::ReceiptCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        let plan = self.engine.store.get(&command.id)?;
        let attempt = receipt_attempt(&plan, &command, origin)?;
        let receipt = self
            .node_server
            .inspect_grant_receipt(&attempt.grant)
            .await?;
        let coordinator_attempt_state = serde_json::to_value(attempt.state)?
            .as_str()
            .unwrap()
            .into();
        Ok(json!(eefn::request_receipt::ReceiptReport {
            schema_version: 1, report_type: "coordinator_request_receipt".into(), success: true,
            runtime_id: self.instance_id.clone(), id: plan.plan_id, revision: plan.revision,
            request_context: plan.goal.request_context.context("Request context missing")?,
            coordinator_attempt_state, receipt,
            execution_requested: false, automatically_retried: false, reconciled: false,
            note: "Read-only executor evidence. Coordinator state is a revision snapshot and remains unchanged. Consumed/completed does not recover output or prove success/termination; missing receipts do not prove no effects. No retry or fallback.".into(),
        }))
    }
    pub async fn execution_command(&self, request: ExecutionRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Execution schema/runtime changed")
        }
        self.execute_reviewed(request.command, None).await
    }
    pub async fn request_stop_receipt_command(
        &self,
        request: ReceiptInspectionRequest,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Stop inspection schema/runtime changed")
        }
        self.inspect_request_stop_receipt(request.command, None)
            .await
    }
    pub(crate) async fn inspect_request_stop_receipt(
        &self,
        command: eefn::request_stop_receipt::Command,
        origin: Option<&str>,
    ) -> Result<Value> {
        let plan = self.engine.store.get(&command.id)?;
        let attempt = receipt_attempt(&plan, &command, origin)?;
        let inspection = self.node_server.inspect_grant_stop(&attempt.grant).await?;
        let coordinator_attempt_state = serde_json::to_value(attempt.state)?
            .as_str()
            .unwrap()
            .to_owned();
        let report = json!(eefn::request_stop_receipt::CoordinatorReport {
            schema_version: 1,
            report_type: "coordinator_request_stop_receipt".into(),
            success: true,
            runtime_id: self.instance_id.clone(),
            id: plan.plan_id,
            revision: plan.revision,
            request_context: plan
                .goal
                .request_context
                .context("Request context missing")?,
            coordinator_attempt_state,
            inspection,
            read_only: true,
            execution_requested: false,
            stop_requested: false,
            automatically_retried: false,
            reconciled: false,
            note: eefn::request_stop_receipt::note()
        });
        if !eefn::request_stop_receipt::confirmed(
            &report,
            &command,
            origin,
            Some(&self.instance_id),
        ) {
            bail!("Stop inspection failed confirmation")
        }
        Ok(report)
    }
    pub async fn request_result_command(&self, request: ReceiptInspectionRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Result query schema/runtime changed")
        }
        self.inspect_request_result(request.command, None).await
    }
    pub(crate) async fn inspect_request_result(
        &self,
        command: eefn::request_result::ResultCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        let plan = self.engine.store.get(&command.id)?;
        let attempt = receipt_attempt(&plan, &command, origin)?;
        let result = self
            .node_server
            .inspect_grant_result(&attempt.grant)
            .await?;
        let state = serde_json::to_value(attempt.state)?
            .as_str()
            .unwrap()
            .to_string();
        Ok(json!(eefn::request_result::ResultReport {
            schema_version:1,report_type:"coordinator_request_result".into(),success:true,runtime_id:self.instance_id.clone(),
            id:plan.plan_id,revision:plan.revision,request_context:plan.goal.request_context.context("Request context missing")?,
            coordinator_attempt_state:state,result,
            execution_requested:false,automatically_retried:false,reconciled:false,
            note:"Explicit read-only retained report/output. Coordinator state remains a revision snapshot, unchanged even when final node evidence exists. No lease renewal, execution, rollback or proof every external effect stopped. Missing output never proves no effects.".into(),
        }))
    }
    pub(crate) async fn execute_reviewed(
        &self,
        command: ExecuteCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        command.validate()?;
        let p = self.engine.store.get(&command.id)?;
        if origin.is_some_and(|node| {
            p.goal
                .request_context
                .as_ref()
                .is_none_or(|c| c.origin_node() != node)
        }) {
            bail!("Request not found for this origin")
        }
        if let Some(e) = &p.execution {
            if e.command == command {
                return report(self, &p, true);
            }
            bail!("Execution already requested; inspect existing receipts, never replay or migrate")
        }
        let approval = p.approval.as_ref().context("Owner approval required")?;
        if p.revision != command.expected_revision
            || approval.approval_sha256 != command.approval_sha256
            || !approval.active_at(&p, eefn::protocol::now_ms())
        {
            bail!("Exact owner approval/revision is not active")
        }
        let driver = Driver::new(self);
        for index in 0..p.tasks.len() {
            let s = driver.session(&p, index).await?;
            if s["issuer_id"] != json!(approval.issuer_id) {
                bail!("Owner approval issuer changed")
            }
        }
        let mut saved = None;
        let guard = self.engine.claim(&command.id, || {
            saved = Some(self.engine.store.edit_with_policy_fence(
                &command.id,
                "Owner-approved execution requested; grants require a separate durable checkpoint",
                &self.instance_id,
                &approval.issuer_id,
                |p| {
                    start(p, &command, eefn::protocol::now_ms())?;
                    Ok(true)
                },
            )?);
            Ok(())
        })?;
        let saved = saved.unwrap();
        let id = command.id.clone();
        let mut closing = self.engine.shutdown_receiver();
        tokio::spawn(async move {
            let _guard = guard;
            if *closing.borrow() {
                return;
            }
            tokio::select! {biased;
                _=closing.changed()=>{},
                outcome=driver.drive(&id)=>{if let Err(error)=outcome {tracing::warn!(job_id=%id,%error,"Reviewed grant execution needs inspection; no replay");}}
            }
        });
        report(self, &saved, false)
    }
}
fn report(runtime: &crate::Runtime, p: &Plan, reused: bool) -> Result<Value> {
    let e = p.execution.as_ref().context("No execution receipt")?;
    Ok(json!(ExecuteReport {schema_version:1,report_type:"coordinator_request_execution".into(),success:true,runtime_id:runtime.instance_id.clone(),
        id:p.plan_id.clone(),operation_id:e.command.operation_id.clone(),source_revision:e.command.expected_revision,approval_sha256:e.command.approval_sha256.clone(),
        revision:p.revision,status:serde_json::to_value(p.status)?.as_str().unwrap().into(),request_context:p.goal.request_context.clone().context("No origin")?,
        reused,persisted:true,execution_requested:true,automatically_retried:false,note:"Durable request receipt, not completion or remote-stop confirmation. Inspect job and grant receipts after uncertainty; no implicit replay, renewal or migration.".into()}))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{event::EventBus, request_approval::tests as approval};
    fn abandonment_command(p: &Plan) -> eefn::request_abandon::AbandonCommand {
        eefn::request_abandon::AbandonCommand {
            id: p.plan_id.clone(),
            expected_revision: p.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            grant_id: p
                .execution
                .as_ref()
                .unwrap()
                .attempts
                .last()
                .unwrap()
                .grant
                .grant_id
                .clone(),
            acknowledge_effects_may_continue: true,
        }
    }
    #[test]
    fn abandonment_preserves_unknown_authority_and_effects_without_replay_or_fake_success() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, journal) = issued(&engine);
        journal.record(&ack(&g)).unwrap();
        engine.store.interrupt(&p.plan_id).unwrap();
        let unknown = engine.store.get(&p.plan_id).unwrap();
        let command = abandonment_command(&unknown);
        let origin = unknown.goal.request_context.as_ref().unwrap().origin_node();
        assert!(
            engine
                .abandon_reviewed(&command, Some("other-origin"))
                .is_err()
        );
        let (closed, reused) = engine.abandon_reviewed(&command, Some(origin)).unwrap();
        assert!(!reused);
        assert_eq!(closed.status, PlanStatus::Abandoned);
        assert_eq!(closed.revision, unknown.revision + 1);
        assert_eq!(json!(closed.goal), json!(unknown.goal));
        assert_eq!(json!(closed.tasks), json!(unknown.tasks));
        assert_eq!(
            json!(closed.execution.as_ref().unwrap().attempts),
            json!(unknown.execution.as_ref().unwrap().attempts)
        );
        assert_eq!(json!(closed.results), json!(unknown.results));
        assert_eq!(
            closed.execution.as_ref().unwrap().attempts[0].state,
            AttemptState::Unknown
        );
        let view = crate::jobs::view(&closed, true);
        assert_eq!(view["tracking_abandoned"], true);
        assert_eq!(view["remote_effects_unknown"], true);
        assert_eq!(view["can_resume"], false);
        assert_eq!(view["can_abandon_tracking"], false);
        let (same, reused) = engine.abandon_reviewed(&command, Some(origin)).unwrap();
        assert!(reused);
        assert_eq!(json!(same), json!(closed));
        assert!(
            engine.abandon_reviewed(&command, None).is_err(),
            "different authenticated actor cannot reuse this action receipt"
        );
        let mut changed = command.clone();
        changed.operation_id = uuid::Uuid::new_v4().to_string();
        assert!(engine.abandon_reviewed(&changed, Some(origin)).is_err());
        assert!(journal.record(&done(&g)).is_err());
        assert!(engine.cancel(&p.plan_id).is_err());
        assert!(engine.pause(&p.plan_id).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(closed));
        engine.store.remove_finished(&p.plan_id).unwrap();
        assert!(engine.store.get(&p.plan_id).is_err());
    }
    #[tokio::test]
    async fn abandonment_requires_idle_exact_unknown_checkpoint_and_rolls_back_denials() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, j) = issued(&engine);
        let command = abandonment_command(&p);
        assert!(engine.abandon_reviewed(&command, None).is_err());
        j.record(&ack(&g)).unwrap();
        engine.store.interrupt(&p.plan_id).unwrap();
        let unknown = engine.store.get(&p.plan_id).unwrap();
        let command = abandonment_command(&unknown);
        let mut bad = command.clone();
        bad.acknowledge_effects_may_continue = false;
        assert!(engine.abandon_reviewed(&bad, None).is_err());
        let mut bad = command.clone();
        bad.expected_revision -= 1;
        assert!(engine.abandon_reviewed(&bad, None).is_err());
        let mut bad = command.clone();
        bad.grant_id = uuid::Uuid::new_v4().to_string();
        assert!(engine.abandon_reviewed(&bad, None).is_err());
        for used in [
            unknown
                .execution
                .as_ref()
                .unwrap()
                .command
                .operation_id
                .clone(),
            unknown.approval.as_ref().unwrap().operation_id.clone(),
            unknown.planning.as_ref().unwrap().operation_id.clone(),
        ] {
            let mut bad = command.clone();
            bad.operation_id = used;
            assert!(engine.abandon_reviewed(&bad, None).is_err());
        }
        let guard = engine.claim(&p.plan_id, || Ok(())).unwrap();
        assert!(engine.abandon_reviewed(&command, None).is_err());
        drop(guard);
        let mut clock_changed = unknown.clone();
        assert!(
            abandon(
                &mut clock_changed,
                &command,
                None,
                false,
                unknown.updated_at_ms - 1
            )
            .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(unknown));
        engine.shutdown().await;
        assert!(engine.abandon_reviewed(&command, None).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(unknown));
    }
    #[test]
    fn abandonment_is_durable_idempotent_under_racing_receipts_and_strict_against_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let engine = TaskEngine::with_store(
            EventBus::new(20),
            JobStore::open(&path, 20, 200000).unwrap(),
        );
        let (p, _, _) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let command = abandonment_command(&engine.store.get(&p.plan_id).unwrap());
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|s| {
            let first = s.spawn(|| {
                barrier.wait();
                engine.abandon_reviewed(&command, None).unwrap()
            });
            barrier.wait();
            let second = engine.abandon_reviewed(&command, None).unwrap();
            let first = first.join().unwrap();
            assert_ne!(first.1, second.1);
            assert_eq!(json!(first.0), json!(second.0));
        });
        let closed = engine.store.get(&p.plan_id).unwrap();
        for (field, bad) in [("schema_version", json!(2)), ("decided_at_ms", json!(0))] {
            let mut raw = json!(closed);
            raw["execution"]["abandonment"][field] = bad;
            assert!(
                crate::jobs::validate_retained_review(&serde_json::from_value(raw).unwrap())
                    .is_err()
            );
        }
        let mut raw = json!(closed);
        raw["execution"]["abandonment"]["command"]["acknowledge_effects_may_continue"] =
            json!(false);
        assert!(
            crate::jobs::validate_retained_review(&serde_json::from_value(raw).unwrap()).is_err()
        );
        let mut bad = closed.clone();
        bad.status = PlanStatus::Cancelled;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = closed.clone();
        bad.execution.as_mut().unwrap().abandonment = None;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        drop(engine);
        let engine =
            TaskEngine::with_store(EventBus::new(20), JobStore::open(&path, 20, 1024).unwrap());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(closed));
        let (same, reused) = engine.abandon_reviewed(&command, None).unwrap();
        assert!(reused);
        assert_eq!(json!(same), json!(closed));
        assert!(engine.resume_job(&p.plan_id).is_err());
    }
    #[test]
    fn receipt_selection_requires_original_origin_and_exact_retained_issuance_without_mutation() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, journal) = issued(&engine);
        let command = eefn::request_receipt::ReceiptCommand {
            id: p.plan_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        let origin = p.goal.request_context.as_ref().unwrap().origin_node();
        assert_eq!(
            receipt_attempt(&p, &command, Some(origin)).unwrap().grant,
            g
        );
        assert!(receipt_attempt(&p, &command, Some("different-origin")).is_err());
        let mut wrong = command.clone();
        wrong.grant_id = uuid::Uuid::new_v4().to_string();
        assert!(receipt_attempt(&p, &wrong, None).is_err());
        let unissued = requested(&engine);
        let no_grant = eefn::request_receipt::ReceiptCommand {
            id: unissued.plan_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        assert!(receipt_attempt(&unissued, &no_grant, None).is_err());
        journal.record(&ack(&g)).unwrap();
        engine.store.interrupt(&p.plan_id).unwrap();
        let unknown = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(
            receipt_attempt(&unknown, &command, None).unwrap().state,
            AttemptState::Unknown
        );
        assert_eq!(json!(unknown), json!(engine.store.get(&p.plan_id).unwrap()));
    }
    fn approved(engine: &TaskEngine) -> Plan {
        let p = approval::compiled(engine);
        let cmd = approval::command(&p);
        approval::save(engine, &p, &cmd, approval::record(&cmd)).unwrap()
    }
    fn requested(engine: &TaskEngine) -> Plan {
        let p = approved(engine);
        let command = ExecuteCommand {
            id: p.plan_id.clone(),
            expected_revision: p.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            approval_sha256: p.approval.as_ref().unwrap().approval_sha256.clone(),
        };
        engine
            .store
            .change(&p.plan_id, "Explicit test execution receipt", |p| {
                start(p, &command, eefn::protocol::now_ms())
            })
            .unwrap()
    }
    fn grant(p: &Plan) -> ExecutionGrant {
        let mut g = crate::request_approval::scope(p, 0, p.approval.as_ref().unwrap()).unwrap();
        g.grant_id = uuid::Uuid::new_v4().to_string();
        g.session_id = uuid::Uuid::new_v4().to_string();
        g.issued_at_ms = eefn::protocol::now_ms();
        g.expires_at_ms = g.issued_at_ms + g.limits.max_execution_ms;
        g
    }
    pub(crate) fn issued(engine: &TaskEngine) -> (Plan, ExecutionGrant, AttemptJournal) {
        let p = requested(engine);
        let g = grant(&p);
        let p = engine
            .store
            .change(&p.plan_id, "Issuance before dispatch", |p| {
                issue(p, 0, g.clone())
            })
            .unwrap();
        let journal = AttemptJournal {
            store: engine.store.clone(),
            job_id: p.plan_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        (p, g, journal)
    }
    fn ack(g: &ExecutionGrant) -> GrantReport {
        GrantReport::new(g, ReceiptState::Accepted, Value::Null, None)
    }
    fn done(g: &ExecutionGrant) -> GrantReport {
        GrantReport::new(
            g,
            ReceiptState::Completed,
            json!({"private":"unique-output-marker"}),
            None,
        )
    }
    #[test]
    fn completed_unsuccessful_report_fails_job_without_losing_evidence_or_allowing_resume() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, journal) = issued(&engine);
        journal.record(&ack(&g)).unwrap();
        let failed = GrantReport::new(
            &g,
            ReceiptState::Completed,
            json!({"success":false,"exit_code":17,"stdout":"bounded"}),
            Some("process_exit_unsuccessful"),
        );
        journal.record(&failed).unwrap();
        let saved = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(saved.status, PlanStatus::Failed);
        assert_eq!(saved.tasks[0].status, TaskStatus::Failed);
        assert!(!saved.results[&g.task_id].success);
        assert_eq!(
            json!(saved.execution.as_ref().unwrap().attempts[0].report),
            json!(Some(&failed))
        );
        assert_eq!(saved.execution.as_ref().unwrap().attempts.len(), 1);
        assert!(engine.resume_job(&p.plan_id).is_err());
        journal.record(&failed).unwrap();
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(saved));
    }
    fn retained(g: &ExecutionGrant, report: GrantReport) -> eefn::request_result::RetainedResult {
        eefn::request_result::RetainedResult {
            schema_version: 1,
            receipt: eefn::request_receipt::ConsumedReceipt {
                schema_version: 1,
                node_id: g.executor_node.clone(),
                issuer_id: g.issuer_id.clone(),
                grant_id: g.grant_id.clone(),
                grant_sha256: eefn::grants::definition_digest(&json!(g)).unwrap(),
                job_id: g.job_id.clone(),
                task_id: g.task_id.clone(),
                attempt: g.attempt,
                state: if report.state == ReceiptState::Completed {
                    "completed"
                } else {
                    "unknown"
                }
                .into(),
                // Deliberately different node wall clock; do not compare to issuer time.
                accepted_at_ms: 1,
                updated_at_ms: 2,
                pipeline_progress: None,
            },
            max_result_bytes: g.limits.max_result_bytes,
            availability: "retained_final_report".into(),
            report_sha256: Some(eefn::grants::definition_digest(&json!(report)).unwrap()),
            report: Some(report),
        }
    }
    fn recovery_command(p: &Plan) -> eefn::request_recovery::RecoverCommand {
        eefn::request_recovery::RecoverCommand {
            id: p.plan_id.clone(),
            expected_revision: p.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            grant_id: p
                .execution
                .as_ref()
                .unwrap()
                .attempts
                .last()
                .unwrap()
                .grant
                .grant_id
                .clone(),
        }
    }
    #[test]
    fn recovery_preserves_real_ack_and_unknown_report_without_replay_and_survives_restart() {
        for acknowledgement in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            drop(rusqlite::Connection::open(&path).unwrap());
            let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
            let engine = TaskEngine::with_store(EventBus::new(20), store);
            let (p, g, journal) = issued(&engine);
            if acknowledgement {
                journal.record(&ack(&g)).unwrap();
                journal
                    .record(&GrantReport::new(
                        &g,
                        ReceiptState::Unknown,
                        Value::Null,
                        Some("completion_missing".into()),
                    ))
                    .unwrap();
            } else {
                engine.store.interrupt(&p.plan_id).unwrap();
            }
            let source = engine.store.get(&p.plan_id).unwrap();
            let cmd = recovery_command(&source);
            let origin = source.goal.request_context.as_ref().unwrap().origin_node();
            let original = &source.execution.as_ref().unwrap().attempts[0];
            let evidence = retained(&g, done(&g));
            let (saved, reused) = engine
                .recover_reviewed(&cmd, Some(origin), &evidence)
                .unwrap();
            assert!(!reused);
            assert_eq!(saved.revision, source.revision + 1);
            assert_eq!(saved.status, PlanStatus::Completed);
            let a = &saved.execution.as_ref().unwrap().attempts[0];
            assert_eq!(json!(a.acknowledgement), json!(original.acknowledgement));
            assert_eq!(json!(a.report), json!(original.report));
            assert_eq!(a.grant, original.grant);
            assert_eq!(saved.goal.request_context, source.goal.request_context);
            assert_eq!(saved.tasks[0].assignments.len(), 1);
            assert_eq!(saved.tasks[0].attempts, 1);
            assert!(saved.tasks[0].result.as_ref().unwrap().data.is_null());
            let view = crate::jobs::view(&saved, true);
            assert_eq!(view["grant_attempts"][0]["acknowledged"], acknowledgement);
            assert_eq!(view["grant_attempts"][0]["success"], true);
            assert_eq!(view["recovery"]["reconciled"], true);
            assert!(
                !serde_json::to_string(&view)
                    .unwrap()
                    .contains("unique-output-marker")
            );
            assert!(engine.resume_job(&p.plan_id).is_err());
            assert!(engine.cancel(&p.plan_id).is_err());
            // Original late ACK/completion cannot replace evidence or revive work.
            assert!(journal.record(&done(&g)).is_err());
            let (same, reused) = engine
                .recover_reviewed(&cmd, Some(origin), &evidence)
                .unwrap();
            assert!(reused);
            assert_eq!(json!(same), json!(saved));
            assert!(
                engine.recovery_source(&cmd, None).is_err(),
                "actor cannot change on receipt reuse"
            );
            drop(journal);
            drop(engine);
            let reopened = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
            assert_eq!(json!(reopened.get(&p.plan_id).unwrap()), json!(saved));
            let engine = TaskEngine::with_store(EventBus::new(20), reopened);
            assert!(engine.recovery_source(&cmd, Some(origin)).unwrap().1);
            assert!(engine.resume_job(&p.plan_id).is_err());
        }
    }
    #[test]
    fn recovery_rejects_wrong_origin_revision_scope_unfinished_and_damaged_evidence_without_writes()
    {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        let live = engine.store.get(&p.plan_id).unwrap();
        let live_cmd = recovery_command(&live);
        let good = retained(&g, done(&g));
        assert!(engine.recover_reviewed(&live_cmd, None, &good).is_err());
        engine.store.interrupt(&p.plan_id).unwrap();
        let source = engine.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        assert!(
            engine
                .recover_reviewed(&cmd, Some("other-origin"), &good)
                .is_err()
        );
        for operation in [
            g.grant_id.clone(),
            source
                .execution
                .as_ref()
                .unwrap()
                .command
                .operation_id
                .clone(),
            source.approval.as_ref().unwrap().operation_id.clone(),
            source.planning.as_ref().unwrap().operation_id.clone(),
            source.request_review.as_ref().unwrap().proposals[0]
                .operation_id
                .clone(),
        ] {
            let mut bad = cmd.clone();
            bad.operation_id = operation;
            assert!(engine.recover_reviewed(&bad, None, &good).is_err());
        }
        let mut bad_cmd = cmd.clone();
        bad_cmd.expected_revision -= 1;
        assert!(engine.recover_reviewed(&bad_cmd, None, &good).is_err());
        let mut bad_cmd = cmd.clone();
        bad_cmd.grant_id = uuid::Uuid::new_v4().to_string();
        assert!(engine.recover_reviewed(&bad_cmd, None, &good).is_err());
        let unknown = retained(
            &g,
            GrantReport::new(
                &g,
                ReceiptState::Unknown,
                Value::Null,
                Some("execution_unknown".into()),
            ),
        );
        unknown.validate().unwrap();
        assert!(engine.recover_reviewed(&cmd, None, &unknown).is_err());
        let mut legacy = good.clone();
        legacy.max_result_bytes = 0;
        legacy.availability = "legacy_metadata_only".into();
        legacy.report = None;
        legacy.report_sha256 = None;
        legacy.validate().unwrap();
        assert!(engine.recover_reviewed(&cmd, None, &legacy).is_err());
        let mut pending = good.clone();
        pending.receipt.state = "accepted".into();
        pending.availability = "no_final_checkpoint".into();
        pending.report = None;
        pending.report_sha256 = None;
        pending.validate().unwrap();
        assert!(engine.recover_reviewed(&cmd, None, &pending).is_err());
        let mut wrong = g.clone();
        wrong.session_id = uuid::Uuid::new_v4().to_string();
        assert!(
            engine
                .recover_reviewed(&cmd, None, &retained(&wrong, done(&wrong)))
                .is_err()
        );
        for field in [
            "issuer_id",
            "executor_node",
            "task_id",
            "plan_sha256",
            "session_id",
            "grant_sha256",
        ] {
            let mut value = json!(good);
            value["report"][field] = json!("changed");
            let bad = serde_json::from_value(value).unwrap();
            assert!(engine.recover_reviewed(&cmd, None, &bad).is_err());
        }
        let mut damaged = good.clone();
        damaged.report.as_mut().unwrap().data = json!({"modified":true});
        assert!(engine.recover_reviewed(&cmd, None, &damaged).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(source));
    }
    #[test]
    fn recovered_failure_is_failed_not_success_and_cached_receipt_can_be_reused_without_evidence() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let cmd = recovery_command(&engine.store.get(&p.plan_id).unwrap());
        let error = retained(
            &g,
            GrantReport::new(
                &g,
                ReceiptState::Completed,
                Value::Null,
                Some("operation_failed".into()),
            ),
        );
        let (saved, _) = engine.recover_reviewed(&cmd, None, &error).unwrap();
        assert_eq!(saved.status, PlanStatus::Failed);
        assert_eq!(saved.tasks[0].status, TaskStatus::Failed);
        assert!(!saved.tasks[0].result.as_ref().unwrap().success);
        assert_eq!(
            saved.tasks[0].result.as_ref().unwrap().error,
            "operation_failed"
        );
        assert!(engine.recovery_source(&cmd, None).unwrap().1);
        let mut changed = cmd.clone();
        changed.operation_id = uuid::Uuid::new_v4().to_string();
        assert!(engine.recovery_source(&changed, None).is_err());
        let mut changed = cmd.clone();
        changed.expected_revision = saved.revision;
        assert!(engine.recovery_source(&changed, None).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(saved));
    }
    #[test]
    fn concurrent_recovery_and_abandonment_have_exactly_one_winner() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let source = engine.store.get(&p.plan_id).unwrap();
        let recover = recovery_command(&source);
        let abandon = abandonment_command(&source);
        let evidence = retained(&g, done(&g));
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let a = {
            let engine = engine.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                engine.recover_reviewed(&recover, None, &evidence).is_ok()
            })
        };
        let b = {
            let engine = engine.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                engine.abandon_reviewed(&abandon, None).is_ok()
            })
        };
        barrier.wait();
        assert_ne!(a.join().unwrap(), b.join().unwrap());
        let saved = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(saved.revision, source.revision + 1);
        assert!(matches!(
            saved.status,
            PlanStatus::Completed | PlanStatus::Abandoned
        ));
    }
    #[test]
    fn recovery_validation_rejects_changed_record_or_fabricated_wire_history() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let cmd = recovery_command(&engine.store.get(&p.plan_id).unwrap());
        let (saved, _) = engine
            .recover_reviewed(&cmd, None, &retained(&g, done(&g)))
            .unwrap();
        for (path, value) in [
            ("schema_version", json!(2)),
            ("decided_at_ms", json!(0)),
            (
                "command",
                json!({"id":cmd.id,"grant_id":cmd.grant_id,"operation_id":cmd.operation_id,"expected_revision":saved.revision}),
            ),
        ] {
            let mut raw = json!(saved);
            raw["execution"]["attempts"][0]["recovery"][path] = value;
            let bad = serde_json::from_value(raw).unwrap();
            assert!(crate::jobs::validate_retained_review(&bad).is_err());
        }
        let mut bad = saved.clone();
        bad.execution.as_mut().unwrap().attempts[0].report = Some(done(&g));
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = saved.clone();
        bad.execution.as_mut().unwrap().attempts[0].recovery = None;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = saved.clone();
        bad.status = PlanStatus::Running;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = saved.clone();
        bad.execution.as_mut().unwrap().attempts[0]
            .recovery
            .as_mut()
            .unwrap()
            .result
            .report
            .as_mut()
            .unwrap()
            .data = json!("changed");
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
    }
    #[tokio::test]
    async fn active_runner_and_shutdown_reject_new_recovery_but_allow_exact_saved_inspection() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        let guard = engine.claim(&p.plan_id, || Ok(())).unwrap();
        engine.store.interrupt(&p.plan_id).unwrap();
        let source = engine.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        let evidence = retained(&g, done(&g));
        assert!(engine.recovery_source(&cmd, None).is_err());
        assert!(engine.recover_reviewed(&cmd, None, &evidence).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(source));
        drop(guard);
        let source = engine.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        let (saved, _) = engine.recover_reviewed(&cmd, None, &evidence).unwrap();
        engine.shutdown().await;
        assert!(engine.recovery_source(&cmd, None).unwrap().1);
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(saved));
        let closed = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&closed);
        closed.store.interrupt(&p.plan_id).unwrap();
        let source = closed.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        closed.recovery_source(&cmd, None).unwrap();
        closed.shutdown().await;
        assert!(
            closed
                .recover_reviewed(&cmd, None, &retained(&g, done(&g)))
                .is_err()
        );
        assert_eq!(json!(closed.store.get(&p.plan_id).unwrap()), json!(source));
    }
    #[test]
    fn journal_quota_failure_leaves_uncertainty_and_cached_evidence_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        let (p, g, journal) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let source = engine.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        let evidence = retained(
            &g,
            GrantReport::new(
                &g,
                ReceiptState::Completed,
                json!({"private":"x".repeat(40000)}),
                None,
            ),
        );
        evidence.validate().unwrap();
        drop(journal);
        drop(engine);
        let store = JobStore::open(
            &path,
            500,
            serde_json::to_vec(&source).unwrap().len() + 1024,
        )
        .unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        assert!(engine.recover_reviewed(&cmd, None, &evidence).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(source));
        assert!(evidence.confirms(&g));
        assert!(engine.resume_job(&p.plan_id).is_err());
    }
    #[test]
    fn source_revision_change_between_result_inspection_and_commit_does_not_reconcile() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, _) = issued(&engine);
        engine.store.interrupt(&p.plan_id).unwrap();
        let source = engine.store.get(&p.plan_id).unwrap();
        let cmd = recovery_command(&source);
        assert!(!engine.recovery_source(&cmd, None).unwrap().1);
        let changed = engine
            .store
            .change(&p.plan_id, "Fixture concurrent checkpoint", |_| Ok(()))
            .unwrap();
        assert!(
            engine
                .recover_reviewed(&cmd, None, &retained(&g, done(&g)))
                .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(changed));
    }
    #[test]
    fn saved_issuance_precedes_ack_and_completion_is_scope_bound_and_idempotent() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, j) = issued(&engine);
        let before = json!(p);
        assert!(j.record(&done(&g)).is_err());
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), before);
        let mut wrong = ack(&g);
        wrong.grant_sha256 = "f".repeat(64);
        assert!(j.record(&wrong).is_err());
        j.record(&ack(&g)).unwrap();
        let accepted = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(
            accepted.execution.as_ref().unwrap().attempts[0].state,
            AttemptState::Accepted
        );
        j.record(&ack(&g)).unwrap();
        assert_eq!(
            json!(engine.store.get(&p.plan_id).unwrap()),
            json!(accepted)
        );
        j.record(&done(&g)).unwrap();
        let completed = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(completed.status, PlanStatus::Completed);
        assert_eq!(completed.tasks[0].status, TaskStatus::Completed);
        assert_eq!(completed.tasks[0].assignments.len(), 1);
        assert_eq!(completed.tasks[0].attempts, 1);
        assert_eq!(
            completed.tasks[0].result.as_ref().unwrap().data,
            Value::Null
        );
        assert!(
            !serde_json::to_string(&crate::jobs::view(&completed, true))
                .unwrap()
                .contains("unique-output-marker")
        );
        j.record(&done(&g)).unwrap();
        assert_eq!(
            json!(engine.store.get(&p.plan_id).unwrap()),
            json!(completed)
        );
        let mut another = g.clone();
        another.grant_id = uuid::Uuid::new_v4().to_string();
        assert!(
            engine
                .store
                .change(&p.plan_id, "Must not replay", |p| issue(p, 0, another))
                .is_err()
        );
        assert_eq!(
            json!(engine.store.get(&p.plan_id).unwrap()),
            json!(completed)
        );
    }
    #[tokio::test]
    async fn restart_fences_issued_or_accepted_attempts_without_resuming_even_read_only_work() {
        struct Never(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl crate::task::TaskExecutor for Never {
            async fn execute(&self, _: &crate::task::Task) -> Result<Value> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(Value::Null)
            }
        }
        for accepted in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            drop(rusqlite::Connection::open(&path).unwrap());
            let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
            let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
            let (p, g, j) = issued(&engine);
            if accepted {
                j.record(&ack(&g)).unwrap();
            }
            drop(j);
            drop(engine);
            drop(store);
            let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
            let engine = TaskEngine::with_store(EventBus::new(20), store);
            let interrupted = engine.store.get(&p.plan_id).unwrap();
            assert_eq!(interrupted.status, PlanStatus::Interrupted);
            assert_eq!(interrupted.tasks[0].status, TaskStatus::Interrupted);
            let a = &interrupted.execution.as_ref().unwrap().attempts[0];
            assert_eq!(a.state, AttemptState::Unknown);
            assert_eq!(a.grant, g);
            assert_eq!(a.acknowledgement.is_some(), accepted);
            let never = Arc::new(Never(std::sync::atomic::AtomicUsize::new(0)));
            engine.set_executor(never.clone());
            assert!(engine.resume_job(&p.plan_id).is_err());
            assert!(engine.pause(&p.plan_id).is_err());
            assert!(engine.cancel(&p.plan_id).is_err());
            assert!(engine.run_plan(interrupted.clone()).await.is_err());
            assert_eq!(never.0.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert_eq!(crate::jobs::view(&interrupted, true)["can_resume"], false);
            let j = AttemptJournal {
                store: engine.store.clone(),
                job_id: p.plan_id.clone(),
                grant_id: g.grant_id.clone(),
            };
            assert!(j.record(&done(&g)).is_err());
            assert_eq!(
                json!(engine.store.get(&p.plan_id).unwrap()),
                json!(interrupted)
            );
        }
    }
    #[test]
    fn rejected_and_unknown_results_do_not_fake_success_or_start_new_attempts() {
        for (state, accepted, expected) in [
            (ReceiptState::Rejected, false, PlanStatus::Failed),
            (ReceiptState::Unknown, true, PlanStatus::Interrupted),
        ] {
            let engine = TaskEngine::new(EventBus::new(20));
            let (p, g, j) = issued(&engine);
            if accepted {
                j.record(&ack(&g)).unwrap();
            }
            let r = GrantReport::new(
                &g,
                state,
                Value::Null,
                Some("local_policy_denied_or_changed".into()),
            );
            j.record(&r).unwrap();
            let terminal = engine.store.get(&p.plan_id).unwrap();
            assert_eq!(terminal.status, expected);
            assert!(!terminal.tasks[0].result.as_ref().unwrap().success);
            assert!(
                engine
                    .store
                    .change(&p.plan_id, "No reissue", |p| issue(p, 0, g.clone()))
                    .is_err()
            );
            assert_eq!(
                json!(engine.store.get(&p.plan_id).unwrap()),
                json!(terminal)
            );
        }
    }
    #[test]
    fn issuance_quota_fails_before_any_grant_and_reserved_space_allows_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 20000).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        let p = requested(&engine);
        let g = grant(&p);
        assert!(
            engine
                .store
                .change(&p.plan_id, "Quota denies issuance", |p| issue(p, 0, g))
                .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(p));
        drop(engine);
        let path = dir.path().join("large.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 200000).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let (p, g, j) = issued(&engine);
        // Fill other journal capacity; ACK reduces its pre-reserved footprint.
        loop {
            let other = crate::task::TaskPlanner
                .plan(crate::task::Goal {
                    request_context: None,
                    description: "quota filler".into(),
                    template: "system_info".into(),
                    params: json!({}),
                    constraints: json!({}),
                })
                .unwrap();
            if store.insert(other).is_err() {
                break;
            }
        }
        j.record(&ack(&g)).unwrap();
        let r = GrantReport::new(
            &g,
            ReceiptState::Completed,
            json!({"content":"q".repeat(65000)}),
            None,
        );
        assert!(r.confirms(&g));
        j.record(&r).unwrap();
        assert_eq!(
            engine.store.get(&p.plan_id).unwrap().status,
            PlanStatus::Completed
        );
        assert_eq!(reserved_bytes(&engine.store.get(&p.plan_id).unwrap()), 0);
    }
    #[test]
    fn immutable_authority_assignments_receipts_and_attempt_state_reject_tampering() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g, j) = issued(&engine);
        j.record(&ack(&g)).unwrap();
        let p = engine.store.get(&p.plan_id).unwrap();
        for (field, bad) in [
            ("params", json!({"unapproved":true})),
            ("attempt", json!(2)),
            ("executor_node", json!("other")),
            ("session_id", json!("not-a-session")),
            ("model_id", json!("other-model")),
            ("expires_at_ms", json!(g.expires_at_ms + 1)),
        ] {
            let mut bad_plan = json!(p);
            bad_plan["execution"]["attempts"][0]["grant"][field] = bad;
            let bad: Plan = serde_json::from_value(bad_plan).unwrap();
            assert!(crate::jobs::validate_retained_review(&bad).is_err());
        }
        let mut bad = p.clone();
        bad.tasks[0].assignments[0].node_id = "other".into();
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = p.clone();
        bad.execution.as_mut().unwrap().attempts[0].acknowledgement = None;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = p.clone();
        bad.status = PlanStatus::Interrupted;
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = p.clone();
        bad.execution.as_mut().unwrap().command.approval_sha256 = "f".repeat(64);
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
        let mut bad = p.clone();
        bad.results.insert(g.task_id.clone(), result(&g, &done(&g)));
        assert!(crate::jobs::validate_retained_review(&bad).is_err());
    }
    #[test]
    fn stop_without_issuance_is_cancelled_but_accepted_work_requires_a_final_receipt() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = requested(&engine);
        let guard = engine.claim(&p.plan_id, || Ok(())).unwrap();
        let stopping = engine.cancel(&p.plan_id).unwrap();
        assert_eq!(stopping.status, PlanStatus::Stopping);
        drop(guard);
        assert_eq!(
            engine.store.get(&p.plan_id).unwrap().status,
            PlanStatus::Cancelled
        );
        let (p, g, j) = issued(&engine);
        let guard = engine.claim(&p.plan_id, || Ok(())).unwrap();
        j.record(&ack(&g)).unwrap();
        assert_eq!(
            engine.cancel(&p.plan_id).unwrap().status,
            PlanStatus::Stopping
        );
        j.record(&done(&g)).unwrap();
        drop(guard);
        let settled = engine.store.get(&p.plan_id).unwrap();
        assert_eq!(settled.status, PlanStatus::Cancelled);
        assert_eq!(settled.tasks[0].status, TaskStatus::Completed);
        assert_eq!(
            settled.execution.as_ref().unwrap().attempts[0].state,
            AttemptState::Completed
        );
    }
    #[test]
    fn start_requires_the_exact_approval_receipt_and_never_renews() {
        let engine = TaskEngine::new(EventBus::new(20));
        let p = approved(&engine);
        let a = p.approval.as_ref().unwrap();
        let good = ExecuteCommand {
            id: p.plan_id.clone(),
            expected_revision: p.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            approval_sha256: a.approval_sha256.clone(),
        };
        for (field, value) in [
            ("id", json!(uuid::Uuid::new_v4().simple().to_string())),
            ("expected_revision", json!(p.revision + 1)),
            ("approval_sha256", json!("f".repeat(64))),
        ] {
            let mut bad = json!(good);
            bad[field] = value;
            let bad: ExecuteCommand = serde_json::from_value(bad).unwrap();
            assert!(start(&mut p.clone(), &bad, a.approved_at_ms).is_err());
        }
        assert!(start(&mut p.clone(), &good, a.expires_at_ms).is_err());
        assert!(start(&mut p.clone(), &good, a.approved_at_ms - 1).is_err());
        let started = engine
            .store
            .change(&p.plan_id, "start once", |p| {
                start(p, &good, eefn::protocol::now_ms())
            })
            .unwrap();
        assert!(
            engine
                .store
                .change(&p.plan_id, "cannot start again", |p| start(
                    p,
                    &good,
                    eefn::protocol::now_ms()
                ))
                .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), json!(started));
    }
}
