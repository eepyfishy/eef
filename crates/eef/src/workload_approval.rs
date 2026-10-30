//! Owner-only conditional session approval in the existing durable job journal.
use crate::{task::Plan, workload_jobs::ApprovalCommand};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: ApprovalCommand,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    success: bool,
    runtime_id: String,
    id: String,
    operation_id: String,
    plan: Plan,
    reused: bool,
    persisted: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
}
fn report(plan: Plan, command: &ApprovalCommand, runtime: &str, reused: bool) -> Value {
    json!(Report {
        schema_version: 2,
        report_type: "workload_approval".into(),
        success: true,
        runtime_id: runtime.into(),
        id: command.id.clone(),
        operation_id: command.operation_id.clone(),
        plan,
        reused,
        persisted: true,
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false,
    })
}
pub fn confirmed(value: &Value, runtime: &str, command: &ApprovalCommand) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && eefn::automation_occurrence::uuid(runtime).is_ok()
        && r.schema_version == 2
        && r.report_type == "workload_approval"
        && r.success
        && r.runtime_id == runtime
        && r.id == command.id
        && r.operation_id == command.operation_id
        && r.plan.plan_id == command.id
        && r.persisted
        && !r.execution_authorized
        && !r.dispatched
        && !r.automatically_retried
        && (r.reused
            || (command.expected_revision.checked_add(1) == Some(r.plan.revision)
                && r.plan.status == crate::task::PlanStatus::Approved
                && r.plan.execution.is_none()))
        && json!(r.plan) == value["plan"]
        && crate::jobs::validate_retained_review(&r.plan).is_ok()
        && r.plan
            .approval
            .as_ref()
            .is_some_and(|a| command.confirms(a))
}

impl crate::Runtime {
    // Only the existing loopback owner API calls this operation. Node-origin
    // planning messages have no approval variant and cannot reach this path.
    pub async fn workload_approval(&self, request: Request) -> Result<Value> {
        ensure!(
            request.schema_version == 2 && request.expected_runtime_id == self.instance_id,
            "Session approval schema/runtime changed; inspect before retrying"
        );
        request.command.validate()?;
        let saved = self.engine.store.get(&request.command.id)?;
        if let Some(a) = &saved.approval {
            // Exact retries inspect retained authority, even offline, expired or
            // cancelled. They never refresh source pins or approval timestamps.
            let plan = crate::workload_jobs::approve(&self.engine, &request.command, &a.issuer_id)?;
            return Ok(report(plan, &request.command, &self.instance_id, true));
        }
        let template =
            eefn::workload_leases::definition::Template::from_params(&saved.goal.params)?;
        ensure!(
            saved.goal.template == eefn::workload_leases::definition::TEMPLATE,
            "Use ordinary approval for an operation plan"
        );
        let before = self
            .node_server
            .grant_session(&template.executor_node)
            .await?;
        let source = self
            .node_server
            .workload_sources(&template.executor_node, 5)
            .await?;
        source.confirms_template(&template)?;
        let after = self
            .node_server
            .grant_session(&template.executor_node)
            .await?;
        ensure!(
            before == after,
            "Source authentication changed during approval inspection"
        );
        let issuer = after["issuer_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing grant issuer"))?;
        let plan = crate::workload_jobs::approve(&self.engine, &request.command, issuer)?;
        Ok(report(plan, &request.command, &self.instance_id, false))
    }
}
