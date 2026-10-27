//! Finite transport-only grants using the EXISTING consumed receipt journal.
//! Not a producer/sink permission. Native socket ownership uses the same lease
//! and permission watcher, including readiness and pending connect/accept.
use super::{
    PeerEndpoint, Recipient, SealedMaterial,
    scope::{Limits, Role, Scope},
};
use crate::grants::{AcceptedGrant, ExecutionGrant, GrantLedger, GrantReport};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_PAYLOAD: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub max_sessions: u8,
    pub max_payload_bytes: u64,
    pub max_wire_bytes: u64,
    pub max_bytes_per_second: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_sessions: 1,
            max_payload_bytes: 256 * 1024,
            max_wire_bytes: 2 * 1024 * 1024,
            max_bytes_per_second: 1024 * 1024,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if !(1..=8).contains(&self.max_sessions)
            || !(1..=MAX_PAYLOAD).contains(&self.max_payload_bytes)
            || !(1..=64 * 1024 * 1024).contains(&self.max_wire_bytes)
            || !(1..=64 * 1024 * 1024).contains(&self.max_bytes_per_second)
        {
            bail!("Invalid peer permission limits")
        }
        Ok(())
    }
    pub(crate) fn permits(&self, definition: &Definition) -> Result<()> {
        self.validate()?;
        if !self.enabled
            || definition.payload_bytes > self.max_payload_bytes
            || definition.limits.max_wire_bytes > self.max_wire_bytes
            || definition.limits.max_bytes_per_second > self.max_bytes_per_second
        {
            bail!("Peer transport is disabled or exceeds local limits")
        }
        Ok(())
    }
}

/// Immutable reviewed operation. No credentials, address, runtime, grant ID or
/// plan hash: ephemeral delivery is separate, avoiding circular plan hashing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub resource_id: String,
    pub peer_node: String,
    pub peer_resource_id: String,
    pub payload_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<super::audio::Audio>,
    pub limits: Limits,
}
impl Definition {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.peer_node)?;
        self.limits.validate()?;
        let (owner, local) = self
            .peer_resource_id
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("Peer resource needs an owner"))?;
        let (own_node, own_local) = self
            .resource_id
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("Local peer resource needs owner"))?;
        crate::network::validate_remote_node_id(own_node)?;
        if ![1, 2].contains(&self.schema_version)
            || own_node == self.peer_node
            || own_local.is_empty()
            || own_local.len() > 100
            || !own_local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || owner != self.peer_node
            || local.is_empty()
            || local.len() > 100
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !(1..=MAX_PAYLOAD).contains(&self.payload_bytes)
            || self.payload_bytes > self.limits.max_wire_bytes
            || self
                .payload_bytes
                .div_ceil(u64::from(self.limits.max_frame_bytes))
                > u64::from(self.limits.max_frames)
        {
            bail!("Invalid finite peer definition")
        }
        match (self.schema_version, &self.audio, &self.payload_sha256) {
            (1, None, Some(hash)) if crate::request_result::valid_digest(hash) => {}
            (2, Some(audio), None)
                if audio.validate().is_ok()
                    && self.payload_bytes == audio.maximum_bytes()
                    && self.limits.max_frame_bytes >= 512
                    && self
                        .payload_bytes
                        .div_ceil(u64::from(self.limits.max_frame_bytes))
                        + 1
                        <= u64::from(self.limits.max_frames) => {}
            _ => bail!("Invalid static/audio peer content contract"),
        }
        Ok(())
    }
    pub fn from_grant(g: &ExecutionGrant) -> Result<(Role, Self)> {
        let role = match (g.capability.as_str(), g.action.as_str()) {
            ("peer.transfer", "send") => Role::Source,
            ("peer.transfer", "receive") => Role::Destination,
            _ => bail!("Not a finite peer operation"),
        };
        let definition: Self = serde_json::from_value(g.params.clone())?;
        definition.validate()?;
        if let Some(audio) = &definition.audio {
            let (source, destination) = if role == Role::Source {
                (g.executor_node.as_str(), definition.peer_node.as_str())
            } else {
                (definition.peer_node.as_str(), g.executor_node.as_str())
            };
            audio.validate_nodes(source, destination)?;
            if g.limits.max_execution_ms < audio.minimum_execution_ms() {
                bail!("Finite audio exceeds grant execution allowance")
            }
        }
        let resource = g
            .resource_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Peer grant requires a resource"))?;
        let (owner, local) = resource
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("Peer grant resource needs owner"))?;
        if resource != definition.resource_id
            || owner != g.executor_node
            || local.is_empty()
            || local.len() > 100
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || g.executor_node == definition.peer_node
            || g.model_id.is_some()
            || g.backend.is_some()
            || g.model_role.is_some()
        {
            bail!("Peer grant resource/model/recipient differs")
        }
        Ok((role, definition))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferReceipt {
    pub schema_version: u32,
    pub pair_session_id: String,
    pub scope_sha256: String,
    pub role: Role,
    pub payload_bytes: u64,
    pub payload_sha256: String,
    pub data_frames: u32,
    pub payload_retained: bool,
    pub sink_effects_confirmed: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub producer_reported: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sink_reported: bool,
}
pub(crate) fn confirmed_result(g: &ExecutionGrant, value: &Value) -> bool {
    let Ok((role, definition)) = Definition::from_grant(g) else {
        return false;
    };
    let Ok(r) = serde_json::from_value::<TransferReceipt>(value.clone()) else {
        return false;
    };
    r.schema_version == definition.schema_version
        && super::scope::uuid(&r.pair_session_id).is_ok()
        && crate::request_result::valid_digest(&r.scope_sha256)
        && r.role == role
        && if definition.audio.is_some() {
            r.payload_bytes >= definition.audio.as_ref().unwrap().minimum_bytes()
                && r.payload_bytes <= definition.payload_bytes
                && crate::request_result::valid_digest(&r.payload_sha256)
                && r.producer_reported == (role == Role::Source)
                && r.sink_reported == (role == Role::Destination)
        } else {
            r.payload_bytes == definition.payload_bytes
                && Some(&r.payload_sha256) == definition.payload_sha256.as_ref()
                && !r.producer_reported
                && !r.sink_reported
        }
        && u64::from(r.data_frames)
            == r.payload_bytes
                .div_ceil(u64::from(definition.limits.max_frame_bytes))
                + u64::from(definition.audio.is_some())
        && !r.payload_retained
        && !r.sink_effects_confirmed
}

