use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const W_LATENCY: f64 = 0.4;
const W_LOAD: f64 = 0.3;
const W_CAPABILITY: f64 = 0.2;
const W_POWER: f64 = 0.1;
const W_PRIORITY: f64 = 0.1;
const STALE_HEARTBEAT_MS: u64 = 10_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeSpecs {
    #[serde(default)]
    pub cpu_cores: u64,
    #[serde(default)]
    pub cpu_freq_mhz: u64,
    #[serde(default)]
    pub ram_mb: u64,
    #[serde(default)]
    pub gpu_model: String,
    #[serde(default)]
    pub gpu_vram_mb: u64,
    #[serde(default)]
    pub gpu_family: String,
    #[serde(default)]
    pub storage_gb: u64,
    #[serde(default)]
    pub network_mbps: f64,
}

impl NodeSpecs {
    pub fn hardware_power(&self) -> f64 {
        let cpu = if self.cpu_freq_mhz == 0 {
            self.cpu_cores as f64
        } else {
            self.cpu_cores as f64 * self.cpu_freq_mhz as f64 / 1000.0
        };
        cpu + self.ram_mb as f64 / 1024.0 + self.gpu_vram_mb as f64 / 1024.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityProvider {
    pub capability: String,
    #[serde(default = "wildcard")]
    pub action: String,
    #[serde(default = "local")]
    pub node_id: String,
    #[serde(default)]
    pub node_name: String,
    #[serde(default = "provider_version")]
    pub version: String,
    #[serde(default)]
    pub properties: Value,
    #[serde(default)]
    pub node_specs: NodeSpecs,
    #[serde(default)]
    pub load: f64,
    #[serde(default)]
    pub latency_ms: f64,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "yes")]
    pub healthy: bool,
    #[serde(default)]
    pub in_flight: u32,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub last_heartbeat_ms: u64,
}

fn wildcard() -> String {
    "*".into()
}
fn local() -> String {
    "local".into()
}
fn provider_version() -> String {
    "1.0".into()
}
const fn yes() -> bool {
    true
}

impl CapabilityProvider {
    pub fn local(capability: &str, action: &str, properties: Value) -> Self {
        Self {
            capability: capability.into(),
            action: action.into(),
            node_id: "local".into(),
            node_name: "main".into(),
            version: "1.0".into(),
            properties,
            node_specs: NodeSpecs::default(),
            load: 0.0,
            latency_ms: 0.0,
            priority: 0,
            healthy: true,
            in_flight: 0,
            max_concurrent: 0,
            last_heartbeat_ms: eefn::protocol::now_ms(),
        }
    }
    fn accepts(&self, action: &str) -> bool {
        self.action == "*" || action == "*" || self.action == action
    }
    fn at_capacity(&self) -> bool {
        self.max_concurrent > 0 && self.in_flight >= self.max_concurrent
    }
}

#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    providers: Arc<RwLock<BTreeMap<String, Vec<CapabilityProvider>>>>,
}

impl CapabilityRegistry {
    pub fn register(&self, provider: CapabilityProvider) {
        let mut providers = self.providers.write().expect("registry lock");
        let entries = providers.entry(provider.capability.clone()).or_default();
        entries.retain(|old| !(old.node_id == provider.node_id && old.action == provider.action));
        entries.push(provider);
    }

    pub fn unregister_node(&self, node_id: &str) {
        let mut providers = self.providers.write().expect("registry lock");
        providers.retain(|_, values| {
            values.retain(|provider| provider.node_id != node_id);
            !values.is_empty()
        });
    }

    pub fn all_capabilities(&self) -> Vec<String> {
        self.providers
            .read()
            .expect("registry lock")
            .keys()
            .cloned()
            .collect()
    }

