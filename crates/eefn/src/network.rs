//! Validated advertisements. Metadata is not proof of reachability or authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkAdvertisement {
    #[serde(default)]
    pub advertised_address: Option<String>,
    #[serde(default)]
    pub coordinator: Option<CoordinatorAdvertisement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorAdvertisement {
    pub coordinator_id: String,
    pub address: String,
    #[serde(default)]
    pub state: CoordinatorState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinatorState {
    Active,
    Standby,
    #[default]
    Unknown,
}

impl NetworkAdvertisement {
    pub fn validate(&self) -> Result<()> {
        if let Some(address) = &self.advertised_address {
            validate_address(address, false)?;
        }
        if let Some(coordinator) = &self.coordinator {
            let id = &coordinator.coordinator_id;
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            {
                bail!(
                    "coordinator_id must be 1-128 ASCII letters, digits, dot, underscore or hyphen"
                )
            }
            validate_address(&coordinator.address, true)?;
        }
        Ok(())
    }
}

/// No DNS lookup, socket connection, protocol mode or inferred service port.
pub fn validate_address(address: &str, require_port: bool) -> Result<()> {
    use std::net::IpAddr;
    if address.is_empty()
        || address.len() > 320
        || address.trim() != address
        || address.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        bail!("advertised address must be a bounded host or host:port")
    }
    let (host, port) = if let Some(rest) = address.strip_prefix('[') {
        let (host, suffix) = rest
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("invalid bracketed IPv6 address"))?;
        if !matches!(host.parse::<IpAddr>(), Ok(IpAddr::V6(_))) {
            bail!("brackets require IPv6")
        }
        (
            host,
            if suffix.is_empty() {
                None
            } else {
                Some(
                    suffix
                        .strip_prefix(':')
                        .ok_or_else(|| anyhow::anyhow!("invalid IPv6 port"))?,
                )
            },
        )
    } else if address.parse::<IpAddr>().is_ok() {
        (address, None)
    } else if let Some((host, port)) = address.split_once(':') {
        (host, Some(port))
    } else {
        (address, None)
    };
    if let Some(port) = port {
        if port.is_empty()
            || !port.bytes().all(|c| c.is_ascii_digit())
            || port.parse::<u16>().ok().filter(|n| *n > 0).is_none()
        {
            bail!("advertised service port must be 1-65535")
        }
    } else if require_port {
        bail!("coordinator advertisement requires an explicit service port")
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => {
            if ip.is_unspecified()
                || ip.is_multicast()
                || ip == IpAddr::V4(std::net::Ipv4Addr::BROADCAST)
            {
                bail!("advertise a unicast address, not a wildcard, multicast or broadcast address")
            }
        }
        Err(_) => {
            if host.is_empty()
                || host.len() > 253
                || host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                || !host.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                bail!("invalid advertised hostname or IP address")
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkChanges {
    pub name: Option<String>,
    pub advertised_address: Option<String>,
    #[serde(default)]
    pub clear_advertised_address: bool,
    pub coordinator: Option<CoordinatorAdvertisement>,
    #[serde(default)]
    pub clear_coordinator: bool,
}

impl NetworkChanges {
    pub fn apply(&self, value: &mut serde_json::Value) -> Result<()> {
        if (self.advertised_address.is_some() && self.clear_advertised_address)
            || (self.coordinator.is_some() && self.clear_coordinator)
        {
            bail!("cannot set and clear the same advertisement")
        }
        if let Some(name) = &self.name {
            crate::setup::validate_name(name)?;
        }
        let mut network: NetworkAdvertisement = serde_json::from_value(
            value
                .get("network")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?;
        if let Some(address) = &self.advertised_address {
            network.advertised_address = Some(address.clone());
        }
        if self.clear_advertised_address {
            network.advertised_address = None;
        }
        if let Some(coordinator) = &self.coordinator {
            network.coordinator = Some(coordinator.clone());
        }
        if self.clear_coordinator {
            network.coordinator = None;
        }
        network.validate()?;
        if let Some(name) = &self.name {
            value["name"] = serde_json::json!(name);
        }
        value["network"] = serde_json::to_value(network)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkCommand {
    Show,
    Diagnose,
    Set { changes: NetworkChanges },
    Peers { query: PeerQuery },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerQuery {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default = "page_size")]
    pub limit: usize,
}

fn page_size() -> usize {
    32
}
impl Default for PeerQuery {
    fn default() -> Self {
        Self {
            node_id: None,
            after: None,
            limit: page_size(),
        }
    }
}
pub fn validate_node_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || id.trim() != id || id.chars().any(char::is_control) {
        bail!("node identifier must contain 1-128 bytes without control characters")
    }
    Ok(())
}
impl PeerQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=32).contains(&self.limit) {
            bail!("network page limit must be 1-32")
        }
        if self.node_id.is_some() && self.after.is_some() {
            bail!("node lookup cannot include a page cursor")
        }
        for id in self.node_id.iter().chain(self.after.iter()) {
            validate_node_id(id)?;
        }
        Ok(())
    }
}

