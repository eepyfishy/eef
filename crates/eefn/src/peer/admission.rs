//! Finite transport-only grants using the EXISTING consumed receipt journal.
//! Not a producer/sink permission, paired job driver, socket service or app API.
use super::{
    PeerEndpoint, Recipient, SealedMaterial,
    scope::{Limits, Role, Scope},
};
use crate::grants::{AcceptedGrant, ExecutionGrant, GrantLedger, GrantReport};
use anyhow::{Result, bail};
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
    pub payload_sha256: String,
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
        if self.schema_version != 1
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
            || !crate::request_result::valid_digest(&self.payload_sha256)
            || self.payload_bytes > self.limits.max_wire_bytes
            || self
                .payload_bytes
                .div_ceil(u64::from(self.limits.max_frame_bytes))
                > u64::from(self.limits.max_frames)
        {
            bail!("Invalid finite peer definition")
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
}
pub(crate) fn confirmed_result(g: &ExecutionGrant, value: &Value) -> bool {
    let Ok((role, definition)) = Definition::from_grant(g) else {
        return false;
    };
    let Ok(r) = serde_json::from_value::<TransferReceipt>(value.clone()) else {
        return false;
    };
    r.schema_version == 1
        && super::scope::uuid(&r.pair_session_id).is_ok()
        && crate::request_result::valid_digest(&r.scope_sha256)
        && r.role == role
        && r.payload_bytes == definition.payload_bytes
        && r.payload_sha256 == definition.payload_sha256
        && u64::from(r.data_frames)
            == definition
                .payload_bytes
                .div_ceil(u64::from(definition.limits.max_frame_bytes))
        && !r.payload_retained
        && !r.sink_effects_confirmed
}

/// Holds an already consumed ordinary grant, one node-wide transport slot and
/// the private endpoint together. No channel extraction or reconnect method.
pub struct AdmittedPeer {
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
    /// result for either physical endpoint. IO must come from future admission.
    pub async fn run<S: crate::tls::NodeIo + 'static>(
        self,
        engine: &crate::NodeEngine,
        stream: S,
        source_payload: Option<Vec<u8>>,
    ) -> GrantReport {
        let Self {
            accepted,
            endpoint,
            role,
            scope,
            definition,
            _slot,
        } = self;
        let operation = async move {
            let frames = definition
                .payload_bytes
                .div_ceil(u64::from(definition.limits.max_frame_bytes))
                as u32;
            if role == Role::Source {
                let bytes = source_payload
                    .ok_or_else(|| anyhow::anyhow!("Missing approved source bytes"))?;
                if bytes.len() as u64 != definition.payload_bytes
                    || hex::encode(Sha256::digest(&bytes)) != definition.payload_sha256
                {
                    bail!("Source bytes differ from approved manifest")
                }
                let mut channel = endpoint.connect(stream).await?;
                for frame in bytes.chunks(definition.limits.max_frame_bytes as usize) {
                    channel.send_data(frame).await?;
                    channel.receive_acknowledgement().await?;
                }
            } else {
                if source_payload.is_some() {
                    bail!("Destination must not supply source bytes")
                }
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
                        && hex::encode(hash.clone().finalize()) != definition.payload_sha256
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
                payload_sha256: definition.payload_sha256,
                data_frames: frames,
                payload_retained: false,
                sink_effects_confirmed: false
            }))
        };
        let report = accepted.run_operation(engine, operation).await;
        drop(_slot);
        report
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
