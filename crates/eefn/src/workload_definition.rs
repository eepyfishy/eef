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
        match &self.source {
            Source::MicrophoneEnergy { resource_id, .. } => resource_id,
        }
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

/// Only the implemented metadata-producing source has a reviewed shape today.
/// No arbitrary plugin path, code, command, model or raw media destination.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    MicrophoneEnergy {
        resource_id: String,
        resource_sha256: String,
        adapter_sha256: String,
        parameters: EnergyParameters,
    },
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
        Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            parameters,
        } => {
            parameters.validate()?;
            let prefix = format!("{node}::");
            let local = resource_id.strip_prefix(&prefix).unwrap_or("");
            ensure!(
                !local.is_empty()
                    && local.len() <= 100
                    && local
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    && digest(resource_sha256)
                    && digest(adapter_sha256)
                    && u64::from(parameters.block_frames) * u64::from(parameters.max_blocks)
                        <= limits.max_source_frames,
                "Workload source resource, adapter or frame budget changed"
            );
        }
    }
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
        crate::context::NodeMetadata {
            resources: vec![resource.clone()],
            ..Default::default()
        }
        .validate()?;
        let Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            parameters,
        } = &self.source;
        ensure!(
            resource.available
                && resource.capability == "audio.capture"
                && *resource_id == crate::context::resource_id(&self.executor_node, &resource.id)
                && *resource_sha256 == crate::grants::definition_digest(&json!(resource))?
                && *adapter_sha256 == hex::encode(Sha256::digest(adapter_source))
                && resource.parameters.keys().all(|key| key == "device"),
            "Local workload resource binding or source bytes changed"
        );
        let device = resource
            .parameters
            .get("device")
            .cloned()
            .unwrap_or_default();
        ensure!(
            device.is_null()
                || device.as_u64().is_some_and(|value| value <= 1023)
                || device.as_str().is_some_and(|value| !value.trim().is_empty()
                    && value.len() <= 256
                    && !value.chars().any(char::is_control)),
            "Invalid private microphone device binding"
        );
        let mut request = parameters.python_parameters()?;
        request["device"] = device;
        Ok(request)
    }
}

#[cfg(test)]
#[path = "workload_definition_tests.rs"]
mod tests;