/// Only these registration fields may enter a peer response. No raw metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerModel {
    pub model_id: String,
    pub backend: Option<String>,
    pub modality: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerRecord {
    pub node_id: String,
    pub display_name: String,
    pub network: NetworkAdvertisement,
    pub capabilities: Vec<String>,
    pub models: Vec<PeerModel>,
}

impl PeerRecord {
    pub fn from_registration(message: &serde_json::Value) -> Result<Self> {
        use serde_json::Value;
        fn field(value: &Value, key: &str, max: usize) -> Result<Option<String>> {
            let Some(value) = value.get(key) else {
                return Ok(None);
            };
            let s = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("registration {key} must be text"))?;
            if s.len() > max || s.chars().any(char::is_control) {
                bail!("registration {key} exceeds text bounds")
            }
            Ok(Some(s.into()))
        }
        let node_id = field(message, "node_id", 128)?
            .ok_or_else(|| anyhow::anyhow!("registration needs node_id"))?;
        validate_node_id(&node_id)?;
        let display_name = field(message, "name", 256)?.unwrap_or_else(|| node_id.clone());
        let network: NetworkAdvertisement = serde_json::from_value(
            message
                .get("network")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )?;
        network.validate()?;
        let capabilities: Vec<String> = serde_json::from_value(
            message
                .get("capabilities")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )?;
        if capabilities.len() > 128
            || capabilities
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
        {
            bail!("registration capability list exceeds bounds")
        }
        let mut models = Vec::new();
        if let Some(raw) = message.get("models") {
            let raw = raw
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("registration models must be a list"))?;
            if raw.len() > 64 {
                bail!("registration model list exceeds 64 entries")
            }
            for model in raw {
                if !model.is_object() {
                    bail!("registration model must be an object")
                }
                let model_id = field(model, "model_id", 256)?
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("registration model needs model_id"))?;
                models.push(PeerModel {
                    model_id,
                    backend: field(model, "backend", 64)?,
                    modality: field(model, "modality", 64)?,
                });
            }
        }
        Ok(Self {
            node_id,
            display_name,
            network,
            capabilities,
            models,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: NetworkCommand,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peer_queries_and_registration_projections_are_bounded() {
        assert!(serde_json::from_str::<PeerQuery>(r#"{"requester":"forged"}"#).is_err());
        assert!(
            PeerQuery {
                node_id: Some("a".into()),
                after: Some("b".into()),
                limit: 1
            }
            .validate()
            .is_err()
        );
        assert!(
            PeerQuery {
                limit: 0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let raw = serde_json::json!({"node_id":"a","models":[{"model_id":"m","backend":"ollama","path":"SECRET","token":"SECRET"}],"source_ip":"SECRET","metadata":{"binding":"SECRET"}});
        let record = PeerRecord::from_registration(&raw).unwrap();
        assert!(!serde_json::to_string(&record).unwrap().contains("SECRET"));
        assert!(PeerRecord::from_registration(&serde_json::json!({"node_id":"a","models":vec![serde_json::json!({"model_id":"m"});65]})).is_err());
        assert!(
            PeerRecord::from_registration(
                &serde_json::json!({"node_id":"a","capabilities":vec!["x";129]})
            )
            .is_err()
        );
    }
    #[test]
    fn validates_addresses_without_resolving_or_inventing_ports() {
        for address in [
            "26.1.2.3",
            "127.0.0.1:51335",
            "node.example:123",
            "::1",
            "[2001:db8::1]:51335",
        ] {
            validate_address(address, false).unwrap();
        }
        for address in [
            "",
            "0.0.0.0",
            "::",
            "255.255.255.255",
            "224.0.0.1",
            "http://node:12",
            "user@host:12",
            "node:0",
            "node:65536",
            "node:+12",
            " node",
            "bad/path",
            "999.1.1.1",
            "[node]:12",
            "node:12:13",
        ] {
            assert!(validate_address(address, false).is_err(), "{address}");
        }
        assert!(validate_address("26.1.2.3", true).is_err());
        assert!(validate_address("::1", true).is_err());
        validate_address("[::1]:51335", true).unwrap();
    }
    #[test]
    fn legacy_defaults_and_unknown_authority_are_explicit() {
        let legacy: NetworkAdvertisement = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy, NetworkAdvertisement::default());
        let coordinator: CoordinatorAdvertisement =
            serde_json::from_str(r#"{"coordinator_id":"local-eef","address":"26.1.2.3:51335"}"#)
                .unwrap();
        assert_eq!(coordinator.state, CoordinatorState::Unknown);
    }
}
