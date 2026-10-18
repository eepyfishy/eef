//! Immutable two-endpoint definition; no credentials or execution authority.
use super::{
    admission::Definition,
    scope::{Limits, Role},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub node_id: String,
    pub resource_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    pub schema_version: u32,
    pub source: Endpoint,
    pub destination: Endpoint,
    pub payload_bytes: u64,
    pub payload_sha256: String,
    pub limits: Limits,
}
impl Pair {
    pub fn endpoint(&self, role: Role) -> &Endpoint {
        if role == Role::Source {
            &self.source
        } else {
            &self.destination
        }
    }
    pub fn definition(&self, role: Role) -> Definition {
        let local = self.endpoint(role);
        let remote = self.endpoint(if role == Role::Source {
            Role::Destination
        } else {
            Role::Source
        });
        Definition {
            schema_version: 1,
            resource_id: local.resource_id.clone(),
            peer_node: remote.node_id.clone(),
            peer_resource_id: remote.resource_id.clone(),
            payload_bytes: self.payload_bytes,
            payload_sha256: self.payload_sha256.clone(),
            limits: self.limits.clone(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.source.node_id == self.destination.node_id {
            bail!("Invalid distinct peer pair")
        }
        self.definition(Role::Source).validate()?;
        self.definition(Role::Destination).validate()
    }
    pub fn from_params(value: &Value) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            transfer: Pair,
        }
        let input: Input = serde_json::from_value(value.clone())?;
        input.transfer.validate()?;
        Ok(input.transfer)
    }
}

/// Runtime route, deliberately NOT part of the immutable semantic definition.
/// It still needs explicit routing/disclosure validation at issuance.
pub fn validate_address(address: SocketAddr) -> Result<()> {
    if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
        bail!("Invalid peer destination address")
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub runtime_generation: String,
    pub finite_transport: bool,
    pub destination_address: Option<SocketAddr>,
}
impl Advertisement {
    pub fn validate(&self) -> Result<()> {
        super::scope::uuid(&self.runtime_generation)?;
        if self.schema_version != 1 || !self.finite_transport {
            bail!("Unsupported peer delivery protocol")
        }
        if let Some(address) = self.destination_address {
            validate_address(address)?
        }
        Ok(())
    }
}