/// Holds an already consumed ordinary grant, one node-wide transport slot and
/// the private endpoint together. No channel extraction or reconnect method.
pub struct AdmittedPeer {
    grant: ExecutionGrant,
    accepted: AcceptedGrant,
    endpoint: PeerEndpoint,
    role: Role,
    scope: Scope,
    definition: Definition,
    _slot: tokio::sync::OwnedSemaphorePermit,
}
impl AdmittedPeer {
    pub fn prepare(
        g: ExecutionGrant,
        ledger: Arc<GrantLedger>,
        engine: &crate::NodeEngine,
        scope: &Scope,
        material: &SealedMaterial,
        recipient: Recipient<'_>,
        lease: crate::grant_time::GrantLease,
    ) -> Result<Self> {
        g.validate()?;
        let (role, definition) = Definition::from_grant(&g)?;
        scope.validate()?;
        let local = scope.party(role);
        let remote = scope.party(if role == Role::Source {
            Role::Destination
        } else {
            Role::Source
        });
        if material.role != role
            || local.grant_id != g.grant_id
            || local.grant_sha256 != crate::grants::definition_digest(&json!(g))?
            || local.node_id != g.executor_node
            || local.control_session_id != g.session_id
            || Some(local.resource_id.as_str()) != g.resource_id.as_deref()
            || remote.node_id != definition.peer_node
            || remote.resource_id != definition.peer_resource_id
            || scope.issuer_id != g.issuer_id
            || scope.job_id != g.job_id
            || scope.plan_sha256 != g.plan_sha256
            || scope.request_context != g.request_context
            || scope.limits != definition.limits
            || scope.issued_at_ms < g.issued_at_ms
            || scope.expires_at_ms > g.expires_at_ms
            || scope.expires_at_ms - scope.issued_at_ms > g.limits.max_execution_ms
        {
            bail!("Peer delivery differs from the exact execution grant")
        }
        // Verify signing, recipient and local identities before consuming a
        // receipt. The fresh issuer lease is still mandatory; no time fallback.
        let endpoint = super::open(scope, material, recipient)?;
        let slot = engine.peer_slot()?;
        let accepted = AcceptedGrant::prepare_peer_with_lease(
            g.clone(),
            ledger,
            engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
            lease,
        )?;
        Ok(Self {
            grant: g,
            accepted,
            endpoint,
            role,
            scope: scope.clone(),
            definition,
            _slot: slot,
        })
    }
    pub fn acknowledgement(&self) -> GrantReport {
        self.accepted.acknowledgement()
    }

