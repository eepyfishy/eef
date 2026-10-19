//! Explicit finite producer bindings for the native control-session executor.
//! No files, devices, capture, playback or implicit permissions. The embedding
//! owner supplies immutable bytes; receive resources verify and discard bytes.
use super::{
    admission::Definition,
    plan::Advertisement,
    scope::{Role, Scope},
};
use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    sync::Arc,
};

// One bounded snapshot across all resources, not a live/unbounded producer.
const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESOURCES: usize = 8;

/// Deliberately not Debug/Serialize: source bytes never enter diagnostics,
/// registration, scope material, model prompts or retained execution receipts.
pub struct FiniteRuntime {
    node_id: String,
    sources: BTreeMap<String, Arc<[u8]>>,
    receivers: BTreeSet<String>,
    address: Option<SocketAddr>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_refuses_unbounded_ambiguous_or_unowned_bindings_without_effects() {
        let source = |id: &str, bytes| BTreeMap::from([(id.into(), bytes)]);
        assert!(FiniteRuntime::new("node", Default::default(), Default::default(), None).is_err());
        for (key, bytes) in [
            ("bad::id", vec![1]),
            ("empty", vec![]),
            ("large", vec![1; MAX_SOURCE_BYTES + 1]),
        ] {
            assert!(
                FiniteRuntime::new("node", source(key, bytes), Default::default(), None).is_err()
            );
        }
        assert!(
            FiniteRuntime::new(
                "node",
                source("transport", vec![1]),
                ["transport".into()].into(),
                Some("127.0.0.1:12345".parse().unwrap())
            )
            .is_err()
        );
        assert!(
            FiniteRuntime::new(
                "node",
                Default::default(),
                ["transport".into()].into(),
                None
            )
            .is_err()
        );
        for address in ["0.0.0.0:12345", "127.0.0.1:0", "224.0.0.1:12345"] {
            assert!(
                FiniteRuntime::new(
                    "node",
                    Default::default(),
                    ["transport".into()].into(),
                    Some(address.parse().unwrap())
                )
                .is_err()
            );
        }
        let runtime = FiniteRuntime::new(
            "node",
            source("transport", vec![1]),
            Default::default(),
            None,
        )
        .unwrap();
        let mut metadata: crate::context::NodeMetadata = serde_json::from_value(
            serde_json::json!({"resources":[{"id":"transport","capability":"peer.transfer"}]}),
        )
        .unwrap();
        runtime.validate_metadata("node", &metadata).unwrap();
        assert!(runtime.validate_metadata("other", &metadata).is_err());
        metadata.resources[0].available = false;
        assert!(runtime.validate_metadata("node", &metadata).is_err());
        metadata.resources[0].available = true;
        metadata.resources[0].capability = "camera.capture".into();
        assert!(runtime.validate_metadata("node", &metadata).is_err());
    }
    #[test]
    fn runtime_matches_manifest_role_and_bound_route_and_discloses_no_payload() {
        let f = crate::peer::tests::Fixture::new();
        let bytes = b"PRIVATE_RESOURCE_BUFFER".to_vec();
        let mut d = Definition {
            schema_version: 1,
            resource_id: f.scope.source.resource_id.clone(),
            peer_node: f.scope.destination.node_id.clone(),
            peer_resource_id: f.scope.destination.resource_id.clone(),
            payload_bytes: bytes.len() as u64,
            payload_sha256: hex::encode(Sha256::digest(&bytes)),
            limits: f.scope.limits.clone(),
        };
        let runtime = FiniteRuntime::new(
            &f.scope.source.node_id,
            BTreeMap::from([("transport".into(), bytes.clone())]),
            Default::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            runtime
                .payload(Role::Source, &d, &f.scope)
                .unwrap()
                .as_deref(),
            Some(bytes.as_slice())
        );
        d.payload_sha256 = "a".repeat(64);
        assert!(runtime.payload(Role::Source, &d, &f.scope).is_err());
        assert!(runtime.payload(Role::Destination, &d, &f.scope).is_err());
        let receiver = FiniteRuntime::new(
            &f.scope.source.node_id,
            Default::default(),
            ["transport".into()].into(),
            Some(f.scope.destination_address),
        )
        .unwrap();
        assert!(
            receiver
                .payload(Role::Destination, &d, &f.scope)
                .unwrap()
                .is_none()
        );
        assert!(receiver.payload(Role::Source, &d, &f.scope).is_err());
        let mut other = f.scope.clone();
        other
            .destination_address
            .set_port(other.destination_address.port() + 1);
        assert!(receiver.payload(Role::Destination, &d, &other).is_err());
        let advertisement = serde_json::to_string(
            &runtime
                .advertisement(&f.scope.source.runtime_generation)
                .unwrap(),
        )
        .unwrap();
        assert!(!advertisement.contains("PRIVATE_RESOURCE_BUFFER"));
    }
}
impl FiniteRuntime {
    /// Resource keys are existing owner-configured LOCAL IDs. An empty or
    /// ambiguous runtime is refused. Binding does not open any sockets.
    pub fn new(
        node_id: &str,
        sources: BTreeMap<String, Vec<u8>>,
        receivers: BTreeSet<String>,
        address: Option<SocketAddr>,
    ) -> Result<Self> {
        crate::network::validate_remote_node_id(node_id)?;
        if let Some(address) = address {
            super::plan::validate_address(address)?;
        }
        let count = sources.len() + receivers.len();
        if !(1..=MAX_RESOURCES).contains(&count)
            || receivers.is_empty() != address.is_none()
            || sources.keys().any(|key| receivers.contains(key))
            || sources.values().any(|bytes| bytes.is_empty())
            || sources
                .values()
                .try_fold(0usize, |n, bytes| n.checked_add(bytes.len()))
                .is_none_or(|n| n > MAX_SOURCE_BYTES)
            || sources.keys().chain(receivers.iter()).any(|key| {
                key.is_empty()
                    || key.len() > 100
                    || !key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        {
            bail!("Invalid bounded finite peer runtime bindings")
        }
        Ok(Self {
            node_id: node_id.into(),
            sources: sources
                .into_iter()
                .map(|(id, bytes)| (id, bytes.into()))
                .collect(),
            receivers,
            address,
        })
    }
    pub(crate) fn validate_metadata(
        &self,
        node: &str,
        metadata: &crate::context::NodeMetadata,
    ) -> Result<()> {
        if node != self.node_id
            || self.sources.keys().chain(self.receivers.iter()).any(|id| {
                !metadata
                    .resources
                    .iter()
                    .any(|r| &r.id == id && r.capability == "peer.transfer" && r.available)
            })
        {
            bail!("Finite peer binding requires an available owner-configured peer resource")
        }
        Ok(())
    }
    pub(crate) fn has_resource(&self, id: &str) -> bool {
        self.sources.contains_key(id) || self.receivers.contains(id)
    }
    pub(crate) fn advertisement(&self, generation: &str) -> Result<Advertisement> {
        let value = Advertisement {
            schema_version: 1,
            runtime_generation: generation.into(),
            finite_transport: true,
            destination_address: self.address,
        };
        value.validate()?;
        Ok(value)
    }
    /// Check immutable manifest and exact local role/address BEFORE a lease,
    /// journal consumption or effects. No route supplied by grant parameters.
    pub(crate) fn payload(
        &self,
        role: Role,
        d: &Definition,
        scope: &Scope,
    ) -> Result<Option<Arc<[u8]>>> {
        d.validate()?;
        let (owner, local) = d
            .resource_id
            .split_once("::")
            .ok_or_else(|| anyhow::anyhow!("Missing resource owner"))?;
        if owner != self.node_id {
            bail!("Peer resource belongs to another runtime")
        }
        if role == Role::Source {
            let bytes = self
                .sources
                .get(local)
                .ok_or_else(|| anyhow::anyhow!("Source producer unavailable"))?;
            if bytes.len() as u64 != d.payload_bytes
                || hex::encode(Sha256::digest(bytes.as_ref())) != d.payload_sha256
            {
                bail!("Source producer differs from approved manifest")
            }
            Ok(Some(bytes.clone()))
        } else {
            if !self.receivers.contains(local) || self.address != Some(scope.destination_address) {
                bail!("Destination resource or route differs from local owner binding")
            }
            Ok(None)
        }
    }
}
