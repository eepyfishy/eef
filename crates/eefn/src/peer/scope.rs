//! Public description of a proposed bounded path. Shape validation grants nothing.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Source,
    Destination,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Party {
    pub node_id: String,
    pub control_session_id: String,
    pub runtime_generation: String,
    pub grant_id: String,
    pub resource_id: String,
}
impl Party {
    fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        for id in [
            &self.control_session_id,
            &self.runtime_generation,
            &self.grant_id,
        ] {
            uuid(id)?;
        }
        let (owner, local) = self
            .resource_id
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("Peer resource needs an explicit owner"))?;
        if owner != self.node_id
            || local.is_empty()
            || local.len() > 100
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            bail!("Peer resource owner or local identity differs")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// TLS application bytes, including framing/control overhead in both directions.
    pub max_wire_bytes: u64,
    pub max_bytes_per_second: u32,
    pub max_frame_bytes: u32,
    pub max_frames: u32,
    pub max_inflight_frames: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub schema_version: u32,
    pub session_id: String,
    pub issuer_id: String,
    pub coordinator_runtime_id: String,
    pub job_id: String,
    pub plan_sha256: String,
    pub request_context: crate::context::RequestContext,
    pub source: Party,
    pub destination: Party,
    pub destination_address: SocketAddr,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub limits: Limits,
}
pub(super) fn uuid(id: &str) -> Result<()> {
    crate::job_commands::validate_operation_id(id)?;
    if uuid::Uuid::parse_str(id)?.is_nil() {
        bail!("Nil peer identity")
    }
    Ok(())
}
impl Scope {
    pub fn party(&self, role: Role) -> &Party {
        match role {
            Role::Source => &self.source,
            Role::Destination => &self.destination,
        }
    }
    pub fn validate(&self) -> Result<()> {
        uuid(&self.session_id)?;
        uuid(&self.coordinator_runtime_id)?;
        crate::request_drafts::validate_id(&self.job_id)?;
        crate::network::validate_remote_node_id(self.request_context.origin_node())?;
        uuid(self.request_context.request_id())?;
        self.source.validate()?;
        self.destination.validate()?;
        let limits = &self.limits;
        if self.schema_version != 1
            || !crate::request_result::valid_digest(&self.issuer_id)
            || !crate::request_result::valid_digest(&self.plan_sha256)
            || self.source.node_id == self.destination.node_id
            || self.source.grant_id == self.destination.grant_id
            || self.issued_at_ms == 0
            || self.expires_at_ms <= self.issued_at_ms
            || self.expires_at_ms - self.issued_at_ms > crate::grants::MAX_DURATION_MS
            || !(1..=64 * 1024 * 1024).contains(&limits.max_wire_bytes)
            || !(1..=64 * 1024 * 1024).contains(&limits.max_bytes_per_second)
            || !(1..=256 * 1024).contains(&limits.max_frame_bytes)
            || !(1..=8).contains(&limits.max_inflight_frames)
            || !(1..=65536).contains(&limits.max_frames)
            || u64::from(limits.max_frame_bytes) * u64::from(limits.max_inflight_frames)
                > 2 * 1024 * 1024
            || self.destination_address.port() == 0
            || self.destination_address.ip().is_unspecified()
            || self.destination_address.ip().is_multicast()
            || serde_json::to_vec(self)?.len() > 16384
        {
            bail!("Invalid peer scope or bounds")
        }
        // Also reject timestamps outside X.509's supported calendar, without overflow.
        self.validity()?;
        Ok(())
    }
    pub fn valid_at(&self, now: u64) -> bool {
        self.validate().is_ok() && now >= self.issued_at_ms && now < self.expires_at_ms
    }
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        crate::grants::definition_digest(&serde_json::json!(self))
    }
    pub(super) fn validity(&self) -> Result<(time::OffsetDateTime, time::OffsetDateTime)> {
        Ok((
            time::OffsetDateTime::from_unix_timestamp_nanos(
                i128::from(self.issued_at_ms) * 1_000_000,
            )?,
            time::OffsetDateTime::from_unix_timestamp_nanos(
                i128::from(self.expires_at_ms) * 1_000_000,
            )?,
        ))
    }
}
