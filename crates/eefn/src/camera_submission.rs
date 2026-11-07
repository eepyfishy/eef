//! Typed camera-source authoring. No capture, permission or execution authority.
use crate::{
    request_submission::{ConcreteCommand, ConcreteInput},
    workload_leases::definition::{
        EventMode, Events, Limits, MotionParameters, Overflow, Source, Template,
    },
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[path = "camera_outcomes.rs"]
pub mod outcomes;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraInput {
    pub schema_version: u32,
    pub original_text: String,
    pub title: String,
    #[serde(default)]
    pub source_node: Option<String>,
    #[serde(default)]
    pub camera: Option<String>,
    pub priority: i32,
    pub parameters: MotionParameters,
    pub limits: Limits,
    pub events: Events,
}
fn resource(node: &str, id: &str) -> Result<String> {
    let local = if let Some((owner, local)) = id.split_once("::") {
        ensure!(owner == node, "Camera resource belongs to another node");
        local
    } else {
        id
    };
    ensure!(
        !local.is_empty()
            && local.len() <= 100
            && local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "Camera selector must be a local or qualified resource ID"
    );
    Ok(crate::context::resource_id(node, local))
}
impl CameraInput {
    pub fn source<'a>(&'a self, origin: &'a str) -> &'a str {
        self.source_node.as_deref().unwrap_or(origin)
    }
    fn template(&self, node: &str, id: String, binding: String, adapter: String) -> Template {
        Template {
            schema_version: 2,
            executor_node: node.into(),
            priority: self.priority,
            source: Source::CameraMotion {
                resource_id: id,
                resource_sha256: binding,
                adapter_sha256: adapter,
                parameters: self.parameters.clone(),
            },
            limits: self.limits.clone(),
            events: self.events.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        crate::interpretation::messages(&self.original_text, &[])?;
        let node = self.source_node.as_deref().unwrap_or("source");
        crate::network::validate_remote_node_id(node)?;
        if let Some(id) = &self.camera {
            // With an implicit source, validate a qualified selector's owner now;
            // binding it to the actual origin remains a separate mandatory check.
            let owner = self
                .source_node
                .as_deref()
                .or_else(|| id.split_once("::").map(|v| v.0))
                .unwrap_or(node);
            crate::network::validate_remote_node_id(owner)?;
            resource(owner, id)?;
        }
        ensure!(
            self.schema_version == 1
                && !self.title.trim().is_empty()
                && self.title.len() <= 512
                && !self.title.chars().any(char::is_control)
                && serde_json::to_vec(self)?.len() <= 32 * 1024,
            "Invalid typed camera request"
        );
        self.template(
            node,
            crate::context::resource_id(node, "camera"),
            "a".repeat(64),
            "b".repeat(64),
        )
        .validate()
    }
    /// Validate retained choices without querying a provider or granting work.
    pub fn concrete(&self, template: &Template, origin: Option<&str>) -> Result<ConcreteInput> {
        self.validate()?;
        template.validate()?;
        let node = self
            .source_node
            .as_deref()
            .or(origin)
            .unwrap_or(&template.executor_node);
        let Source::CameraMotion {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &template.source
        else {
            bail!("Camera intent cannot bind a microphone workload")
        };
        ensure!(
            template.executor_node == node
                && self
                    .camera
                    .as_deref()
                    .is_none_or(|id| resource(node, id).is_ok_and(|id| id == *resource_id))
                && self.template(
                    node,
                    resource_id.clone(),
                    resource_sha256.clone(),
                    adapter_sha256.clone()
                ) == *template,
            "Retained camera choices differ from immutable typed input"
        );
        let input = ConcreteInput {
            schema_version: 1,
            original_text: self.original_text.clone(),
            title: self.title.clone(),
            input: crate::request_planning::PlanInput {
                schema_version: 1,
                template: crate::workload_leases::definition::TEMPLATE.into(),
                params: json!({"workload":template}),
                constraints: json!({}),
            },
        };
        input.validate()?;
        Ok(input)
    }
    pub fn resolve(
        &self,
        origin: &str,
        snapshot: &crate::camera_sources::Report,
    ) -> Result<ConcreteInput> {
        self.validate()?;
        let node = self.source(origin);
        snapshot.validate(&crate::workload_sources::Request {
            schema_version: 2,
            expected_node_id: node.into(),
            expected_runtime_id: snapshot.runtime_id.clone(),
        })?;
        if !snapshot.local_bindings_current {
            return Err(outcomes::RefusalReason::BindingNotApplied.into());
        }
        let wanted = self
            .camera
            .as_deref()
            .map(|id| resource(node, id))
            .transpose()?;
        let choices = snapshot
            .resources
            .iter()
            .filter(|r| {
                r.available
                    && r.resource_sha256.is_some()
                    && wanted.as_ref().is_none_or(|id| *id == r.resource_id)
            })
            .collect::<Vec<_>>();
        if choices.is_empty() {
            return Err(outcomes::RefusalReason::ResourceUnavailable.into());
        }
        if choices.len() != 1 {
            return Err(outcomes::RefusalReason::ResourceAmbiguous.into());
        }
        let selected = choices[0];
        let template = self.template(
            node,
            selected.resource_id.clone(),
            selected.resource_sha256.clone().unwrap(),
            snapshot.adapter_sha256.clone(),
        );
        self.concrete(&template, Some(origin))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraCommand {
    pub operation_id: String,
    pub input: CameraInput,
}
impl CameraCommand {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        self.input.validate()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: CameraCommand,
}

#[derive(Clone, Debug, clap::Args)]
pub struct CameraSubmission {
    #[command(flatten)]
    pub source: Box<CameraSourceOptions>,
    #[command(flatten)]
    pub capture: Box<CameraCaptureOptions>,
    #[command(flatten)]
    pub limits: Box<CameraLimitOptions>,
    #[command(flatten)]
    pub events: Box<CameraEventOptions>,
    /// Preserve this receipt after uncertainty; never generate another to retry.
    #[arg(long)]
    pub operation_id: Option<String>,
}
// Separate flattened groups bound each derive parser's stack frame while keeping
// the same flat CLI flags and the existing one-MiB Windows parser requirement.
#[derive(Clone, Debug, clap::Args)]
pub struct CameraSourceOptions {
    /// Defaults to this node; another source must be disclosed to this origin.
    #[arg(long)]
    pub source_node: Option<String>,
    /// Omit only when the source has exactly one available configured camera.
    #[arg(long)]
    pub camera: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, default_value = "Camera activity")]
    pub title: String,
    #[arg(long, default_value_t = 0)]
    pub priority: i32,
}
#[derive(Clone, Debug, clap::Args)]
pub struct CameraCaptureOptions {
    #[arg(long, default_value_t = 640)]
    pub width: u32,
    #[arg(long, default_value_t = 480)]
    pub height: u32,
    #[arg(long, default_value_t = 64)]
    pub sample_width: u32,
    #[arg(long, default_value_t = 48)]
    pub sample_height: u32,
    #[arg(long, default_value_t = 30)]
    pub frames: u32,
    #[arg(long, default_value_t = 200)]
    pub interval_ms: u64,
    #[arg(long, default_value_t = 15)]
    pub activation_level: u32,
    #[arg(long, default_value_t = 5)]
    pub deactivation_level: u32,
    #[arg(long, default_value_t = 2000)]
    pub io_timeout_ms: u64,
    #[arg(long, default_value_t = 10000)]
    pub max_duration_ms: u64,
}
use crate::microphone_submission::{
    MicrophoneEventOptions as CameraEventOptions, MicrophoneLimitOptions as CameraLimitOptions,
};
impl CameraSubmission {
    pub fn command(&self) -> Result<CameraCommand> {
        let source = &self.source;
        let capture = &self.capture;
        let limits = &self.limits;
        let events = &self.events;
        let command = CameraCommand {
            operation_id: self
                .operation_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            input: CameraInput {
                schema_version: 1,
                original_text: source.text.clone().unwrap_or_else(|| {
                    format!(
                        "Review up to {} camera motion observations from {}.",
                        capture.frames,
                        source.source_node.as_deref().unwrap_or("this node")
                    )
                }),
                title: source.title.clone(),
                source_node: source.source_node.clone(),
                camera: source.camera.clone(),
                priority: source.priority,
                parameters: MotionParameters {
                    width: capture.width,
                    height: capture.height,
                    sample_width: capture.sample_width,
                    sample_height: capture.sample_height,
                    max_frames: capture.frames,
                    interval_ms: capture.interval_ms,
                    activation_level: capture.activation_level,
                    deactivation_level: capture.deactivation_level,
                    io_timeout_ms: capture.io_timeout_ms,
                    max_duration_ms: capture.max_duration_ms,
                },
                limits: Limits {
                    max_session_ms: limits.max_session_ms,
                    max_lease_ms: limits.max_lease_ms,
                    max_job_memory_mib: limits.max_job_memory_mib,
                    max_source_frames: u64::from(capture.frames),
                },
                events: Events {
                    mode: EventMode::ActivityTransitions,
                    min_interval_ms: events.min_interval_ms,
                    max_events: events.max_events,
                    max_event_bytes: events.max_event_bytes,
                    max_total_bytes: events.max_total_bytes,
                    max_pending: events.max_pending,
                    overflow: Overflow::Stop,
                },
            },
        };
        command.validate()?;
        Ok(command)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraReport {
    pub schema_version: u32,
    pub report_type: String,
    pub operation_id: String,
    pub input: CameraInput,
    pub concrete: ConcreteInput,
    pub result: Value,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, command: &CameraCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() || command.validate().is_err() {
        return false;
    }
    let Ok(report) = serde_json::from_value::<CameraReport>(value.clone()) else {
        return false;
    };
    let Ok(template) = Template::from_params(&report.concrete.input.params) else {
        return false;
    };
    report.schema_version == 1
        && report.report_type == "coordinator_camera_request"
        && report.operation_id == command.operation_id
        && report.input == command.input
        && !report.execution_authorized
        && !report.dispatched
        && !report.automatically_retried
        && command
            .input
            .concrete(&template, Some(node))
            .is_ok_and(|v| v == report.concrete)
        && crate::request_submission::confirmed_with_camera(
            &report.result,
            node,
            &ConcreteCommand {
                operation_id: command.operation_id.clone(),
                request: report.concrete,
            },
            &command.input,
        )
}
const NOTE: &str = "Camera intent saved/resolved/compiled only. Separate owner approval and explicit execution are required. After uncertainty inspect jobs find with this operation ID, then requests plan. Retained choices are never automatically re-resolved or replayed; no capture, permission, inference or retry fallback.";
const REFUSAL_NOTE: &str = "The coordinator refused this attempt before entering the review checkpoint. Earlier or concurrent requests may still exist. Inspect the operation receipt; no automatic retry, capture, permission, approval or execution was requested.";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    command: CameraCommand,
    success: bool,
    coordinator_acknowledged: bool,
    outcome_unknown: bool,
    result: Option<Value>,
    error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refusal: Option<outcomes::Refusal>,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed_node(value: &Value, node: &str, command: &CameraCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() || command.validate().is_err() {
        return false;
    }
    let Ok(r) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    r.schema_version == 1
        && r.report_type == "node_camera_request"
        && r.node_id == node
        && r.command == *command
        && !r.execution_authorized
        && !r.dispatched
        && !r.automatically_retried
        && r.note
            == (if r.refusal.is_some() {
                REFUSAL_NOTE
            } else {
                NOTE
            })
        && if let Some(refusal) = &r.refusal {
            !r.success
                && r.coordinator_acknowledged
                && !r.outcome_unknown
                && r.result.is_none()
                && r.error_code.as_deref() == Some(refusal.reason.code())
                && outcomes::confirmed(&json!(refusal), node, command)
        } else if r.success {
            r.coordinator_acknowledged
                && !r.outcome_unknown
                && r.error_code.is_none()
                && r.result
                    .as_ref()
                    .is_some_and(|v| confirmed(v, node, command))
        } else {
            !r.coordinator_acknowledged
                && r.result.is_none()
                && r.error_code.as_deref()
                    == Some(if r.outcome_unknown {
                        "camera_authoring_unconfirmed"
                    } else {
                        "camera_authoring_not_sent"
                    })
        }
}
pub fn unavailable(node: &str, command: CameraCommand, unknown: bool) -> Value {
    json!(NodeReport {
        schema_version: 1,
        report_type: "node_camera_request".into(),
        node_id: node.into(),
        command,
        success: false,
        coordinator_acknowledged: false,
        outcome_unknown: unknown,
        result: None,
        refusal: None,
        error_code: Some(
            if unknown {
                "camera_authoring_unconfirmed"
            } else {
                "camera_authoring_not_sent"
            }
            .into()
        ),
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false,
        note: NOTE.into()
    })
}
impl crate::NodeService {
    pub async fn camera_request(&self, request: CameraRequest) -> Result<Value> {
        ensure!(
            request.schema_version == 1 && request.expected_node_id == self.node_id,
            "Camera authoring schema/target mismatch"
        );
        request.command.validate()?;
        let response = self
            .submissions
            .request_correlated(
                "requests.camera",
                json!(request.command.input),
                Some(request.command.operation_id.clone()),
            )
            .await;
        let value = match response {
            Ok(value) if confirmed(&value, &self.node_id, &request.command) => json!(NodeReport {
                schema_version: 1,
                report_type: "node_camera_request".into(),
                node_id: self.node_id.clone(),
                command: request.command.clone(),
                success: true,
                coordinator_acknowledged: true,
                outcome_unknown: false,
                result: Some(value),
                error_code: None,
                refusal: None,
                execution_authorized: false,
                dispatched: false,
                automatically_retried: false,
                note: NOTE.into()
            }),
            Ok(value) if outcomes::confirmed(&value, &self.node_id, &request.command) => {
                let refusal: outcomes::Refusal = serde_json::from_value(value)?;
                json!(NodeReport {
                    schema_version: 1,
                    report_type: "node_camera_request".into(),
                    node_id: self.node_id.clone(),
                    command: request.command.clone(),
                    success: false,
                    coordinator_acknowledged: true,
                    outcome_unknown: false,
                    result: None,
                    error_code: Some(refusal.reason.code().into()),
                    refusal: Some(refusal),
                    execution_authorized: false,
                    dispatched: false,
                    automatically_retried: false,
                    note: REFUSAL_NOTE.into()
                })
            }
            Err(error)
                if matches!(
                    error.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                ) =>
            {
                unavailable(&self.node_id, request.command.clone(), false)
            }
            _ => unavailable(&self.node_id, request.command.clone(), true),
        };
        ensure!(
            confirmed_node(&value, &self.node_id, &request.command),
            "Unconfirmed camera authoring checkpoint"
        );
        Ok(value)
    }
}
pub fn render_node(value: &Value) -> Result<String> {
    let r: NodeReport = serde_json::from_value(value.clone())?;
    ensure!(
        confirmed_node(value, &r.node_id, &r.command),
        "Unconfirmed camera authoring report"
    );
    if let Some(refusal) = r.refusal {
        Ok(format!(
            "Camera preparation refused\n{}\nSource node: {}\n{}\nThis attempt did not enter the review checkpoint. An earlier or concurrent request may still exist.\nInspect receipt: eefn jobs find --operation-id {}\nNo automatic retry, approval or execution was requested.\n",
            refusal.reason,
            r.command.input.source(&r.node_id),
            refusal.reason.guidance(),
            r.command.operation_id
        ))
    } else if let Some(result) = r.result {
        let report: CameraReport = serde_json::from_value(result)?;
        let template = Template::from_params(&report.concrete.input.params)?;
        let id = report.result["plan"]["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Camera job ID unavailable"))?;
        Ok(format!(
            "{} camera activity request\nSource node: {} | Resource: {}\nJob: {}\nNo capture, permission, approval or execution was requested.\nInspect: eefn requests plan {}\nCoordinator owner approval remains separate: eef requests inspect {}\n",
            if report.result["creation_reused"] == true {
                "Retained"
            } else {
                "Prepared"
            },
            template.executor_node,
            template.resource_id(),
            id,
            id,
            id
        ))
    } else {
        Ok(format!(
            "Camera authoring {}\nOperation ID: {}\nInspect before deciding what to do: eefn jobs find --operation-id {}\nKeep this receipt. No automatic retry, capture, permission, approval or execution was requested.\n",
            if r.outcome_unknown {
                "was not confirmed"
            } else {
                "was not sent"
            },
            r.command.operation_id,
            r.command.operation_id
        ))
    }
}

#[cfg(test)]
#[path = "camera_submission_tests.rs"]
mod tests;
