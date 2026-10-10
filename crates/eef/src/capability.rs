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
}

#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    providers: Arc<RwLock<RegistryEntries>>,
    policy: Arc<crate::scheduling::SchedulingPolicy>,
}

type ProviderKey = (String, String, String);
fn provider_key(provider: &CapabilityProvider) -> ProviderKey {
    (
        provider.node_id.clone(),
        provider.capability.clone(),
        provider.action.clone(),
    )
}

#[derive(Default)]
struct RegistryEntries {
    entries: BTreeMap<String, Vec<CapabilityProvider>>,
    reservations: BTreeMap<String, ReservationEntry>,
}
struct ReservationEntry {
    key: ProviderKey,
    current_registration: bool,
}
impl std::ops::Deref for RegistryEntries {
    type Target = BTreeMap<String, Vec<CapabilityProvider>>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
impl std::ops::DerefMut for RegistryEntries {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

/// Coordinator-local capacity reservation, not an execution grant or remote
/// acknowledgement. Dropping it releases only the registration it reserved.
pub struct ProviderReservation {
    registry: CapabilityRegistry,
    id: String,
    provider: CapabilityProvider,
}
impl ProviderReservation {
    pub fn provider(&self) -> &CapabilityProvider {
        &self.provider
    }
}
impl Drop for ProviderReservation {
    fn drop(&mut self) {
        let mut providers = self.registry.providers.write().expect("registry lock");
        let Some(reservation) = providers.reservations.remove(&self.id) else {
            return;
        };
        if !reservation.current_registration {
            return;
        }
        let key = reservation.key;
        if let Some(provider) = providers
            .get_mut(&key.1)
            .and_then(|values| values.iter_mut().find(|p| provider_key(p) == key))
        {
            provider.in_flight = provider.in_flight.saturating_sub(1);
        }
    }
}

impl CapabilityRegistry {
    pub fn with_policy(policy: crate::scheduling::SchedulingPolicy) -> Result<Self> {
        policy.validate()?;
        Ok(Self {
            providers: Default::default(),
            policy: Arc::new(policy),
        })
    }

    pub fn scheduling_policy(&self) -> &crate::scheduling::SchedulingPolicy {
        &self.policy
    }

    /// Bounded local-owner inspection, not executor acknowledgements or grants.
    pub fn admission_snapshot(&self) -> Value {
        let state = self.providers.read().expect("registry lock");
        let counts = reservation_counts(&state);
        let providers = state
            .values()
            .flatten()
            .filter(|p| !matches!(p.capability.as_str(), "llm.infer" | "vlm.analyze"));
        let provider_count = providers.clone().count();
        json!({"total_in_flight":state.reservations.len(),
            "retired_in_flight":state.reservations.values().filter(|r|!r.current_registration).count(),
            "node_counts":counts.iter().take(256).map(|(node,count)|json!({"node_id":node,"in_flight":count})).collect::<Vec<_>>(),
            "nodes_truncated":counts.len()>256,
            "providers":providers.take(512).map(|p|json!({"node_id":p.node_id,"capability":p.capability,"action":p.action,
                "in_flight":p.in_flight,"effective_per_provider_concurrency":provider_ceiling(p, &self.policy)})).collect::<Vec<_>>(),
            "providers_truncated":provider_count>512})
    }

    fn eligible(
        &self,
        state: &RegistryEntries,
        capability: &str,
        action: &str,
    ) -> Vec<CapabilityProvider> {
        if self.policy.max_in_flight > 0
            && state.reservations.len() >= self.policy.max_in_flight as usize
        {
            return Vec::new();
        }
        let counts = reservation_counts(state);
        state
            .get(capability)
            .into_iter()
            .flatten()
            .filter(|p| {
                let limit = self
                    .policy
                    .node_limits
                    .get(&p.node_id)
                    .copied()
                    .unwrap_or(0);
                p.healthy
                    && p.accepts(action)
                    && (limit == 0 || counts.get(&p.node_id).copied().unwrap_or(0) < limit as usize)
            })
            .cloned()
            .collect()
    }

    pub fn register(&self, provider: CapabilityProvider) {
        let mut providers = self.providers.write().expect("registry lock");
        // An explicit replacement is a new registration. A late release from
        // its predecessor must never decrement this registration's counters.
        let key = provider_key(&provider);
        for reservation in providers.reservations.values_mut().filter(|r| r.key == key) {
            reservation.current_registration = false;
        }
        let entries = providers.entry(provider.capability.clone()).or_default();
        entries.retain(|old| !(old.node_id == provider.node_id && old.action == provider.action));
        entries.push(provider);
    }

