//! Exact reviewed session definition. Validation and hashing do not issue,
//! consume or authorize work. Product admission must persist this definition
//! through the existing job/attempt and grant receipt machinery before effects.
use super::{Contract, MAX_LEASE_MS, MAX_SESSION_MS};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub const CAPABILITY: &str = "workload.session";
pub const TEMPLATE: &str = "continuous_workload";

/// Exact reviewable source and bounds, before any grant identity is assigned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub schema_version: u32,
    pub executor_node: String,
    pub priority: i32,
    pub source: Source,
    pub limits: Limits,
    pub events: Events,
}
impl Template {
    pub fn from_params(params: &serde_json::Value) -> Result<Self> {
        ensure!(
            params
                .as_object()
                .is_some_and(|p| p.len() == 1 && p.contains_key("workload")),
            "Workload input requires one exact definition"
        );
        let template: Self = serde_json::from_value(params["workload"].clone())?;
        template.validate()?;
        Ok(template)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2 && serde_json::to_vec(self)?.len() <= 64 * 1024,
            "Invalid workload template schema or size"
        );
        validate_components(
            &self.executor_node,
            self.priority,
            &self.source,
            &self.limits,
            &self.events,
        )
    }
    pub fn resource_id(&self) -> &str {
        self.source.resource_id()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub grant_id: String,
    pub issuer_id: String,
    pub session_id: String,
    pub executor_node: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub plan_sha256: String,
    #[serde(deserialize_with = "strict_context")]
    pub request_context: crate::context::RequestContext,
    pub priority: i32,
    pub source: Source,
    pub limits: Limits,
    pub events: Events,
}

/// Reviewed metadata-producing sources, not arbitrary extension entry points.
/// No arbitrary plugin path, code, command, model or raw media destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    MicrophoneEnergy,
    CameraMotion,
}
impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MicrophoneEnergy => "microphone_energy",
            Self::CameraMotion => "camera_motion",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    MicrophoneEnergy {
        resource_id: String,
        resource_sha256: String,
        adapter_sha256: String,
        parameters: EnergyParameters,
    },
    CameraMotion {
        resource_id: String,
        resource_sha256: String,
        adapter_sha256: String,
        parameters: MotionParameters,
    },
}

