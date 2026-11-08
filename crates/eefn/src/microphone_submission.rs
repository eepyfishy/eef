//! Typed microphone-source authoring. No capture, permission or execution authority.
use crate::{
    request_submission::{ConcreteCommand, ConcreteInput},
    workload_leases::definition::{
        EnergyParameters, EventMode, Events, Limits, Overflow, Source, Template,
    },
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[path = "microphone_outcomes.rs"]
pub mod outcomes;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneInput {
    pub schema_version: u32,
    pub original_text: String,
    pub title: String,
    #[serde(default)]
    pub source_node: Option<String>,
    #[serde(default)]
    pub microphone: Option<String>,
    pub priority: i32,
    pub parameters: EnergyParameters,
    pub limits: Limits,
    pub events: Events,
}
fn resource(node: &str, id: &str) -> Result<String> {
    let local = if let Some((owner, local)) = id.split_once("::") {
        ensure!(owner == node, "Microphone resource belongs to another node");
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
        "Microphone selector must be a local or qualified resource ID"
    );
    Ok(crate::context::resource_id(node, local))
}
impl MicrophoneInput {
    pub fn source<'a>(&'a self, origin: &'a str) -> &'a str {
        self.source_node.as_deref().unwrap_or(origin)
    }
    fn template(&self, node: &str, id: String, binding: String, adapter: String) -> Template {
        Template {
            schema_version: 2,
            executor_node: node.into(),
            priority: self.priority,
            source: Source::MicrophoneEnergy {
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
        if let Some(id) = &self.microphone {
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
            "Invalid typed microphone request"
        );
        self.template(
            node,
            crate::context::resource_id(node, "microphone"),
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
        let Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &template.source
        else {
            bail!("Microphone intent cannot bind a camera workload")
        };
        ensure!(
            template.executor_node == node
                && self
                    .microphone
                    .as_deref()
                    .is_none_or(|id| resource(node, id).is_ok_and(|id| id == *resource_id))
                && self.template(
                    node,
                    resource_id.clone(),
                    resource_sha256.clone(),
                    adapter_sha256.clone()
                ) == *template,
            "Retained microphone choices differ from immutable typed input"
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
        snapshot: &crate::workload_sources::Report,
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
            .microphone
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
pub struct MicrophoneCommand {
    pub operation_id: String,
    pub input: MicrophoneInput,
}
impl MicrophoneCommand {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        self.input.validate()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: MicrophoneCommand,
}

#[derive(Clone, Debug, clap::Args)]
pub struct MicrophoneSubmission {
    #[command(flatten)]
    pub source: Box<MicrophoneSourceOptions>,
    #[command(flatten)]
    pub capture: Box<MicrophoneCaptureOptions>,
    #[command(flatten)]
    pub limits: Box<MicrophoneLimitOptions>,
    #[command(flatten)]
    pub events: Box<MicrophoneEventOptions>,
    /// Preserve this receipt after uncertainty; never generate another to retry.
    #[arg(long)]
    pub operation_id: Option<String>,
}
// Separate flattened groups bound each derive parser's stack frame while keeping
// the same flat CLI flags and the existing one-MiB Windows parser requirement.
#[derive(Clone, Debug, clap::Args)]
pub struct MicrophoneSourceOptions {
    /// Defaults to this node; another source must be disclosed to this origin.
    #[arg(long)]
    pub source_node: Option<String>,
    /// Omit only when the source has exactly one available configured microphone.
    #[arg(long)]
    pub microphone: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, default_value = "Microphone activity")]
    pub title: String,
    #[arg(long, default_value_t = 0)]
    pub priority: i32,
}
#[derive(Clone, Debug, clap::Args)]
pub struct MicrophoneCaptureOptions {
    #[arg(long, default_value_t = 16000)]
    pub sample_rate: u32,
    #[arg(long, default_value_t = 1)]
    pub channels: u32,
    #[arg(long, default_value_t = 1600)]
    pub block_frames: u32,
    #[arg(long, default_value_t = 100)]
    pub blocks: u32,
    #[arg(long, default_value_t = 1000)]
    pub activation_level: u32,
    #[arg(long, default_value_t = 500)]
    pub deactivation_level: u32,
}
#[derive(Clone, Debug, clap::Args)]
pub struct MicrophoneLimitOptions {
    #[arg(long, default_value_t = 60000)]
    pub max_session_ms: u64,
    #[arg(long, default_value_t = 10000)]
    pub max_lease_ms: u64,
    #[arg(long, default_value_t = 128)]
    pub max_job_memory_mib: u64,
}
#[derive(Clone, Debug, clap::Args)]
pub struct MicrophoneEventOptions {
    #[arg(long, default_value_t = 100)]
    pub min_interval_ms: u64,
    #[arg(long, default_value_t = 1000)]
    pub max_events: u64,
    #[arg(long, default_value_t = 1024)]
    pub max_event_bytes: usize,
    // Fit a fresh node's receipt limit without silently raising owner policy.
    #[arg(long, default_value_t = 65536)]
    pub max_total_bytes: usize,
    #[arg(long, default_value_t = 4)]
    pub max_pending: usize,
}
impl MicrophoneSubmission {
    pub fn command(&self) -> Result<MicrophoneCommand> {
        let source = &self.source;
        let capture = &self.capture;
        let limits = &self.limits;
        let events = &self.events;
        let command = MicrophoneCommand {operation_id:self.operation_id.clone().unwrap_or_else(||uuid::Uuid::new_v4().to_string()),
            input:MicrophoneInput {schema_version:1,
                original_text:source.text.clone().unwrap_or_else(||format!(
                    "Review {} microphone activity blocks from {} at {} Hz, {} frames per block.",
                    capture.blocks,source.source_node.as_deref().unwrap_or("this node"),capture.sample_rate,capture.block_frames)),
                title:source.title.clone(), source_node:source.source_node.clone(), microphone:source.microphone.clone(), priority:source.priority,
                parameters:EnergyParameters {sample_rate:capture.sample_rate,channels:capture.channels,block_frames:capture.block_frames,
                    max_blocks:capture.blocks,activation_level:capture.activation_level,deactivation_level:capture.deactivation_level},
                limits:Limits {max_session_ms:limits.max_session_ms,max_lease_ms:limits.max_lease_ms,
                    max_job_memory_mib:limits.max_job_memory_mib,
                    max_source_frames:u64::from(capture.blocks)*u64::from(capture.block_frames)},
                events:Events {mode:EventMode::ActivityTransitions,min_interval_ms:events.min_interval_ms,max_events:events.max_events,
                    max_event_bytes:events.max_event_bytes,max_total_bytes:events.max_total_bytes,max_pending:events.max_pending,overflow:Overflow::Stop} }};
        command.validate()?;
        Ok(command)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneReport {
    pub schema_version: u32,
    pub report_type: String,
    pub operation_id: String,
    pub input: MicrophoneInput,
    pub concrete: ConcreteInput,
    pub result: Value,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, command: &MicrophoneCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() || command.validate().is_err() {
        return false;
    }
    let Ok(report) = serde_json::from_value::<MicrophoneReport>(value.clone()) else {
        return false;
    };
    let Ok(template) = Template::from_params(&report.concrete.input.params) else {
        return false;
    };
    report.schema_version == 1
        && report.report_type == "coordinator_microphone_request"
        && report.operation_id == command.operation_id
        && report.input == command.input
        && !report.execution_authorized
        && !report.dispatched
        && !report.automatically_retried
        && command
            .input
            .concrete(&template, Some(node))
            .is_ok_and(|v| v == report.concrete)
        && crate::request_submission::confirmed_with_workload(
            &report.result,
            node,
            &ConcreteCommand {
                operation_id: command.operation_id.clone(),
                request: report.concrete,
            },
            &command.input,
        )
}
const NOTE: &str = "Microphone intent saved/resolved/compiled only. Separate owner approval and explicit execution are required. After uncertainty inspect jobs find with this operation ID, then requests plan. Retained choices are never automatically re-resolved or replayed; no capture, permission, inference or retry fallback.";
const REFUSAL_NOTE: &str = "The coordinator refused this attempt before entering the review checkpoint. Earlier or concurrent requests may still exist. Inspect the operation receipt; no automatic retry, capture, permission, approval or execution was requested.";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    command: MicrophoneCommand,
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
pub fn confirmed_node(value: &Value, node: &str, command: &MicrophoneCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() || command.validate().is_err() {
        return false;
    }
    let Ok(r) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    r.schema_version == 1
        && r.report_type == "node_microphone_request"
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
                        "microphone_authoring_unconfirmed"
                    } else {
                        "microphone_authoring_not_sent"
                    })
        }
}
pub fn unavailable(node: &str, command: MicrophoneCommand, unknown: bool) -> Value {
    json!(NodeReport {
        schema_version: 1,
        report_type: "node_microphone_request".into(),
        node_id: node.into(),
        command,
        success: false,
        coordinator_acknowledged: false,
        outcome_unknown: unknown,
        result: None,
        refusal: None,
        error_code: Some(
            if unknown {
                "microphone_authoring_unconfirmed"
            } else {
                "microphone_authoring_not_sent"
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
    pub async fn microphone_request(&self, request: MicrophoneRequest) -> Result<Value> {
        ensure!(
            request.schema_version == 1 && request.expected_node_id == self.node_id,
            "Microphone authoring schema/target mismatch"
        );
        request.command.validate()?;
        let response = self
            .submissions
            .request_correlated(
                "requests.microphone",
                json!(request.command.input),
                Some(request.command.operation_id.clone()),
            )
            .await;
        let value = match response {
            Ok(value) if confirmed(&value, &self.node_id, &request.command) => json!(NodeReport {
                schema_version: 1,
                report_type: "node_microphone_request".into(),
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
                    report_type: "node_microphone_request".into(),
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
            "Unconfirmed microphone authoring checkpoint"
        );
        Ok(value)
    }
}
pub fn render_node(value: &Value) -> Result<String> {
    let r: NodeReport = serde_json::from_value(value.clone())?;
    ensure!(
        confirmed_node(value, &r.node_id, &r.command),
        "Unconfirmed microphone authoring report"
    );
    if let Some(refusal) = r.refusal {
        Ok(format!(
            "Microphone preparation refused\n{}\nSource node: {}\n{}\nThis attempt did not enter the review checkpoint. An earlier or concurrent request may still exist.\nInspect receipt: eefn jobs find --operation-id {}\nNo automatic retry, approval or execution was requested.\n",
            refusal.reason,
            r.command.input.source(&r.node_id),
            refusal.reason.guidance(),
            r.command.operation_id
        ))
    } else if let Some(result) = r.result {
        let report: MicrophoneReport = serde_json::from_value(result)?;
        let template = Template::from_params(&report.concrete.input.params)?;
        let id = report.result["plan"]["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Microphone job ID unavailable"))?;
        Ok(format!(
            "{} microphone activity request\nSource node: {} | Resource: {}\nJob: {}\nNo capture, permission, approval or execution was requested.\nInspect: eefn requests plan {}\nCoordinator owner approval remains separate: eef requests inspect {}\n",
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
            "Microphone authoring {}\nOperation ID: {}\nInspect before deciding what to do: eefn jobs find --operation-id {}\nKeep this receipt. No automatic retry, capture, permission, approval or execution was requested.\n",
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
#[path = "microphone_submission_tests.rs"]
mod tests;