    pub fn all_providers(&self) -> Vec<CapabilityProvider> {
        self.providers
            .read()
            .expect("registry lock")
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn find(&self, capability: &str, action: &str) -> Vec<CapabilityProvider> {
        self.providers
            .read()
            .expect("registry lock")
            .get(capability)
            .into_iter()
            .flatten()
            .filter(|provider| provider.healthy && provider.accepts(action))
            .cloned()
            .collect()
    }

    pub fn find_best(
        &self,
        capability: &str,
        action: &str,
        constraints: &Value,
    ) -> Option<CapabilityProvider> {
        self.find_best_with_context(capability, action, constraints, None)
    }

    pub fn find_best_with_context(
        &self,
        capability: &str,
        action: &str,
        constraints: &Value,
        context: Option<&eefn::context::RequestContext>,
    ) -> Option<CapabilityProvider> {
        let mut candidates = self
            .find(capability, action)
            .into_iter()
            .flat_map(|provider| {
                let resources = provider
                    .properties
                    .get("resources")
                    .and_then(Value::as_array)
                    .map(|resources| {
                        resources
                            .iter()
                            .filter(|r| r["capability"] == capability)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if resources.is_empty() {
                    return vec![provider];
                }
                resources
                    .into_iter()
                    .filter(|resource| resource["available"] == true)
                    .filter_map(|resource| {
                        let id = resource["id"].as_str()?;
                        let mut candidate = provider.clone();
                        candidate.properties["selected_resource_id"] =
                            json!(eefn::context::resource_id(&provider.node_id, id));
                        candidate.properties["area"] = resource["area"].clone();
                        Some(candidate)
                    })
                    .collect()
            })
            .filter(|provider| !provider.at_capacity() && satisfies(provider, constraints))
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let proximity = |provider: &CapabilityProvider| {
                context.map(|context| {
                    let area: Vec<String> =
                        serde_json::from_value(provider.properties["area"].clone())
                            .unwrap_or_default();
                    eefn::context::locality(context, &provider.node_id, &area)
                })
            };
            stale(left)
                .cmp(&stale(right))
                .then_with(|| proximity(left).cmp(&proximity(right)))
                .then_with(|| {
                    score(right)
                        .total_cmp(&score(left))
                        .then_with(|| (left.node_id != "local").cmp(&(right.node_id != "local")))
                })
                .then_with(|| left.node_id.cmp(&right.node_id))
                .then_with(|| {
                    left.properties["selected_resource_id"]
                        .as_str()
                        .cmp(&right.properties["selected_resource_id"].as_str())
                })
        });
        candidates.into_iter().next()
    }

    pub fn acquire(&self, provider: &CapabilityProvider) {
        self.mutate_provider(provider, |entry| {
            entry.in_flight = entry.in_flight.saturating_add(1)
        });
    }
    pub fn release(&self, provider: &CapabilityProvider) {
        self.mutate_provider(provider, |entry| {
            entry.in_flight = entry.in_flight.saturating_sub(1)
        });
    }

    fn mutate_provider(
        &self,
        provider: &CapabilityProvider,
        update: impl FnOnce(&mut CapabilityProvider),
    ) {
        if let Some(entry) = self
            .providers
            .write()
            .expect("registry lock")
            .get_mut(&provider.capability)
            .and_then(|values| {
                values.iter_mut().find(|entry| {
                    entry.node_id == provider.node_id && entry.action == provider.action
                })
            })
        {
            update(entry);
        }
    }

    pub fn update_metrics(
        &self,
        node_id: &str,
        load: f64,
        latency_ms: f64,
        specs: Option<NodeSpecs>,
    ) {
        let now = eefn::protocol::now_ms();
        for provider in self
            .providers
            .write()
            .expect("registry lock")
            .values_mut()
            .flatten()
            .filter(|provider| provider.node_id == node_id)
        {
            provider.load = load.clamp(0.0, 1.0);
            provider.latency_ms = latency_ms.max(0.0);
            provider.last_heartbeat_ms = now;
            if let Some(specs) = &specs {
                provider.node_specs = specs.clone();
            }
        }
    }

    pub fn validate_params(&self, capability: &str, action: &str, params: &Value) -> Result<()> {
        let schema = self
            .find(capability, action)
            .into_iter()
            .find_map(|provider| provider.properties.get("params").cloned());
        let Some(schema) = schema.and_then(|value| value.as_object().cloned()) else {
            return Ok(());
        };
        let params = params.as_object().cloned().unwrap_or_default();
        let mut errors = Vec::new();
        for (name, raw_rule) in schema {
            let rule = raw_rule
                .as_object()
                .cloned()
                .unwrap_or_else(|| json!({"type": raw_rule}).as_object().unwrap().clone());
            let value = params.get(&name);
            if value.is_none() || value == Some(&Value::Null) {
                if rule.get("required").and_then(Value::as_bool) == Some(true) {
                    errors.push(format!("missing required param '{name}'"));
                }
                continue;
            }
            let value = value.unwrap();
            let expected = rule.get("type").and_then(Value::as_str).unwrap_or("any");
            if !type_matches(value, expected) {
                errors.push(format!("param '{name}' must be {expected}"));
                continue;
            }
            if let Some(allowed) = rule.get("enum").and_then(Value::as_array) {
                if !allowed.contains(value) {
                    errors.push(format!("param '{name}' is not an allowed value"));
                }
            }
            if let Some(number) = value.as_f64() {
                if rule
                    .get("min")
                    .and_then(Value::as_f64)
                    .is_some_and(|min| number < min)
                {
                    errors.push(format!("param '{name}' is below minimum"));
                }
                if rule
                    .get("max")
                    .and_then(Value::as_f64)
                    .is_some_and(|max| number > max)
                {
                    errors.push(format!("param '{name}' is above maximum"));
                }
            }
        }
        if !errors.is_empty() {
            bail!(errors.join(", "))
        }
        Ok(())
    }
}

fn satisfies(provider: &CapabilityProvider, constraints: &Value) -> bool {
    let Some(constraints) = constraints.as_object() else {
        return true;
    };
    for (key, value) in constraints {
        let matches = match key.as_str() {
            "gpu" => value
                .as_str()
                .is_none_or(|gpu| gpu.is_empty() || provider.node_specs.gpu_family == gpu),
            "gpu_vram_min_mb" | "vram_min_mb" => value.as_u64().is_none_or(|minimum| {
                provider.node_specs.gpu_vram_mb == 0 || provider.node_specs.gpu_vram_mb >= minimum
            }),
            "node_id" => value.as_str().is_none_or(|id| provider.node_id == id),
            "node_name" => value.as_str().is_none_or(|name| provider.node_name == name),
            "resource_id" => {
                value.is_string() && provider.properties.get("selected_resource_id") == Some(value)
            }
            "area" => value.is_array() && provider.properties.get("area") == Some(value),
            _ => true,
        };
        if !matches {
            return false;
        }
    }
    true
}

fn stale(provider: &CapabilityProvider) -> bool {
    provider.node_id != "local"
        && eefn::protocol::now_ms().saturating_sub(provider.last_heartbeat_ms) > STALE_HEARTBEAT_MS
}

fn score(provider: &CapabilityProvider) -> f64 {
    let latency = 1.0 / (1.0 + provider.latency_ms / 100.0);
    let load = if stale(provider) {
        0.0
    } else {
        1.0 - provider.load
    };
    let power = provider.node_specs.hardware_power();
    let power = if power == 0.0 {
        0.0
    } else {
        power / (power + 1.0)
    };
    latency * W_LATENCY
        + load * W_LOAD
        + W_CAPABILITY
        + power * W_POWER
        + f64::from(provider.priority) * W_PRIORITY
}

fn type_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "any" => true,
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "dict" | "object" => value.is_object(),
        "list" | "array" => value.is_array(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_resource_routing_respects_health_capacity_and_explicit_targets() {
        let registry = CapabilityRegistry::default();
        let context = eefn::context::RequestContext::new(
            "req".into(),
            "origin".into(),
            vec!["Home".into(), "Office".into()],
        )
        .unwrap();
        let mut origin = CapabilityProvider::local(
            "camera.capture",
            "*",
            json!({"area":["Home","Office"],"resources":[{"id":"camera","capability":"camera.capture","area":["Home","Office"],"available":true}]}),
        );
        origin.node_id = "origin".into();
        origin.load = 0.9;
        origin.max_concurrent = 1;
        let mut room = origin.clone();
        room.node_id = "room".into();
        room.load = 0.0;
        let mut remote = room.clone();
        remote.node_id = "remote".into();
        remote.properties["resources"][0]["area"] = json!(["Elsewhere"]);
        registry.register(origin.clone());
        registry.register(room.clone());
        registry.register(remote);
        let best = |constraints: Value| {
            registry.find_best_with_context(
                "camera.capture",
                "capture",
                &constraints,
                Some(&context),
            )
        };
        assert_eq!(best(json!({})).unwrap().node_id, "origin");
        registry.acquire(&origin);
        assert_eq!(best(json!({})).unwrap().node_id, "room");
        assert!(best(json!({"resource_id":"origin::camera"})).is_none());
        registry.release(&origin);
        origin.last_heartbeat_ms = 0;
        registry.register(origin.clone());
        assert_eq!(best(json!({})).unwrap().node_id, "room");
        origin.healthy = false;
        registry.register(origin);
        room.properties["resources"][0]["available"] = json!(false);
        registry.register(room);
        assert_eq!(best(json!({})).unwrap().node_id, "remote");
        assert!(best(json!({"resource_id":"room::camera"})).is_none());
        assert!(best(json!({"area":["Home","Office"]})).is_none());
        assert_eq!(
            best(json!({"resource_id":"remote::camera"}))
                .unwrap()
                .properties["selected_resource_id"],
            "remote::camera"
        );
    }

    #[test]
    fn real_load_capacity_constraints_and_validation() {
        let registry = CapabilityRegistry::default();
        let mut busy = CapabilityProvider::local(
            "work",
            "*",
            json!({"params": {"count": {"type": "integer", "required": true}}}),
        );
        busy.node_id = "busy".into();
        busy.load = 0.9;
        busy.last_heartbeat_ms = eefn::protocol::now_ms();
        let mut idle = busy.clone();
        idle.node_id = "idle".into();
        idle.load = 0.1;
        idle.node_specs.gpu_family = "cuda".into();
        registry.register(busy);
        registry.register(idle);
        assert_eq!(
            registry
                .find_best("work", "run", &json!({}))
                .unwrap()
                .node_id,
            "idle"
        );
        assert!(
            registry
                .find_best("work", "run", &json!({"gpu": "vulkan"}))
                .is_none()
        );
        assert!(registry.validate_params("work", "run", &json!({})).is_err());
        assert!(
            registry
                .validate_params("work", "run", &json!({"count": 2}))
                .is_ok()
        );
    }
}