    pub fn unregister_node(&self, node_id: &str) {
        let mut providers = self.providers.write().expect("registry lock");
        for reservation in providers
            .reservations
            .values_mut()
            .filter(|r| r.key.0 == node_id)
        {
            reservation.current_registration = false;
        }
        providers.retain(|_, values| {
            values.retain(|provider| provider.node_id != node_id);
            !values.is_empty()
        });
    }

    /// Refresh an authenticated node snapshot without resetting scheduler-owned
    /// counters/limits for unchanged capabilities. Hold one lock across replacement.
    pub fn refresh_node(&self, node_id: &str, replacements: Vec<CapabilityProvider>) {
        let mut providers = self.providers.write().expect("registry lock");
        let previous = providers
            .values()
            .flatten()
            .filter(|p| p.node_id == node_id)
            .map(|p| ((p.capability.clone(), p.action.clone()), p.clone()))
            .collect::<BTreeMap<_, _>>();
        providers.retain(|_, values| {
            values.retain(|p| p.node_id != node_id);
            !values.is_empty()
        });
        let retained = replacements
            .iter()
            .filter(|p| p.node_id == node_id)
            .map(provider_key)
            .collect::<std::collections::BTreeSet<_>>();
        for reservation in providers
            .reservations
            .values_mut()
            .filter(|r| r.key.0 == node_id && !retained.contains(&r.key))
        {
            reservation.current_registration = false;
        }
        for mut provider in replacements.into_iter().filter(|p| p.node_id == node_id) {
            if let Some(old) = previous.get(&(provider.capability.clone(), provider.action.clone()))
            {
                provider.load = old.load;
                provider.latency_ms = old.latency_ms;
                provider.priority = old.priority;
                provider.in_flight = old.in_flight;
                provider.max_concurrent = old.max_concurrent;
            }
            let entries = providers.entry(provider.capability.clone()).or_default();
            entries
                .retain(|old| !(old.node_id == provider.node_id && old.action == provider.action));
            entries.push(provider);
        }
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

    /// Registry health only; not a reservation, permission check or live probe.
    pub fn remote_provider_counts(&self) -> (usize, usize) {
        let providers = self.providers.read().expect("registry lock");
        providers
            .values()
            .flatten()
            .filter(|provider| provider.node_id != "local")
            .fold((0, 0), |(total, healthy), provider| {
                (total + 1, healthy + usize::from(provider.healthy))
            })
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
        self.find_best_scoped(capability, action, constraints, context, None)
    }

    /// Apply owner-disclosed executor scope before ranking, never after selecting
    /// an undisclosed best provider. No permission or reservation is implied.
    pub fn find_best_scoped(
        &self,
        capability: &str,
        action: &str,
        constraints: &Value,
        context: Option<&eefn::context::RequestContext>,
        permitted: Option<&std::collections::BTreeSet<String>>,
    ) -> Option<CapabilityProvider> {
        let state = self.providers.read().expect("registry lock");
        select_provider(
            self.eligible(&state, capability, action)
                .into_iter()
                .filter(|p| permitted.is_none_or(|nodes| nodes.contains(&p.node_id)))
                .collect(),
            capability,
            constraints,
            context,
            &self.policy,
        )
    }

    /// Selection and counter increment share ONE lock, so concurrent dispatch
    /// cannot both pass the same max_concurrent check. No await/IO under the lock.
    pub fn reserve_best_with_context(
        &self,
        capability: &str,
        action: &str,
        constraints: &Value,
        context: Option<&eefn::context::RequestContext>,
    ) -> Option<ProviderReservation> {
        let mut providers = self.providers.write().expect("registry lock");
        let candidates = self.eligible(&providers, capability, action);
        let mut selected =
            select_provider(candidates, capability, constraints, context, &self.policy)?;
        let key = provider_key(&selected);
        let current = providers
            .get_mut(capability)?
            .iter_mut()
            .find(|p| provider_key(p) == key)?;
        current.in_flight = current.in_flight.checked_add(1)?;
        selected.in_flight = current.in_flight;
        let id = uuid::Uuid::new_v4().to_string();
        providers.reservations.insert(
            id.clone(),
            ReservationEntry {
                key,
                current_registration: true,
            },
        );
        Some(ProviderReservation {
            registry: self.clone(),
            id,
            provider: selected,
        })
    }

    pub fn update_metrics(
        &self,
        node_id: &str,
        load: f64,
        latency_ms: f64,
        specs: Option<NodeSpecs>,
    ) {
        self.update_metrics_at(node_id, load, latency_ms, specs, eefn::protocol::now_ms());
    }

    pub fn update_metrics_at(
        &self,
        node_id: &str,
        load: f64,
        latency_ms: f64,
        specs: Option<NodeSpecs>,
        received_at_ms: u64,
    ) {
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
            provider.last_heartbeat_ms = received_at_ms;
            if let Some(specs) = &specs {
                provider.node_specs = specs.clone();
            }
        }
    }

