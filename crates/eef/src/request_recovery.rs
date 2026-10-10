//! Reconcile an idle interrupted attempt from authenticated retained evidence.
use crate::{
    request_execution::{AttemptState, receipt_attempt},
    task::{Plan, PlanStatus, TaskStatus},
};
use anyhow::{Context, Result, bail};
use eefn::{
    request_recovery::{RecoverCommand, RecoveryActor, RecoveryRecord},
    request_result::RetainedResult,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: RecoverCommand,
}
fn actor(origin: Option<&str>) -> RecoveryActor {
    if origin.is_some() {
        RecoveryActor::OriginNode
    } else {
        RecoveryActor::CoordinatorOwner
    }
}
/// Returns true only for an exact durable receipt reuse. No network work here.
pub(crate) fn eligible(
    p: &Plan,
    command: &RecoverCommand,
    origin: Option<&str>,
    unavailable: bool,
) -> Result<bool> {
    command.validate()?;
    let a = receipt_attempt(
        p,
        &eefn::request_receipt::ReceiptCommand {
            id: command.id.clone(),
            grant_id: command.grant_id.clone(),
        },
        origin,
    )?;
    if let Some(r) = &a.recovery {
        if r.command == *command && r.actor == actor(origin) {
            return Ok(true);
        }
        bail!("Recovery already retained; inspect without replacing the receipt")
    }
    let e = p.execution.as_ref().unwrap();
    if unavailable
        || p.status != PlanStatus::Interrupted
        || p.revision != command.expected_revision
        || a.state != AttemptState::Unknown
        || e.abandonment.is_some()
        || e.attempts
            .last()
            .is_none_or(|last| last.grant.grant_id != command.grant_id)
        || crate::request_execution::operation_used(p, &command.operation_id)
        || e.attempts.iter().any(|a| {
            a.recovery
                .as_ref()
                .is_some_and(|r| r.command.operation_id == command.operation_id)
        })
    {
        bail!(
            "Recovery requires the exact idle interrupted revision and uncertain last grant; no job change"
        )
    }
    Ok(false)
}
pub(crate) fn apply(
    p: &mut Plan,
    command: &RecoverCommand,
    origin: Option<&str>,
    unavailable: bool,
    evidence: &RetainedResult,
    now: u64,
) -> Result<bool> {
    if eligible(p, command, origin, unavailable)? {
        return Ok(false);
    }
    let a = p.execution.as_ref().unwrap().attempts.last().unwrap();
    let record = RecoveryRecord {
        schema_version: 1,
        command: command.clone(),
        actor: actor(origin),
        decided_at_ms: now,
        result: evidence.clone(),
    };
    record.validate()?;
    if !evidence.confirms(&a.grant) || now < p.updated_at_ms || now < a.grant.issued_at_ms {
        bail!("Retained result does not confirm the exact issued grant; no job change")
    }
    let report = evidence.report.as_ref().unwrap();
    let result = crate::request_execution::result(&a.grant, report);
    let index = p.execution.as_ref().unwrap().attempts.len() - 1;
    let a = &mut p.execution.as_mut().unwrap().attempts[index];
    a.state = AttemptState::Completed;
    // Preserve the actual ACK and original missing/unknown wire report. Evidence
    // is a separate checkpoint, not a fabricated acknowledgement or new grant.
    a.recovery = Some(record);
    p.tasks[index].status = if report.success {
        TaskStatus::Completed
    } else {
        TaskStatus::Failed
    };
    p.tasks[index].result = Some(result.clone());
    p.results.insert(result.task_id.clone(), result);
    settle(p, report.success);
    Ok(true)
}
fn settle(p: &mut Plan, success: bool) {
    if !success {
        crate::request_execution::cancel_pending(p);
        p.status = PlanStatus::Failed;
    } else if p.tasks.iter().all(|t| t.status == TaskStatus::Completed) {
        p.status = PlanStatus::Completed;
    } else {
        // Completion of this attempt does not authorize any unissued step.
        p.status = PlanStatus::Interrupted;
    }
}
pub(crate) fn validate_record(
    p: &Plan,
    a: &crate::request_execution::GrantedAttempt,
) -> Result<()> {
    let Some(r) = &a.recovery else { return Ok(()) };
    r.validate()?;
    let e = p.execution.as_ref().unwrap();
    if a.state != AttemptState::Completed
        || !r.result.confirms(&a.grant)
        || r.command.id != p.plan_id
        || r.command.grant_id != a.grant.grant_id
        || r.command.expected_revision <= e.command.expected_revision
        || r.command.expected_revision + 1 > p.revision
        || r.decided_at_ms < a.grant.issued_at_ms
        || r.decided_at_ms > p.updated_at_ms
        || crate::request_execution::operation_used(p, &r.command.operation_id)
        || e.abandonment.is_some()
        || e.attempts
            .last()
            .is_none_or(|last| last.grant.grant_id != a.grant.grant_id)
        || a.report
            .as_ref()
            .is_some_and(|report| report.state != eefn::grants::ReceiptState::Unknown)
        || (a.report.is_some() && a.acknowledgement.is_none())
        || matches!(
            p.status,
            PlanStatus::Running | PlanStatus::Stopping | PlanStatus::Abandoned
        )
        || (p.tasks.iter().all(|t| t.status == TaskStatus::Completed)
            && p.status != PlanStatus::Completed)
        || (p.tasks.iter().any(|t| t.status == TaskStatus::Failed)
            && p.status != PlanStatus::Failed)
    {
        bail!("Damaged recovery evidence; retained without dispatch or replay")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remaining_step_policy_holds_unissued_work_or_cancels_it_on_failed_evidence() {
        // Policy-only test: the current concrete compiler does not yet accept
        // implicit camera -> VLM input wiring. Do not present this as a valid
        // compound grant, continuous pipeline or acceptance of that workflow.
        let mut p = crate::task::TaskPlanner
            .plan(crate::task::Goal {
                description: "policy fixture".into(),
                template: "observe_room".into(),
                params: json!({}),
                constraints: json!({}),
                request_context: None,
            })
            .unwrap();
        p.tasks[0].status = TaskStatus::Completed;
        settle(&mut p, true);
        assert_eq!(p.status, PlanStatus::Interrupted);
        assert_eq!(p.tasks[1].status, TaskStatus::Pending);
        assert_eq!(p.tasks[1].attempts, 0);
        assert!(p.tasks[1].assignments.is_empty());
        p.tasks[0].status = TaskStatus::Failed;
        settle(&mut p, false);
        assert_eq!(p.status, PlanStatus::Failed);
        assert_eq!(p.tasks[1].status, TaskStatus::Cancelled);
        assert_eq!(p.tasks[1].attempts, 0);
    }
}
impl crate::Runtime {
    pub async fn request_recovery_command(&self, request: RecoveryRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Recovery schema/runtime changed")
        }
        self.recover_request(request.command, None).await
    }
    pub(crate) async fn recover_request(
        &self,
        command: RecoverCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        // Two admissions: no lock spans the read-only remote inspection. Recheck
        // exact source revision under the active/shutdown gate before committing.
        let (source, reused) = self.engine.recovery_source(&command, origin)?;
        let (saved, reused) = if reused {
            (source, true)
        } else {
            let grant = &source
                .execution
                .as_ref()
                .unwrap()
                .attempts
                .last()
                .unwrap()
                .grant;
            let evidence = self.node_server.inspect_grant_result(grant).await?;
            self.engine.recover_reviewed(&command, origin, &evidence)?
        };
        let r = saved
            .execution
            .as_ref()
            .unwrap()
            .attempts
            .iter()
            .find_map(|a| a.recovery.as_ref().filter(|r| r.command == command))
            .context("Missing saved recovery receipt")?;
        Ok(json!(eefn::request_recovery::RecoverReport {
            schema_version: 1, report_type: "coordinator_request_recovery".into(), success: true,
            runtime_id: self.instance_id.clone(), id: saved.plan_id.clone(), revision: saved.revision,
            status: serde_json::to_value(saved.status)?.as_str().unwrap().into(),
            request_context: saved.goal.request_context.clone().context("Missing original context")?,
            command: command.clone(), actor: r.actor, applied_revision: command.expected_revision + 1,
            result_sha256: r.result.report_sha256.clone().context("Missing result checksum")?, reused,
            persisted: true, reconciled: true, execution_requested: false, automatically_retried: false,
            remote_stop_confirmed: false,
            note: "Exact retained node completion explicitly reconciled. Original grant, ACK and uncertain report remain unchanged. Unissued steps stay held; no replay, continuation, lease renewal, rollback or remote-stop proof.".into(),
        }))
    }
}