    /// Finite transport receipt only, not capture, storage, playback or a job
    /// result for either physical endpoint. The caller owns supplied IO;
    /// native delivery instead uses run_tcp to include socket waits in the lease.
    pub async fn run<S: crate::tls::NodeIo + 'static>(
        self,
        engine: &crate::NodeEngine,
        stream: S,
        source_payload: Option<Vec<u8>>,
    ) -> GrantReport {
        let execution_deadline = tokio::time::Instant::from_std(self.accepted.execution_deadline());
        let Self {
            grant,
            accepted,
            endpoint,
            role,
            scope,
            definition,
            _slot,
        } = self;
        let operation = async move {
            if definition.audio.is_some() {
                if source_payload.is_some() {
                    bail!("Audio must use its approved capture adapter")
                }
                super::audio_runtime::transfer(
                    engine,
                    &grant,
                    endpoint,
                    role,
                    scope,
                    definition,
                    stream,
                    execution_deadline,
                )
                .await
            } else {
                transfer(
                    endpoint,
                    role,
                    scope,
                    definition,
                    stream,
                    source_payload.map(Arc::<[u8]>::from),
                )
                .await
            }
        };
        let report = accepted.run_operation(engine, operation).await;
        drop(_slot);
        report
    }

    /// Open exactly one scoped TCP route AFTER durable consumption. Destination
    /// readiness means its listener is owned, not that a source or sink ran.
    /// Even binding, readiness output and accept/connect are inside the original
    /// lease/policy watcher. Dropping the future releases every owned socket.
    pub async fn run_tcp<F, Fut>(
        self,
        engine: &crate::NodeEngine,
        source_payload: Option<Arc<[u8]>>,
        ready: F,
    ) -> GrantReport
    where
        F: FnOnce(GrantReport) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let acknowledgement = self.acknowledgement();
        let execution_deadline = tokio::time::Instant::from_std(self.accepted.execution_deadline());
        let Self {
            grant,
            accepted,
            endpoint,
            role,
            scope,
            definition,
            _slot,
        } = self;
        let operation = async move {
            // No caller-provided route or DNS resolution. The numeric endpoint
            // is signed, scoped and checked against the runtime's local binding.
            super::plan::validate_address(scope.destination_address)?;
            validate_payload(role, &definition, source_payload.as_deref())?;
            let stream = if role == Role::Destination {
                let listener = tokio::net::TcpListener::bind(scope.destination_address).await?;
                ready(acknowledgement).await?;
                let (stream, _) = listener.accept().await?;
                // One admitted connection only. An unauthenticated connection
                // cannot cause retries or acquire another grant/session slot.
                drop(listener);
                stream
            } else {
                ready(acknowledgement).await?;
                tokio::net::TcpStream::connect(scope.destination_address).await?
            };
            if definition.audio.is_some() {
                super::audio_runtime::transfer(
                    engine,
                    &grant,
                    endpoint,
                    role,
                    scope,
                    definition,
                    stream,
                    execution_deadline,
                )
                .await
            } else {
                transfer(endpoint, role, scope, definition, stream, source_payload).await
            }
        };
        let report = accepted.run_operation(engine, operation).await;
        drop(_slot);
        report
    }
}

fn validate_payload(role: Role, definition: &Definition, payload: Option<&[u8]>) -> Result<()> {
    if definition.audio.is_some() {
        if payload.is_some() {
            bail!("Audio must use its approved capture adapter")
        }
        return Ok(());
    }
    if role == Role::Source {
        let bytes = payload.ok_or_else(|| anyhow::anyhow!("Missing approved source bytes"))?;
        if bytes.len() as u64 != definition.payload_bytes
            || Some(hex::encode(Sha256::digest(bytes))) != definition.payload_sha256
        {
            bail!("Source bytes differ from approved manifest")
        }
    } else if payload.is_some() {
        bail!("Destination must not supply source bytes")
    }
    Ok(())
}

async fn transfer<S: crate::tls::NodeIo + 'static>(
    endpoint: PeerEndpoint,
    role: Role,
    scope: Scope,
    definition: Definition,
    stream: S,
    source_payload: Option<Arc<[u8]>>,
) -> Result<Value> {
    validate_payload(role, &definition, source_payload.as_deref())?;
    let frames = definition
        .payload_bytes
        .div_ceil(u64::from(definition.limits.max_frame_bytes)) as u32;
    if role == Role::Source {
        let bytes = source_payload.expect("validated source payload");
        let mut channel = endpoint.connect(stream).await?;
        for frame in bytes.chunks(definition.limits.max_frame_bytes as usize) {
            channel.send_data(frame).await?;
            channel.receive_acknowledgement().await?;
        }
    } else {
        let mut channel = endpoint.accept(stream).await?;
        let mut received = 0u64;
        let mut hash = Sha256::new();
        for index in 0..frames {
            let bytes = channel.receive_data().await?;
            let expected = (definition.payload_bytes - received)
                .min(u64::from(definition.limits.max_frame_bytes));
            if bytes.len() as u64 != expected {
                bail!("Peer frame differs from approved finite layout")
            }
            received += bytes.len() as u64;
            hash.update(&bytes);
            if index + 1 == frames
                && Some(hex::encode(hash.clone().finalize())) != definition.payload_sha256
            {
                bail!("Received bytes differ from approved manifest")
            }
            channel.acknowledge().await?;
        }
    }
    Ok(json!(TransferReceipt {
        schema_version: 1,
        pair_session_id: scope.session_id.clone(),
        scope_sha256: scope.digest()?,
        role,
        payload_bytes: definition.payload_bytes,
        payload_sha256: definition
            .payload_sha256
            .context("Missing static payload digest")?,
        data_frames: frames,
        payload_retained: false,
        sink_effects_confirmed: false,
        producer_reported: false,
        sink_reported: false
    }))
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
