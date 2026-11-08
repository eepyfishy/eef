//! Confirmed refusal before this attempt enters the review checkpoint. It is
//! not evidence that no concurrent or earlier request exists under this receipt.
use super::{CameraCommand, CameraInput};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    #[error("The source cannot currently be inspected for this request.")]
    SourceUnavailable,
    #[error("The source node is unavailable or not disclosed to this request origin.")]
    SourceUnavailableOrNotDisclosed,
    #[error("No available configured camera matches this request.")]
    ResourceUnavailable,
    #[error("More than one camera matches; select an exact resource.")]
    ResourceAmbiguous,
    #[error("The source node has unapplied resource settings.")]
    BindingNotApplied,
    #[error("This operation ID already belongs to different saved input.")]
    IntentConflict,
    #[error("The saved request has no compiled plan; inspect its current status.")]
    PreparationIncomplete,
}
impl RefusalReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "camera_source_unavailable",
            Self::SourceUnavailableOrNotDisclosed => "camera_source_unavailable_or_not_disclosed",
            Self::ResourceUnavailable => "camera_resource_unavailable",
            Self::ResourceAmbiguous => "camera_resource_ambiguous",
            Self::BindingNotApplied => "camera_binding_not_applied",
            Self::IntentConflict => "camera_intent_conflict",
            Self::PreparationIncomplete => "camera_preparation_incomplete",
        }
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::SourceUnavailable => {
                "Check that the source node is connected and paired. On that node: eefn connection show"
            }
            Self::SourceUnavailableOrNotDisclosed => {
                "Check source connectivity and ask the coordinator owner to verify node disclosure for this origin."
            }
            Self::ResourceUnavailable => {
                "Inspect configured resources on the source node: eefn workloads camera-sources"
            }
            Self::ResourceAmbiguous => {
                "Inspect source resources with eefn workloads camera-sources, then select one with --camera RESOURCE_ID."
            }
            Self::BindingNotApplied => {
                "Apply the saved source configuration with an explicit source-node restart: eefn restart"
            }
            Self::IntentConflict => {
                "Inspect the original receipt. Changed settings require a separately intended new request and operation ID."
            }
            Self::PreparationIncomplete => {
                "Receipt reuse does not continue compilation. Inspect the saved job, then explicitly prepare it with eefn requests compile-saved JOB_ID --revision REVISION or cancel it."
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub schema_version: u32,
    pub report_type: String,
    pub origin_node: String,
    pub operation_id: String,
    pub input: CameraInput,
    pub reason: RefusalReason,
    pub review_checkpoint_attempted: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
impl Refusal {
    pub fn new(node: &str, command: &CameraCommand, reason: RefusalReason) -> Result<Self> {
        crate::network::validate_remote_node_id(node)?;
        command.validate()?;
        Ok(Self {
            schema_version: 1,
            report_type: "coordinator_camera_refusal".into(),
            origin_node: node.into(),
            operation_id: command.operation_id.clone(),
            input: command.input.clone(),
            reason,
            review_checkpoint_attempted: false,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
        })
    }
}
pub fn confirmed(value: &Value, node: &str, command: &CameraCommand) -> bool {
    let Ok(report) = serde_json::from_value::<Refusal>(value.clone()) else {
        return false;
    };
    Refusal::new(node, command, report.reason).is_ok_and(|expected| expected == report)
}
pub fn refused(node: &str, command: &CameraCommand, reason: RefusalReason) -> Result<Value> {
    Ok(json!(Refusal::new(node, command, reason)?))
}
