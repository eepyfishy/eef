//! Read-only coordinator-persisted workload output for the existing result
//! command. Completion and availability never create execution/replay authority.
use crate::{
    grants::workload::{Receipt, VisionHistory},
    workload_leases::definition::Definition,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOTE: &str = "Read-only coordinator-persisted workload results. No source or model was contacted, no work was reconciled or replayed, and physical effects or model quality are not proven.";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub request_context: crate::context::RequestContext,
    pub definition: Definition,
    pub receipt: crate::request_receipt::ConsumedReceipt,
    pub completion: crate::grants::workload::CompletionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<crate::workload_network::EventSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisionHistory>,
    pub read_only: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
impl Report {
    pub fn validate(
        &self,
        command: &crate::request_result::ResultCommand,
        origin: Option<&str>,
        runtime: Option<&str>,
    ) -> Result<()> {
        command.validate()?;
        self.definition.validate()?;
        crate::automation_occurrence::uuid(&self.runtime_id)?;
        ensure!(
            self.schema_version == 1
                && self.report_type == "coordinator_workload_result"
                && self.success
                && self.id == command.id
                && self.definition.job_id == command.id
                && self.definition.grant_id == command.grant_id
                && self.request_context == self.definition.request_context
                && self.revision > 0
                && origin.is_none_or(|o| o == self.request_context.origin_node())
                && runtime.is_none_or(|r| r == self.runtime_id)
                && self.receipt.state == "completed"
                && self.read_only
                && !self.execution_requested
                && !self.automatically_retried
                && !self.reconciled
                && self.note == NOTE,
            "Unconfirmed scoped workload result"
        );
        Receipt {
            schema_version: 3,
            definition: self.definition.clone(),
            receipt: self.receipt.clone(),
            completion: Some(self.completion.clone()),
            execution_authorized: false,
            restart_authorized: false,
        }
        .validate()?;
        if self.definition.schema_version == 3 {
            ensure!(
                self.metadata.is_some(),
                "Vision result lacks its durable metadata"
            );
        }
        if let Some(events) = &self.metadata {
            ensure!(
                self.completion.event_count == events.count
                    && self.completion.event_bytes == events.bytes
                    && events.count > 0
                    && events.count <= self.definition.events.max_events
                    && events.bytes <= self.definition.events.max_total_bytes
                    && self.completion.event_payload_prefix_sha256.as_deref()
                        == Some(events.prefix_sha256.as_str()),
                "Workload result differs from retained metadata"
            );
        }
        let vision_bytes = match (&self.vision, self.definition.schema_version) {
            (Some(history), 3) => {
                history.validate(&self.definition)?;
                ensure!(
                    history.receipt == self.receipt
                        && serde_json::to_value(&history.completion)?
                            == serde_json::to_value(&self.completion)?,
                    "Workload result differs from retained analysis"
                );
                VisionHistory::max_bytes(&self.definition)?
            }
            (None, 2) => 0,
            _ => anyhow::bail!("Workload result source and analysis differ"),
        };
        ensure!(
            serde_json::to_vec(self)?.len()
                <= vision_bytes
                    .checked_add(128 * 1024)
                    .ok_or_else(|| anyhow::anyhow!("Workload result bound overflow"))?,
            "Workload result exceeds reviewed output bound"
        );
        Ok(())
    }
}
pub fn confirmed(
    value: &Value,
    command: &crate::request_result::ResultCommand,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    serde_json::from_value::<Report>(value.clone())
        .is_ok_and(|r| r.validate(command, origin, runtime).is_ok())
}
