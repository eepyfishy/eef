//! Read-only interrupted workload evidence. A successful read is not completed
//! work, reconciliation, renewed authority or permission to replay a model call.
use crate::{
    grants::workload::{VisionHistory, VisionInspection},
    workload_leases::definition::Definition,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const NOTE: &str = "Interrupted workload; coordinator completion is unconfirmed. This is a read-only metadata/retained-analysis snapshot. Missing data does not prove no effects, and even completed node evidence does not complete or reconcile this job. No source/model execution, lease renewal, automatic retry or replay was requested.";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisAvailability {
    NotApplicable,
    UnavailableOrUnconfirmed,
    RetainedSnapshot,
}
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
    pub job_status: String,
    pub coordinator_attempt_state: String,
    pub metadata: crate::workload_network::EventSummary,
    pub analysis_availability: AnalysisAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<VisionInspection>,
    pub job_completed: bool,
    pub outcome_confirmed: bool,
    pub read_only: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub model_replay_authorized: bool,
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
                && self.report_type == "coordinator_workload_uncertain_result"
                && self.success
                && self.id == command.id
                && self.definition.job_id == command.id
                && self.definition.grant_id == command.grant_id
                && self.revision > 0
                && self.request_context == self.definition.request_context
                && origin.is_none_or(|o| o == self.request_context.origin_node())
                && runtime.is_none_or(|r| r == self.runtime_id)
                && self.job_status == "interrupted"
                && self.coordinator_attempt_state == "unknown"
                && !self.job_completed
                && !self.outcome_confirmed
                && self.read_only
                && !self.execution_requested
                && !self.automatically_retried
                && !self.reconciled
                && !self.model_replay_authorized
                && self.note == NOTE,
            "Unconfirmed interrupted workload result scope or authority flags"
        );
        let m = &self.metadata;
        ensure!(
            m.count <= self.definition.events.max_events
                && m.bytes <= self.definition.events.max_total_bytes
                && m.bytes >= usize::try_from(m.count)?.saturating_mul(128)
                && crate::request_result::valid_digest(&m.prefix_sha256),
            "Invalid interrupted metadata summary"
        );
        if m.count == 0 {
            let empty = crate::workload_network::summarize_events(&self.definition, &[])?;
            ensure!(
                m.bytes == 0 && m.prefix_sha256 == empty.prefix_sha256,
                "Invalid empty interrupted metadata prefix"
            );
        }
        let budget = match (
            self.definition.schema_version,
            self.analysis_availability,
            &self.vision,
        ) {
            (2, AnalysisAvailability::NotApplicable, None) => 0,
            (3, AnalysisAvailability::UnavailableOrUnconfirmed, None) => {
                VisionHistory::max_bytes(&self.definition)?
            }
            (3, AnalysisAvailability::RetainedSnapshot, Some(vision)) => {
                vision.validate(&self.definition)?;
                VisionHistory::max_bytes(&self.definition)?
            }
            _ => {
                anyhow::bail!("Interrupted analysis availability differs from its source/evidence")
            }
        };
        ensure!(
            serde_json::to_vec(self)?.len()
                <= budget
                    .checked_add(128 * 1024)
                    .ok_or_else(|| anyhow::anyhow!("Interrupted result bound overflow"))?,
            "Interrupted result exceeds reviewed bounds"
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
