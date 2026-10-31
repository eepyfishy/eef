//! Existing receipt commands can inspect workload history without a live issuer.
use crate::{Runtime, task::Plan};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub(crate) fn retained<'a>(
    plan: &'a Plan,
    command: &eefn::request_receipt::ReceiptCommand,
    origin: Option<&str>,
) -> Result<&'a crate::workload_jobs::Execution> {
    command.validate()?;
    if plan.plan_id != command.id
        || origin.is_some_and(|id| {
            plan.goal
                .request_context
                .as_ref()
                .is_none_or(|c| c.origin_node() != id)
        })
    {
        bail!("Request not found for this origin")
    }
    crate::request_execution::validate_retained(plan)?;
    let execution = plan
        .execution
        .as_ref()
        .and_then(|e| e.workload.as_ref())
        .context("No issued workload")?;
    if execution.definition.grant_id != command.grant_id {
        bail!("Issued workload not found for this request")
    }
    Ok(execution)
}
impl Runtime {
    pub(crate) async fn inspect_workload_receipt(
        &self,
        plan: &Plan,
        command: &eefn::request_receipt::ReceiptCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        let execution = retained(plan, command, origin)?;
        let receipt = self
            .node_server
            .inspect_workload_receipt(&execution.definition)
            .await?;
        report(plan, command, origin, &self.instance_id, receipt)
    }
}
pub(crate) fn report(
    plan: &Plan,
    command: &eefn::request_receipt::ReceiptCommand,
    origin: Option<&str>,
    runtime: &str,
    receipt: eefn::grants::workload::Receipt,
) -> Result<Value> {
    let execution = retained(plan, command, origin)?;
    receipt.validate()?;
    if receipt.definition != execution.definition {
        bail!("Retained workload differs from original issuance")
    }
    let report = json!(eefn::workload_inspection::CoordinatorReport {
        schema_version: 2,
        report_type: "coordinator_workload_receipt".into(),
        success: true,
        runtime_id: runtime.into(),
        id: plan.plan_id.clone(),
        revision: plan.revision,
        request_context: plan
            .goal
            .request_context
            .clone()
            .context("Request context missing")?,
        coordinator_attempt_state: serde_json::to_value(execution.state)?
            .as_str()
            .context("Workload state unavailable")?
            .into(),
        receipt,
        read_only: true,
        execution_requested: false,
        automatically_retried: false,
        reconciled: false,
        note: eefn::workload_inspection::NOTE.into(),
    });
    if !eefn::request_receipt::confirmed(&report, command, origin, Some(runtime)) {
        bail!("Workload receipt confirmation failed")
    }
    Ok(report)
}