    /// Validate against the selected executor, not an unrelated provider's schema.
    pub fn validate_provider_params(provider: &CapabilityProvider, params: &Value) -> Result<()> {
        Self::validate_properties_params(&provider.properties, params)
    }

    pub fn validate_properties_params(properties: &Value, params: &Value) -> Result<()> {
        validate_schema_params(properties.get("params"), params)
    }
}

fn select_provider(
    candidates: Vec<CapabilityProvider>,
    capability: &str,
    constraints: &Value,
    context: Option<&eefn::context::RequestContext>,
    policy: &crate::scheduling::SchedulingPolicy,
) -> Option<CapabilityProvider> {
    let now_ms = eefn::protocol::now_ms();
    let mut candidates = candidates
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
        .filter(|provider| {
            let ceiling = provider_ceiling(provider, policy);
            !stale(provider, now_ms, policy.freshness_seconds * 1000)
                && (ceiling == 0 || provider.in_flight < ceiling)
                && satisfies(provider, constraints)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        let proximity = |provider: &CapabilityProvider| {
            context.map(|context| {
                let area: Vec<String> =
                    serde_json::from_value(provider.properties["area"].clone()).unwrap_or_default();
                eefn::context::locality(context, &provider.node_id, &area)
            })
        };
        proximity(left)
            .cmp(&proximity(right))
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

fn validate_schema_params(schema: Option<&Value>, params: &Value) -> Result<()> {
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

fn satisfies(provider: &CapabilityProvider, constraints: &Value) -> bool {
    if constraints.is_null() {
        return true;
    }
    let Some(constraints) = constraints.as_object() else {
        return false;
    };
    for (key, value) in constraints {
        let matches = match key.as_str() {
            "gpu" => value
                .as_str()
                .is_some_and(|gpu| !gpu.is_empty() && provider.node_specs.gpu_family == gpu),
            "gpu_vram_min_mb" | "vram_min_mb" => value
                .as_u64()
                .is_some_and(|minimum| provider.node_specs.gpu_vram_mb >= minimum),
            "node_id" => value.as_str().is_some_and(|id| provider.node_id == id),
            "node_name" => value
                .as_str()
                .is_some_and(|name| provider.node_name == name),
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

fn stale(provider: &CapabilityProvider, now_ms: u64, freshness_ms: u64) -> bool {
    provider.node_id != "local" && now_ms.saturating_sub(provider.last_heartbeat_ms) > freshness_ms
}

fn provider_ceiling(
    provider: &CapabilityProvider,
    policy: &crate::scheduling::SchedulingPolicy,
) -> u32 {
    match (provider.max_concurrent, policy.per_provider_concurrency) {
        (0, right) => right,
        (left, 0) => left,
        (left, right) => left.min(right),
    }
}

fn reservation_counts(state: &RegistryEntries) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for reservation in state.reservations.values() {
        *counts.entry(reservation.key.0.clone()).or_insert(0) += 1;
    }
    counts
}

fn score(provider: &CapabilityProvider) -> f64 {
    let latency = 1.0 / (1.0 + provider.latency_ms / 100.0);
    // Selection already excluded stale candidates using one sampled timestamp.
    let load = 1.0 - provider.load;
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
    fn origin_scope_filters_before_ranking_without_widening_exact_constraints() {
        let registry = CapabilityRegistry::default();
        let mut preferred = CapabilityProvider::local("system.info", "run", json!({}));
        preferred.node_id = "hidden".into();
        preferred.last_heartbeat_ms = eefn::protocol::now_ms();
        preferred.priority = 100;
        let mut own = preferred.clone();
        own.node_id = "owner".into();
        own.priority = 0;
        registry.register(preferred);
        registry.register(own);
        assert_eq!(
            registry
                .find_best("system.info", "run", &json!({}))
                .unwrap()
                .node_id,
            "hidden"
        );
        let scope = std::collections::BTreeSet::from(["owner".to_string()]);
        assert_eq!(
            registry
                .find_best_scoped("system.info", "run", &json!({}), None, Some(&scope))
                .unwrap()
                .node_id,
            "owner"
        );
        assert!(
            registry
                .find_best_scoped(
                    "system.info",
                    "run",
                    &json!({"node_id":"hidden"}),
                    None,
                    Some(&scope)
                )
                .is_none()
        );
        assert!(
            registry
                .find_best_scoped(
                    "system.info",
                    "run",
                    &json!({}),
                    None,
                    Some(&std::collections::BTreeSet::new())
                )
                .is_none()
        );
    }

    #[test]
    fn aggregate_global_capacity_is_atomic_and_retirement_does_not_free_owned_futures() {
        let registry = CapabilityRegistry::with_policy(crate::scheduling::SchedulingPolicy {
            max_in_flight: 3,
            ..Default::default()
        })
        .unwrap();
        for node in ["a", "b", "c", "d"] {
            let mut provider = CapabilityProvider::local("work", "run", json!({}));
            provider.node_id = node.into();
            registry.register(provider);
        }
        let barrier = std::sync::Barrier::new(24);
        let active = std::thread::scope(|scope| {
            let threads = (0..24)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        registry.reserve_best_with_context("work", "run", &json!({}), None)
                    })
                })
                .collect::<Vec<_>>();
            threads
                .into_iter()
                .filter_map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(active.len(), 3);
        assert_eq!(registry.admission_snapshot()["total_in_flight"], 3);
        for node in ["a", "b", "c", "d"] {
            registry.unregister_node(node);
        }
        assert_eq!(registry.admission_snapshot()["retired_in_flight"], 3);
        registry.register(CapabilityProvider::local("work", "run", json!({})));
        assert!(
            registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .is_none()
        );
        drop(active);
        assert_eq!(registry.admission_snapshot()["total_in_flight"], 0);
        assert!(
            registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .is_some()
        );
    }

    #[test]
    fn node_limits_span_capabilities_and_provider_ceiling_never_widens_executor_limit() {
        let registry = CapabilityRegistry::with_policy(crate::scheduling::SchedulingPolicy {
            node_limits: BTreeMap::from([("node".into(), 1)]),
            per_provider_concurrency: 2,
            ..Default::default()
        })
        .unwrap();
        for capability in ["one", "two"] {
            let mut provider = CapabilityProvider::local(capability, "run", json!({}));
            provider.node_id = "node".into();
            registry.register(provider);
        }
        let active = registry
            .reserve_best_with_context("one", "run", &json!({}), None)
            .unwrap();
        assert!(
            registry
                .reserve_best_with_context("two", "run", &json!({}), None)
                .is_none()
        );
        registry.unregister_node("node");
        let mut replacement = CapabilityProvider::local("two", "run", json!({}));
        replacement.node_id = "node".into();
        replacement.max_concurrent = 1;
        registry.register(replacement);
        assert!(
            registry
                .reserve_best_with_context("two", "run", &json!({}), None)
                .is_none(),
            "retired future still occupies its node budget"
        );
        drop(active);
        let new = registry
            .reserve_best_with_context("two", "run", &json!({}), None)
            .unwrap();
        assert!(
            registry
                .reserve_best_with_context("two", "run", &json!({}), None)
                .is_none()
        );
        assert_eq!(
            registry.admission_snapshot()["providers"][0]["effective_per_provider_concurrency"],
            1
        );
        drop(new);
    }

    #[test]
    fn configured_freshness_and_zero_limits_preserve_legacy_capacity_semantics() {
        let mut provider = CapabilityProvider::local("work", "run", json!({}));
        provider.node_id = "node".into();
        provider.last_heartbeat_ms = eefn::protocol::now_ms() - 15_000;
        let legacy = CapabilityRegistry::default();
        legacy.register(provider.clone());
        assert!(legacy.find_best("work", "run", &json!({})).is_none());
        let configured = CapabilityRegistry::with_policy(crate::scheduling::SchedulingPolicy {
            freshness_seconds: 30,
            ..Default::default()
        })
        .unwrap();
        configured.register(provider);
        let one = configured
            .reserve_best_with_context("work", "run", &json!({}), None)
            .unwrap();
        let two = configured
            .reserve_best_with_context("work", "run", &json!({}), None)
            .unwrap();
        assert_eq!(configured.admission_snapshot()["total_in_flight"], 2);
        drop((one, two));
        assert_eq!(configured.admission_snapshot()["total_in_flight"], 0);
    }

    #[test]
    fn admission_inspection_is_bounded_and_excludes_dedicated_model_routes_and_secrets() {
        let registry = CapabilityRegistry::default();
        for number in 0..513 {
            registry.register(CapabilityProvider::local(
                &format!("cap{number:04}"),
                "run",
                json!({"secret":"PRIVATE"}),
            ));
        }
        registry.register(CapabilityProvider::local("llm.infer", "run", json!({})));
        let value = registry.admission_snapshot();
        assert_eq!(value["providers"].as_array().unwrap().len(), 512);
        assert_eq!(value["providers_truncated"], true);
        assert!(!value.to_string().contains("PRIVATE"));
        assert!(!value.to_string().contains("llm.infer"));
    }

    fn limited(registry: &CapabilityRegistry, node: &str) -> CapabilityProvider {
        let mut provider = CapabilityProvider::local("work", "run", json!({}));
        provider.node_id = node.into();
        provider.max_concurrent = 1;
        registry.register(provider.clone());
        provider
    }

    #[test]
    fn concurrent_selection_reserves_capacity_atomically_and_releases() {
        let registry = CapabilityRegistry::default();
        limited(&registry, "node");
        let barrier = std::sync::Barrier::new(24);
        let reservations = std::thread::scope(|scope| {
            let handles = (0..24)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        registry.reserve_best_with_context("work", "run", &json!({}), None)
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            reservations.len(),
            1,
            "a one-slot provider admitted concurrent work"
        );
        assert_eq!(registry.find("work", "run")[0].in_flight, 1);
        assert!(registry.find_best("work", "run", &json!({})).is_none());
        drop(reservations);
        assert_eq!(registry.find("work", "run")[0].in_flight, 0);
        assert!(
            registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .is_some()
        );
        assert!(registry.providers.read().unwrap().reservations.is_empty());
    }

    #[test]
    fn retired_registration_release_cannot_free_replacement_capacity() {
        for explicit_replace in [true, false] {
            let registry = CapabilityRegistry::default();
            let provider = limited(&registry, "node");
            let old = registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .unwrap();
            if explicit_replace {
                registry.register(provider);
            } else {
                registry.unregister_node("node");
                limited(&registry, "node");
            }
            let new = registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .unwrap();
            drop(old);
            assert_eq!(registry.find("work", "run")[0].in_flight, 1);
            assert!(
                registry
                    .reserve_best_with_context("work", "run", &json!({}), None)
                    .is_none()
            );
            drop(new);
            assert_eq!(registry.find("work", "run")[0].in_flight, 0);
        }
    }

    #[test]
    fn refresh_preserves_reservations_but_removed_capabilities_retire_them() {
        let registry = CapabilityRegistry::default();
        let provider = limited(&registry, "node");
        let active = registry
            .reserve_best_with_context("work", "run", &json!({}), None)
            .unwrap();
        registry.refresh_node("node", vec![provider.clone()]);
        assert_eq!(registry.find("work", "run")[0].in_flight, 1);
        assert!(
            registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .is_none()
        );
        drop(active);
        assert_eq!(registry.find("work", "run")[0].in_flight, 0);
        let retired = registry
            .reserve_best_with_context("work", "run", &json!({}), None)
            .unwrap();
        registry.refresh_node("node", vec![]);
        registry.refresh_node("node", vec![provider]);
        let new = registry
            .reserve_best_with_context("work", "run", &json!({}), None)
            .unwrap();
        drop(retired);
        assert_eq!(registry.find("work", "run")[0].in_flight, 1);
        drop(new);
        assert!(registry.providers.read().unwrap().reservations.is_empty());
    }

    #[test]
    fn stale_and_malformed_target_or_hardware_constraints_never_relax_selection() {
        let registry = CapabilityRegistry::default();
        let mut provider = limited(&registry, "node");
        for constraints in [
            json!(false),
            json!([]),
            json!({"node_id":null}),
            json!({"node_name":123}),
            json!({"gpu":null}),
            json!({"gpu":""}),
            json!({"gpu_vram_min_mb":-1}),
            json!({"vram_min_mb":"1"}),
            json!({"gpu_vram_min_mb":1}),
        ] {
            assert!(
                registry.find_best("work", "run", &constraints).is_none(),
                "{constraints}"
            );
            assert!(
                registry
                    .reserve_best_with_context("work", "run", &constraints, None)
                    .is_none()
            );
        }
        provider.last_heartbeat_ms = 0;
        registry.register(provider);
        assert!(registry.find_best("work", "run", &json!({})).is_none());
        assert!(
            registry
                .reserve_best_with_context("work", "run", &json!({}), None)
                .is_none()
        );
        registry.update_metrics("node", 0.0, 0.0, None);
        assert!(registry.find_best("work", "run", &Value::Null).is_some());
    }

    #[test]
    fn selected_provider_schema_is_not_replaced_by_another_executor() {
        let loose = CapabilityProvider::local("work", "run", json!({}));
        let strict = CapabilityProvider::local(
            "work",
            "run",
            json!({"params":{"count":{"type":"integer","required":true}}}),
        );
        assert!(CapabilityRegistry::validate_provider_params(&loose, &json!({})).is_ok());
        assert!(CapabilityRegistry::validate_provider_params(&strict, &json!({})).is_err());
        assert!(CapabilityRegistry::validate_provider_params(&strict, &json!({"count":2})).is_ok());
    }

    #[test]
    fn diagnostic_counts_separate_remote_providers_from_local_and_health() {
        let registry = CapabilityRegistry::default();
        assert_eq!(registry.remote_provider_counts(), (0, 0));
        registry.register(CapabilityProvider::local("system.ping", "*", json!({})));
        assert_eq!(registry.remote_provider_counts(), (0, 0));
        let mut first = CapabilityProvider::local("http.request", "*", json!({}));
        first.node_id = "fixture".into();
        first.max_concurrent = 1;
        first.in_flight = 1; // Healthy does not imply currently available capacity.
        let mut second = first.clone();
        second.capability = "filesystem.read".into();
        second.healthy = false;
        registry.refresh_node("fixture", vec![first.clone(), second]);
        assert_eq!(registry.remote_provider_counts(), (2, 1));
        registry.refresh_node("fixture", vec![first]);
        assert_eq!(registry.remote_provider_counts(), (1, 1));
        registry.unregister_node("fixture");
        assert_eq!(registry.remote_provider_counts(), (0, 0));
        assert_eq!(registry.all_providers().len(), 1);
    }

    #[test]
    fn model_registration_refresh_retains_active_scheduler_accounting() {
        let registry = CapabilityRegistry::default();
        let mut original = CapabilityProvider::local("system.ping", "*", json!({}));
        original.node_id = "fixture".into();
        original.in_flight = 2;
        original.max_concurrent = 3;
        original.load = 0.7;
        original.latency_ms = 12.0;
        original.priority = 4;
        registry.register(original.clone());
        registry.register(CapabilityProvider::local("local.keep", "*", json!({})));
        let mut refreshed = original.clone();
        refreshed.in_flight = 0;
        refreshed.max_concurrent = 0;
        refreshed.node_name = "renamed".into();
        refreshed.load = 0.0;
        refreshed.priority = 0;
        registry.refresh_node("fixture", vec![refreshed.clone(), refreshed]);
        assert_eq!(registry.find("system.ping", "run").len(), 1);
        let provider = registry.find("system.ping", "run").pop().unwrap();
        assert_eq!(provider.in_flight, 2);
        assert_eq!(provider.max_concurrent, 3);
        assert_eq!(provider.load, 0.7);
        assert_eq!(provider.latency_ms, 12.0);
        assert_eq!(provider.priority, 4);
        assert_eq!(provider.node_name, "renamed");
        registry.refresh_node("fixture", vec![]);
        assert!(registry.find("system.ping", "run").is_empty());
        assert_eq!(registry.find("local.keep", "run").len(), 1);
    }

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
        let reservation = registry
            .reserve_best_with_context("camera.capture", "capture", &json!({}), Some(&context))
            .unwrap();
        assert_eq!(best(json!({})).unwrap().node_id, "room");
        assert!(best(json!({"resource_id":"origin::camera"})).is_none());
        drop(reservation);
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
        let selected = registry.find_best("work", "run", &json!({})).unwrap();
        assert!(CapabilityRegistry::validate_provider_params(&selected, &json!({})).is_err());
        assert!(
            CapabilityRegistry::validate_provider_params(&selected, &json!({"count": 2})).is_ok()
        );
    }
}
