//! Typed finite-audio authoring, not permission, capture, playback or execution.
use crate::{
    peer::{
        audio::Audio,
        plan::{Endpoint, Pair},
        scope::Limits,
    },
    request_submission::{ConcreteCommand, ConcreteInput},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn default_limits() -> Limits {
    let policy = crate::peer::admission::Policy::default();
    Limits {
        max_wire_bytes: policy.max_wire_bytes,
        max_bytes_per_second: policy.max_bytes_per_second,
        max_frame_bytes: 4096,
        max_frames: 1024,
        max_inflight_frames: 1,
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInput {
    pub schema_version: u32,
    pub original_text: String,
    pub title: String,
    #[serde(default)]
    pub source_node: Option<String>,
    pub destination_node: String,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u8,
    #[serde(default)]
    pub source_transport: Option<String>,
    #[serde(default)]
    pub destination_transport: Option<String>,
    #[serde(default)]
    pub microphone: Option<String>,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default = "default_limits")]
    pub limits: Limits,
}
fn local_id<'a>(node: &str, id: &'a str) -> Result<&'a str> {
    if let Some((owner, local)) = id.split_once("::") {
        if owner != node {
            bail!("Audio resource belongs to another node")
        }
        Ok(local)
    } else {
        Ok(id)
    }
}
fn selector(id: &str) -> Result<()> {
    let local = if let Some((node, local)) = id.split_once("::") {
        crate::network::validate_remote_node_id(node)?;
        local
    } else {
        id
    };
    if local.is_empty()
        || local.len() > 100
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        bail!("Audio resource selectors must be local or qualified resource IDs")
    }
    Ok(())
}
impl AudioInput {
    fn audio(&self, microphone: String, speaker: String) -> Audio {
        Audio {
            duration_ms: self.duration_ms,
            sample_rate: self.sample_rate,
            channels: self.channels,
            source_resource_id: microphone,
            destination_resource_id: speaker,
        }
    }
    pub fn validate(&self) -> Result<()> {
        crate::interpretation::messages(&self.original_text, &[])?;
        crate::network::validate_remote_node_id(&self.destination_node)?;
        if let Some(source) = &self.source_node {
            crate::network::validate_remote_node_id(source)?;
        }
        self.audio(String::new(), String::new()).validate()?;
        self.limits.validate()?;
        if self.schema_version != 1
            || self.title.trim().is_empty()
            || self.title.len() > 512
            || self.title.chars().any(char::is_control)
            || self.source_node.as_ref() == Some(&self.destination_node)
            || serde_json::to_vec(self)?.len() > 32 * 1024
        {
            bail!("Invalid explicit finite audio source")
        }
        for id in [
            &self.source_transport,
            &self.destination_transport,
            &self.microphone,
            &self.speaker,
        ]
        .into_iter()
        .flatten()
        {
            selector(id)?;
        }
        for id in [&self.destination_transport, &self.speaker]
            .into_iter()
            .flatten()
        {
            local_id(&self.destination_node, id)?;
        }
        if let Some(node) = &self.source_node {
            for id in [&self.source_transport, &self.microphone]
                .into_iter()
                .flatten()
            {
                local_id(node, id)?;
            }
        }
        let maximum = self.audio(String::new(), String::new()).maximum_bytes();
        if maximum > self.limits.max_wire_bytes
            || self.limits.max_frame_bytes < 512
            || maximum.div_ceil(u64::from(self.limits.max_frame_bytes)) + 1
                > u64::from(self.limits.max_frames)
        {
            bail!("Audio format/duration exceeds requested transport bounds")
        }
        Ok(())
    }
    pub fn source<'a>(&'a self, origin: &'a str) -> &'a str {
        self.source_node.as_deref().unwrap_or(origin)
    }
    /// None validates structure only; retained jobs additionally bind the origin.
    pub fn concrete(&self, pair: &Pair, origin: Option<&str>) -> Result<ConcreteInput> {
        self.validate()?;
        pair.validate()?;
        let source = self
            .source_node
            .as_deref()
            .or(origin)
            .unwrap_or(&pair.source.node_id);
        if pair.schema_version != 2
            || pair.source.node_id != source
            || pair.destination.node_id != self.destination_node
            || pair.limits != self.limits
            || pair.audio.as_ref().is_none_or(|a| {
                a.duration_ms != self.duration_ms
                    || a.sample_rate != self.sample_rate
                    || a.channels != self.channels
            })
        {
            bail!("Retained audio choices differ from the immutable typed source")
        }
        let audio = pair.audio.as_ref().unwrap();
        for (node, requested, actual) in [
            (source, &self.source_transport, &pair.source.resource_id),
            (
                &self.destination_node,
                &self.destination_transport,
                &pair.destination.resource_id,
            ),
            (source, &self.microphone, &audio.source_resource_id),
            (
                &self.destination_node,
                &self.speaker,
                &audio.destination_resource_id,
            ),
        ] {
            if let Some(requested) = requested {
                if *actual != crate::context::resource_id(node, local_id(node, requested)?) {
                    bail!("Audio binding differs from requested resource")
                }
            }
        }
        let input = ConcreteInput {
            schema_version: 1,
            original_text: self.original_text.clone(),
            title: self.title.clone(),
            input: crate::request_planning::PlanInput {
                schema_version: 1,
                template: "peer_transfer".into(),
                params: json!({"transfer":pair}),
                constraints: json!({}),
            },
        };
        input.validate()?;
        Ok(input)
    }
    pub fn resolve(
        &self,
        origin: &str,
        source: &EndpointResources,
        destination: &EndpointResources,
    ) -> Result<ConcreteInput> {
        self.validate()?;
        source.validate()?;
        destination.validate()?;
        if source.node_id != self.source(origin)
            || destination.node_id != self.destination_node
            || !source.finite_audio
            || !destination.finite_audio
            || !destination.can_receive
        {
            bail!("Requested nodes lack the current finite-audio endpoint roles")
        }
        let pair = Pair {
            schema_version: 2,
            source: Endpoint {
                node_id: source.node_id.clone(),
                resource_id: source.choose("peer.transfer", self.source_transport.as_deref())?,
            },
            destination: Endpoint {
                node_id: destination.node_id.clone(),
                resource_id: destination
                    .choose("peer.transfer", self.destination_transport.as_deref())?,
            },
            payload_bytes: self.audio(String::new(), String::new()).maximum_bytes(),
            payload_sha256: None,
            audio: Some(self.audio(
                source.choose("audio.capture", self.microphone.as_deref())?,
                destination.choose("audio.play", self.speaker.as_deref())?,
            )),
            limits: self.limits.clone(),
        };
        self.concrete(&pair, Some(origin))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceChoice {
    pub id: String,
    pub capability: String,
    pub available: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointResources {
    pub node_id: String,
    pub finite_audio: bool,
    pub can_receive: bool,
    pub resources: Vec<ResourceChoice>,
}
impl EndpointResources {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        if self.resources.len() > 128 {
            bail!("Audio resource context exceeds bounds")
        }
        let mut seen = std::collections::HashSet::new();
        for r in &self.resources {
            selector(&r.id)?;
            if !r.id.starts_with(&format!("{}::", self.node_id))
                || !["peer.transfer", "audio.capture", "audio.play"]
                    .contains(&r.capability.as_str())
                || !seen.insert(&r.id)
            {
                bail!("Invalid audio resource context")
            }
        }
        Ok(())
    }
    fn choose(&self, capability: &str, requested: Option<&str>) -> Result<String> {
        let wanted = requested
            .map(|id| local_id(&self.node_id, id))
            .transpose()?
            .map(|local| crate::context::resource_id(&self.node_id, local));
        let candidates = self
            .resources
            .iter()
            .filter(|r| {
                r.capability == capability
                    && r.available
                    && wanted.as_ref().is_none_or(|id| id == &r.id)
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            bail!(
                "Audio resource unavailable or ambiguous; inspect node relay setup and select an exact resource"
            )
        }
        Ok(candidates[0].id.clone())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioCommand {
    pub operation_id: String,
    pub input: AudioInput,
}
impl AudioCommand {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        self.input.validate()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: AudioCommand,
}

#[derive(Clone, Debug, clap::Args)]
pub struct AudioSubmission {
    /// Destination node ID listed by network peers.
    #[arg(long = "to")]
    pub destination_node: String,
    /// Defaults to this node; other sources must be owner-disclosed.
    #[arg(long)]
    pub source_node: Option<String>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, default_value = "Finite audio relay")]
    pub title: String,
    #[arg(long,default_value_t=2000,value_parser=clap::value_parser!(u32).range(100..=10000))]
    pub duration_ms: u32,
    #[arg(long, default_value_t = 16000)]
    pub sample_rate: u32,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u8).range(1..=2))]
    pub channels: u8,
    #[arg(long)]
    pub source_transport: Option<String>,
    #[arg(long)]
    pub destination_transport: Option<String>,
    #[arg(long)]
    pub microphone: Option<String>,
    #[arg(long)]
    pub speaker: Option<String>,
    #[arg(long)]
    pub max_wire_bytes: Option<u64>,
    #[arg(long)]
    pub max_bytes_per_second: Option<u32>,
    /// Retain this exact receipt after uncertainty; never replace it to retry.
    #[arg(long)]
    pub operation_id: Option<String>,
}
impl AudioSubmission {
    pub fn command(&self) -> Result<AudioCommand> {
        let mut limits = default_limits();
        if let Some(n) = self.max_wire_bytes {
            limits.max_wire_bytes = n;
        }
        if let Some(n) = self.max_bytes_per_second {
            limits.max_bytes_per_second = n;
        }
        let command = AudioCommand {
            operation_id: self
                .operation_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            input: AudioInput {
                schema_version: 1,
                original_text: self.text.clone().unwrap_or_else(|| {
                    format!(
                        "Relay {} ms microphone audio from {} to node {}.",
                        self.duration_ms,
                        self.source_node.as_deref().unwrap_or("this node"),
                        self.destination_node
                    )
                }),
                title: self.title.clone(),
                source_node: self.source_node.clone(),
                destination_node: self.destination_node.clone(),
                duration_ms: self.duration_ms,
                sample_rate: self.sample_rate,
                channels: self.channels,
                source_transport: self.source_transport.clone(),
                destination_transport: self.destination_transport.clone(),
                microphone: self.microphone.clone(),
                speaker: self.speaker.clone(),
                limits,
            },
        };
        command.validate()?;
        Ok(command)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioReport {
    pub schema_version: u32,
    pub report_type: String,
    pub operation_id: String,
    pub input: AudioInput,
    pub concrete: ConcreteInput,
    pub result: Value,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, command: &AudioCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() {
        return false;
    }
    if command.validate().is_err() {
        return false;
    }
    let Ok(report) = serde_json::from_value::<AudioReport>(value.clone()) else {
        return false;
    };
    let Ok(pair) = Pair::from_params(&report.concrete.input.params) else {
        return false;
    };
    if report.schema_version != 1
        || report.report_type != "coordinator_audio_request"
        || report.operation_id != command.operation_id
        || report.input != command.input
        || report.execution_authorized
        || report.dispatched
        || report.automatically_retried
        || !command
            .input
            .concrete(&pair, Some(node))
            .is_ok_and(|expected| expected == report.concrete)
    {
        return false;
    }
    crate::request_submission::confirmed_with_source(
        &report.result,
        node,
        &ConcreteCommand {
            operation_id: command.operation_id.clone(),
            request: report.concrete,
        },
        Some(&command.input),
    )
}
const NOTE: &str = "Audio intent saved/resolved/compiled only. Separate owner review, approval and execution are required. After uncertainty inspect jobs find with this operation ID, then requests plan. Retained choices are never automatically re-resolved or replayed; no inference or permission fallback.";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    command: AudioCommand,
    success: bool,
    coordinator_acknowledged: bool,
    outcome_unknown: bool,
    result: Option<Value>,
    error_code: Option<String>,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed_node(value: &Value, node: &str, command: &AudioCommand) -> bool {
    if crate::network::validate_remote_node_id(node).is_err() {
        return false;
    }
    if command.validate().is_err() {
        return false;
    }
    let Ok(report) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    report.schema_version == 1
        && report.report_type == "node_audio_request"
        && report.node_id == node
        && report.command == *command
        && !report.execution_authorized
        && !report.dispatched
        && !report.automatically_retried
        && report.note == NOTE
        && if report.success {
            report.coordinator_acknowledged
                && !report.outcome_unknown
                && report.error_code.is_none()
                && report
                    .result
                    .as_ref()
                    .is_some_and(|value| confirmed(value, node, command))
        } else {
            !report.coordinator_acknowledged
                && report.result.is_none()
                && report.error_code.as_deref()
                    == Some(if report.outcome_unknown {
                        "audio_authoring_unconfirmed"
                    } else {
                        "audio_authoring_not_sent"
                    })
        }
}
pub fn unavailable(node: &str, command: AudioCommand, unknown: bool) -> Value {
    json!(NodeReport {
        schema_version: 1,
        report_type: "node_audio_request".into(),
        node_id: node.into(),
        command,
        success: false,
        coordinator_acknowledged: false,
        outcome_unknown: unknown,
        result: None,
        error_code: Some(
            if unknown {
                "audio_authoring_unconfirmed"
            } else {
                "audio_authoring_not_sent"
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
    pub async fn audio_request(&self, request: AudioRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Audio authoring schema/target mismatch")
        }
        request.command.validate()?;
        let response = self
            .submissions
            .request_correlated(
                "requests.audio",
                json!(request.command.input),
                Some(request.command.operation_id.clone()),
            )
            .await;
        let value = match response {
            Ok(value) if confirmed(&value, &self.node_id, &request.command) => json!(NodeReport {
                schema_version: 1,
                report_type: "node_audio_request".into(),
                node_id: self.node_id.clone(),
                command: request.command.clone(),
                success: true,
                coordinator_acknowledged: true,
                outcome_unknown: false,
                result: Some(value),
                error_code: None,
                execution_authorized: false,
                dispatched: false,
                automatically_retried: false,
                note: NOTE.into()
            }),
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
        if !confirmed_node(&value, &self.node_id, &request.command) {
            bail!("Audio authoring checkpoint failed confirmation")
        }
        Ok(value)
    }
}

pub fn render_node(value: &Value) -> Result<String> {
    let report: NodeReport = serde_json::from_value(value.clone())?;
    if !confirmed_node(value, &report.node_id, &report.command) {
        bail!("Unconfirmed audio authoring report")
    }
    if let Some(result) = report.result {
        let audio: AudioReport = serde_json::from_value(result)?;
        let pair = Pair::from_params(&audio.concrete.input.params)?;
        let plan = &audio.result["plan"];
        let id = plan["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Audio job ID unavailable"))?;
        let status = plan["status"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Audio job status unavailable"))?;
        let reused = audio.result["creation_reused"] == true;
        Ok(format!(
            "{} finite audio relay: {} ms, {} Hz, {} channel(s)\nFrom node {} to node {}\nJob: {} | Status: {}\n{}\nThis command did not record or play audio, grant permissions, approve work or execute it.\nInspect here: eefn requests plan {}\nCoordinator owner review/approval is separate: eef requests inspect {}\n",
            if reused { "Retained" } else { "Prepared" },
            audio.input.duration_ms,
            audio.input.sample_rate,
            audio.input.channels,
            pair.source.node_id,
            pair.destination.node_id,
            id,
            status,
            if reused {
                "Original saved resource choices were preserved."
            } else {
                "Exact resource choices are saved for review."
            },
            id,
            id
        ))
    } else {
        Ok(format!(
            "Audio authoring {}\nOperation ID: {}\nNo confirmed saved/compiled result; no work was approved or requested to execute.\nInspect before deciding what to do: eefn jobs find --operation-id {}\nKeep this receipt; do not generate another merely to retry. No automatic retry or inference fallback.\n",
            if report.outcome_unknown {
                "was not confirmed"
            } else {
                "was not sent"
            },
            report.command.operation_id,
            report.command.operation_id
        ))
    }
}

#[cfg(test)]
#[path = "audio_submission_tests.rs"]
mod tests;