impl Source {
    pub fn kind(&self) -> &'static str {
        self.source_kind().as_str()
    }
    pub fn source_kind(&self) -> SourceKind {
        match self {
            Self::MicrophoneEnergy { .. } => SourceKind::MicrophoneEnergy,
            Self::CameraMotion { .. } => SourceKind::CameraMotion,
        }
    }
    pub fn resource_id(&self) -> &str {
        match self {
            Self::MicrophoneEnergy { resource_id, .. } | Self::CameraMotion { resource_id, .. } => {
                resource_id
            }
        }
    }
    pub fn resource_sha256(&self) -> &str {
        match self {
            Self::MicrophoneEnergy {
                resource_sha256, ..
            }
            | Self::CameraMotion {
                resource_sha256, ..
            } => resource_sha256,
        }
    }
    pub fn adapter_sha256(&self) -> &str {
        match self {
            Self::MicrophoneEnergy { adapter_sha256, .. }
            | Self::CameraMotion { adapter_sha256, .. } => adapter_sha256,
        }
    }
    pub fn capability(&self) -> &'static str {
        match self {
            Self::MicrophoneEnergy { .. } => "audio.capture",
            Self::CameraMotion { .. } => "camera.capture",
        }
    }
    pub fn frames_per_observation(&self) -> u64 {
        match self {
            Self::MicrophoneEnergy { parameters, .. } => u64::from(parameters.block_frames),
            Self::CameraMotion { .. } => 1,
        }
    }
    pub fn max_observations(&self) -> u64 {
        match self {
            Self::MicrophoneEnergy { parameters, .. } => u64::from(parameters.max_blocks),
            Self::CameraMotion { parameters, .. } => u64::from(parameters.max_frames),
        }
    }
    pub fn max_level(&self) -> u16 {
        match self {
            Self::MicrophoneEnergy { .. } => 32768,
            Self::CameraMotion { .. } => 255,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionParameters {
    pub width: u32,
    pub height: u32,
    pub sample_width: u32,
    pub sample_height: u32,
    pub max_frames: u32,
    pub interval_ms: u64,
    pub activation_level: u32,
    pub deactivation_level: u32,
    pub io_timeout_ms: u64,
    pub max_duration_ms: u64,
}
impl MotionParameters {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (160..=1920).contains(&self.width)
                && (120..=1080).contains(&self.height)
                && (8..=160).contains(&self.sample_width)
                && (8..=120).contains(&self.sample_height)
                && (1..=100000).contains(&self.max_frames)
                && (100..=10000).contains(&self.interval_ms)
                && (1..=255).contains(&self.activation_level)
                && self.deactivation_level <= self.activation_level
                && (100..=10000).contains(&self.io_timeout_ms)
                && (100..=3600000).contains(&self.max_duration_ms)
                && self.io_timeout_ms <= self.max_duration_ms,
            "Invalid camera motion parameters"
        );
        Ok(())
    }
    fn python_parameters(&self) -> Result<serde_json::Value> {
        self.validate()?;
        Ok(serde_json::to_value(self)?)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnergyParameters {
    pub sample_rate: u32,
    pub channels: u32,
    pub block_frames: u32,
    pub max_blocks: u32,
    pub activation_level: u32,
    pub deactivation_level: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_session_ms: u64,
    pub max_lease_ms: u64,
    pub max_job_memory_mib: u64,
    pub max_source_frames: u64,
}

/// Metadata-only reporting with explicit aggregate and pending bounds. This
/// describes required behavior, not an event queue or retention implementation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    pub mode: EventMode,
    pub min_interval_ms: u64,
    pub max_events: u64,
    pub max_event_bytes: usize,
    pub max_total_bytes: usize,
    pub max_pending: usize,
    pub overflow: Overflow,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventMode {
    ActivityTransitions,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    Stop,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn strict_context<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<crate::context::RequestContext, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        request_id: String,
        origin_node: String,
        origin_area: Vec<String>,
    }
    let raw = Raw::deserialize(deserializer)?;
    crate::context::RequestContext::new(raw.request_id, raw.origin_node, raw.origin_area)
        .map_err(serde::de::Error::custom)
}
impl EnergyParameters {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (8000..=48000).contains(&self.sample_rate)
                && (1..=2).contains(&self.channels)
                && (80..=4096).contains(&self.block_frames)
                && (1..=100_000).contains(&self.max_blocks)
                && (1..=32768).contains(&self.activation_level)
                && self.deactivation_level <= self.activation_level,
            "Invalid microphone energy parameters"
        );
        Ok(())
    }
    fn python_parameters(&self) -> Result<serde_json::Value> {
        self.validate()?;
        Ok(
            json!({"sample_rate":self.sample_rate,"channels":self.channels,
            "block_frames":self.block_frames,"max_blocks":self.max_blocks,
            "activation_level":self.activation_level,
            "deactivation_level":self.deactivation_level}),
        )
    }
}
/// Private adapter bindings are checked only on their owning node. Other peers
/// receive this digest, never the device setting or other private properties.
pub fn microphone_resource_digest(resource: &crate::context::Resource) -> Result<String> {
    crate::context::NodeMetadata {
        resources: vec![resource.clone()],
        ..Default::default()
    }
    .validate()?;
    ensure!(
        resource.capability == "audio.capture" && resource.parameters.keys().all(|k| k == "device"),
        "Unsupported microphone resource binding"
    );
    let device = resource
        .parameters
        .get("device")
        .cloned()
        .unwrap_or_default();
    ensure!(
        device.is_null()
            || device.as_u64().is_some_and(|n| n <= 1023)
            || device.as_str().is_some_and(|s| !s.trim().is_empty()
                && s.len() <= 256
                && !s.chars().any(char::is_control)),
        "Invalid private microphone device binding"
    );
    crate::grants::definition_digest(&json!(resource))
}
pub fn camera_resource_digest(resource: &crate::context::Resource) -> Result<String> {
    crate::context::NodeMetadata {
        resources: vec![resource.clone()],
        ..Default::default()
    }
    .validate()?;
    ensure!(
        resource.capability == "camera.capture"
            && resource.parameters.keys().all(|key| key == "device"),
        "Unsupported camera resource binding"
    );
    let device = resource
        .parameters
        .get("device")
        .cloned()
        .unwrap_or_default();
    let permitted = device.is_null()
        || device.as_u64().is_some_and(|value| value <= 1023)
        || device.as_str().is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 1024
                && !value.chars().any(|c| c.is_control() || c.is_whitespace())
                && reqwest::Url::parse(value).is_ok_and(|url| {
                    matches!(url.scheme(), "rtsp" | "rtsps")
                        && url.host_str().is_some_and(|host| !host.is_empty())
                        && url.fragment().is_none_or(str::is_empty)
                        && url.port().is_none_or(|port| port > 0)
                })
        });
    ensure!(permitted, "Invalid private camera device binding");
    crate::grants::definition_digest(&json!(resource))
}
impl Definition {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2
                && digest(&self.plan_sha256)
                && (-1000..=1000).contains(&self.priority)
                && serde_json::to_vec(self)?.len() <= 64 * 1024,
            "Invalid workload definition version, plan or size"
        );
        // Reuse the existing exact grant/attempt identity rules. The temporary
        // digest here validates identity only; scope() replaces it with the hash
        // of this entire validated definition, avoiding a circular digest field.
        self.identity_scope("0".repeat(64)).validate()?;
        crate::network::validate_remote_node_id(self.request_context.origin_node())?;
        crate::automation_occurrence::uuid(self.request_context.request_id())?;
        crate::context::validate_area(self.request_context.origin_area())?;
        validate_components(
            &self.executor_node,
            self.priority,
            &self.source,
            &self.limits,
            &self.events,
        )
    }
    fn identity_scope(&self, grant_sha256: String) -> crate::grants::stop::Scope {
        crate::grants::stop::Scope {
            grant_id: self.grant_id.clone(),
            grant_sha256,
            issuer_id: self.issuer_id.clone(),
            session_id: self.session_id.clone(),
            executor_node: self.executor_node.clone(),
            job_id: self.job_id.clone(),
            task_id: self.task_id.clone(),
            attempt: self.attempt,
        }
    }
    pub fn template(&self) -> Template {
        Template {
            schema_version: self.schema_version,
            executor_node: self.executor_node.clone(),
            priority: self.priority,
            source: self.source.clone(),
            limits: self.limits.clone(),
            events: self.events.clone(),
        }
    }
    pub fn scope(&self) -> Result<crate::grants::stop::Scope> {
        self.validate()?;
        Ok(self.identity_scope(crate::grants::definition_digest(&json!(self))?))
    }
    pub fn lease_contract(&self) -> Result<Contract> {
        Ok(Contract {
            schema_version: 2,
            scope: self.scope()?,
            max_session_ms: self.limits.max_session_ms,
            max_lease_ms: self.limits.max_lease_ms,
        })
    }
}
fn validate_components(
    node: &str,
    priority: i32,
    source: &Source,
    limits: &Limits,
    events: &Events,
) -> Result<()> {
    crate::network::validate_remote_node_id(node)?;
    ensure!(
        (-1000..=1000).contains(&priority),
        "Invalid workload priority"
    );
    crate::model_memory::validate(Some(limits.max_job_memory_mib))?;
    ensure!(
        (1..=MAX_SESSION_MS).contains(&limits.max_session_ms)
            && (1..=MAX_LEASE_MS).contains(&limits.max_lease_ms)
            && limits.max_lease_ms <= limits.max_session_ms
            && (1..=409_600_000).contains(&limits.max_source_frames),
        "Invalid workload session or source limits"
    );
    ensure!(
        (10..=60_000).contains(&events.min_interval_ms)
            && (1..=100_000).contains(&events.max_events)
            && (128..=4096).contains(&events.max_event_bytes)
            && (1024..=4 * 1024 * 1024).contains(&events.max_total_bytes)
            && events.max_event_bytes <= events.max_total_bytes
            && (1..=64).contains(&events.max_pending),
        "Invalid workload event bounds"
    );
    match source {
        Source::MicrophoneEnergy { parameters, .. } => parameters.validate()?,
        Source::CameraMotion { parameters, .. } => {
            parameters.validate()?;
            ensure!(
                parameters.max_duration_ms <= limits.max_session_ms,
                "Camera source duration exceeds reviewed session"
            );
        }
    }
    let prefix = format!("{node}::");
    let local = source.resource_id().strip_prefix(&prefix).unwrap_or("");
    ensure!(
        !local.is_empty()
            && local.len() <= 100
            && local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && digest(source.resource_sha256())
            && digest(source.adapter_sha256())
            && source.frames_per_observation() * source.max_observations()
                <= limits.max_source_frames,
        "Workload source resource, adapter or frame budget changed"
    );
    Ok(())
}
impl Definition {
    /// Resolve private device data only on its owning node, against the reviewed
    /// resource snapshot and source bytes. Never advertise/forward this request.
    /// This neither opens a source nor proves admission or permission/ownership.
    pub fn bound_python_parameters(
        &self,
        resource: &crate::context::Resource,
        adapter_source: &[u8],
    ) -> Result<serde_json::Value> {
        self.validate()?;
        let (digest, mut request) = match &self.source {
            Source::MicrophoneEnergy { parameters, .. } => (
                microphone_resource_digest(resource)?,
                parameters.python_parameters()?,
            ),
            Source::CameraMotion { parameters, .. } => (
                camera_resource_digest(resource)?,
                parameters.python_parameters()?,
            ),
        };
        ensure!(
            resource.available
                && self.source.resource_id()
                    == crate::context::resource_id(&self.executor_node, &resource.id)
                && self.source.resource_sha256() == digest
                && self.source.adapter_sha256() == hex::encode(Sha256::digest(adapter_source)),
            "Local workload resource binding or source bytes changed"
        );
        let mut device = resource
            .parameters
            .get("device")
            .cloned()
            .unwrap_or_default();
        if matches!(&self.source, Source::CameraMotion { .. }) && device.is_null() {
            device = json!(0);
        }
        request["device"] = device;
        Ok(request)
    }
}

#[cfg(test)]
#[path = "workload_definition_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workload_camera_definition_tests.rs"]
pub(crate) mod camera_tests;
