//! Read-only version-two workload evidence over the existing authenticated frame.
//! Registration advertises inspection separately from unimplemented execution.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOTE: &str = "Read-only retained workload evidence. Coordinator tracking is unchanged. Historical completion does not authorize execution, renewal, restart or replay, and does not prove physical effects stopped. Missing evidence does not prove no work occurred.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub receipt_versions: Vec<u32>,
    pub read_only: bool,
}
impl Advertisement {
    pub fn new(available: bool) -> Self {
        Self {
            schema_version: 2,
            available,
            receipt_versions: vec![2, 3],
            read_only: true,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 2 || self.receipt_versions != [2, 3] || !self.read_only {
            bail!("Unsupported workload inspection; no ordinary grant fallback")
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub scope: crate::grants::stop::Scope,
}
impl WireRequest {
    pub fn validate(&self, issuer: &str, node: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        self.scope.validate()?;
        // Historical scope.session_id is evidence only. The current authenticated
        // channel independently pins issuer/node and correlates the response.
        if self.kind != "workload_receipt"
            || self.scope.issuer_id != issuer
            || self.scope.executor_node != node
        {
            bail!("Workload inspection requires the original pinned issuer and exact node")
        }
        Ok(())
    }
    pub fn read(
        &self,
        ledger: &crate::grants::GrantLedger,
        issuer: &str,
        node: &str,
    ) -> Result<crate::grants::workload::Receipt> {
        self.validate(issuer, node)?;
        let receipt = ledger.workload_receipt(issuer, &self.scope.grant_id)?;
        if receipt.definition.scope()? != self.scope {
            bail!("Retained workload definition differs; no fallback or recovery")
        }
        Ok(receipt)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub request_context: crate::context::RequestContext,
    pub coordinator_attempt_state: String,
    pub receipt: crate::grants::workload::Receipt,
    pub read_only: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    command: &crate::request_receipt::ReceiptCommand,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(report) = serde_json::from_value::<CoordinatorReport>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && report.receipt.validate().is_ok()
        && report.schema_version == 2
        && report.report_type == "coordinator_workload_receipt"
        && report.success
        && report.id == command.id
        && report.receipt.definition.job_id == command.id
        && report.receipt.definition.grant_id == command.grant_id
        && report.request_context == report.receipt.definition.request_context
        && report.revision > 0
        && crate::automation_occurrence::uuid(&report.runtime_id).is_ok()
        && runtime.is_none_or(|id| id == report.runtime_id)
        && origin.is_none_or(|id| id == report.request_context.origin_node())
        && matches!(
            report.coordinator_attempt_state.as_str(),
            "issued" | "accepted" | "completed" | "unknown"
        )
        && report.read_only
        && !report.execution_requested
        && !report.automatically_retried
        && !report.reconciled
        && report.note == NOTE
}
