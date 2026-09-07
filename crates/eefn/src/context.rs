//! Compatible location/resource metadata and immutable request-origin snapshots.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

pub type AreaPath = Vec<String>;

pub fn validate_area(area: &[String]) -> Result<()> {
    if area.len() > 16
        || area.iter().any(|part| {
            part.trim().is_empty()
                || part.len() > 128
                || part.contains('/')
                || part.chars().any(char::is_control)
        })
    {
        bail!("area must have at most 16 nonempty levels without slashes or control characters")
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeMetadata {
    #[serde(default)]
    pub area: AreaPath,
    #[serde(default)]
    pub hardware_type: String,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub capability: String,
    #[serde(default)]
    pub area: AreaPath,
    #[serde(default = "yes")]
    pub available: bool,
    /// Owner-configured adapter bindings, never advertised to other nodes/EEF.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub parameters: Map<String, Value>,
    #[serde(default)]
    pub properties: BTreeMap<String, Value>,
}
fn yes() -> bool {
    true
}

impl NodeMetadata {
    pub fn validate(&self) -> Result<()> {
        validate_area(&self.area)?;
        if self.resources.len() > 128 || serde_json::to_vec(self)?.len() > 128 * 1024 {
            bail!("node metadata exceeds the supported size")
        }
        let mut ids = HashSet::new();
        for resource in &self.resources {
            if resource.id.is_empty()
                || resource.id.len() > 100
                || !resource
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                || !ids.insert(&resource.id)
            {
                bail!(
                    "resource IDs must be unique on the node and use letters, numbers, dot, underscore or hyphen"
                )
            }
            if resource.capability.trim().is_empty() || resource.capability.len() > 128 {
                bail!("resource capability is required")
            }
            if resource.parameters.contains_key("resource_id")
                || resource.parameters.contains_key("request_context")
            {
                bail!("resource bindings cannot redefine routing or origin")
            }
            validate_area(&resource.area)?;
        }
        Ok(())
    }

    pub fn advertised(&self, capabilities: &[String]) -> Self {
        let mut result = self.clone();
        for resource in &mut result.resources {
            resource.parameters.clear();
            resource.available &= capabilities.contains(&resource.capability);
            if resource.area.is_empty() {
                resource.area = self.area.clone();
            }
        }
        result
    }
}

pub fn resource_id(node_id: &str, local_id: &str) -> String {
    format!("{node_id}::{local_id}")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawRequestContext")]
pub struct RequestContext {
    request_id: String,
    origin_node: String,
    origin_area: AreaPath,
}

#[derive(Deserialize)]
struct RawRequestContext {
    request_id: String,
    origin_node: String,
    origin_area: AreaPath,
}
impl TryFrom<RawRequestContext> for RequestContext {
    type Error = anyhow::Error;
    fn try_from(raw: RawRequestContext) -> Result<Self> {
        Self::new(raw.request_id, raw.origin_node, raw.origin_area)
    }
}
impl RequestContext {
    pub fn new(request_id: String, origin_node: String, origin_area: AreaPath) -> Result<Self> {
        if request_id.is_empty()
            || request_id.len() > 128
            || origin_node.is_empty()
            || origin_node.len() > 128
        {
            bail!("invalid request identity")
        }
        validate_area(&origin_area)?;
        Ok(Self {
            request_id,
            origin_node,
            origin_area,
        })
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn origin_node(&self) -> &str {
        &self.origin_node
    }
    pub fn origin_area(&self) -> &[String] {
        &self.origin_area
    }
}

/// Lower ranks are more local. Unknown locations do not imply proximity.
pub fn locality(origin: &RequestContext, node: &str, area: &[String]) -> (u8, usize) {
    if origin.origin_node == node {
        return (0, 0);
    }
    if origin.origin_area.is_empty() || area.is_empty() {
        return (3, usize::MAX);
    }
    let common = origin
        .origin_area
        .iter()
        .zip(area)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return (3, usize::MAX);
    }
    if origin.origin_area == area {
        return (1, 0);
    }
    (2, origin.origin_area.len() + area.len() - 2 * common)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn decoded_context_and_metadata_enforce_bounds() {
        assert!(
            serde_json::from_value::<RequestContext>(
                json!({"request_id":"", "origin_node":"node", "origin_area":[]})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RequestContext>(
                json!({"request_id":"req", "origin_node":"node", "origin_area":["Home/Office"]})
            )
            .is_err()
        );
        for value in [
            json!({"area":vec!["level";17]}),
            json!({"resources":[{"id":"bad::id","capability":"camera.capture"}]}),
            json!({"resources":[{"id":"camera","capability":"camera.capture","parameters":{"request_context":{}}}]}),
            json!({"extensions":{"oversized":"x".repeat(128*1024)}}),
        ] {
            assert!(
                serde_json::from_value::<NodeMetadata>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
    #[test]
    fn compatible_defaults_and_no_private_resource_bindings_on_wire() {
        let empty: NodeMetadata = serde_json::from_value(json!({})).unwrap();
        empty.validate().unwrap();
        let mut metadata: NodeMetadata=serde_json::from_value(json!({"area":["home","office"],"resources":[{"id":"camera","capability":"camera.capture","parameters":{"device":2,"password":"private"}}]})).unwrap();
        metadata.validate().unwrap();
        let advertised = metadata.advertised(&[]);
        assert!(!advertised.resources[0].available);
        assert_eq!(advertised.resources[0].area, metadata.area);
        assert!(
            !serde_json::to_string(&advertised)
                .unwrap()
                .contains("private")
        );
        metadata.resources.push(metadata.resources[0].clone());
        assert!(metadata.validate().is_err());
        assert_ne!(
            resource_id("node-a", "camera"),
            resource_id("node-b", "camera")
        );
    }
    #[test]
    fn locality_prefers_origin_then_room_then_shared_ancestors() {
        let origin = RequestContext::new(
            "request".into(),
            "a".into(),
            vec!["home".into(), "office".into(), "desk".into()],
        )
        .unwrap();
        assert!(locality(&origin, "a", &[]) < locality(&origin, "b", origin.origin_area()));
        assert!(
            locality(&origin, "b", origin.origin_area())
                < locality(&origin, "c", &["home".into(), "office".into()])
        );
        assert!(
            locality(&origin, "c", &["home".into(), "office".into()]) < locality(&origin, "d", &[])
        );
    }
}
