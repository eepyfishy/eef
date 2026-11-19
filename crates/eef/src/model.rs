use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use eefn::NodeServer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::warn;

use crate::event::EventBus;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Vlm,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    Warm,
    Unloaded,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelSpec {
    pub model_id: String,
    pub modality: Modality,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub quality_tier: String,
    #[serde(default)]
    pub vram_needed_mb: u64,
    #[serde(default = "unknown")]
    pub backend: String,
    #[serde(default = "unknown")]
    pub hardware: String,
    #[serde(default = "unknown")]
    pub node_id: String,
    #[serde(default = "unloaded")]
    pub status: ModelStatus,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub model_metadata: Option<eefn::model_metadata::ModelMetadata>,
}

fn unknown() -> String {
    "unknown".into()
}
const fn unloaded() -> ModelStatus {
    ModelStatus::Unloaded
}

#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub modality: Modality,
    pub tier: Option<String>,
    pub model_id: Option<String>,
    pub constraints: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteQuery {
    pub capability: String,
    pub role: Option<String>,
    pub node_id: Option<String>,
    pub backend: Option<String>,
    pub model_id: Option<String>,
    #[serde(default = "route_limit")]
    pub limit: usize,
}
fn route_limit() -> usize {
    8
}
fn role_identifier(role: &str) -> Result<()> {
    if role.is_empty()
        || role.len() > 64
        || !role
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        bail!("model role must be a bounded ASCII identifier")
    }
    Ok(())
}
pub fn validate_model_constraints(constraints: &Value) -> Result<()> {
    if let Some(role) = constraints.get("model_role") {
        role_identifier(role.as_str().context("model_role must be text")?)?;
    }
    Ok(())
}
impl RouteQuery {
    pub fn request(&self) -> Result<ModelRequest> {
        let modality = match self.capability.as_str() {
            "llm.infer" => Modality::Text,
            "vlm.analyze" => Modality::Vlm,
            _ => bail!("route preview currently supports llm.infer and vlm.analyze"),
        };
        if !(1..=32).contains(&self.limit) {
            bail!("route preview limit must be 1-32")
        }
        if let Some(role) = &self.role {
            role_identifier(role)?;
        }
        if let Some(id) = &self.node_id {
            eefn::network::validate_node_id(id)?;
        }
        if let Some(id) = &self.model_id {
            eefn::model_selection::validate_id(id)?;
        }
        if let Some(backend) = &self.backend {
            role_identifier(backend)?;
        }
        let mut constraints = json!({});
        for (key, value) in [
            ("model_role", &self.role),
            ("node_id", &self.node_id),
            ("backend", &self.backend),
        ] {
            if let Some(value) = value {
                constraints[key] = json!(value);
            }
        }
        Ok(ModelRequest {
            modality,
            tier: None,
            model_id: self.model_id.clone(),
            constraints,
        })
    }
}

#[derive(Default)]
struct RegistryState {
    models: BTreeMap<String, Vec<ModelSpec>>,
    loads: HashMap<String, f64>,
    memory: HashMap<String, (u64, Instant)>,
    request_admission: HashMap<String, eefn::model_lifecycle_policy::RequestAdmission>,
}

#[derive(Clone, Default)]
pub struct ModelRegistry {
    state: Arc<RwLock<RegistryState>>,
}

impl ModelRegistry {
    pub fn register(&self, spec: ModelSpec) {
        let mut state = self.state.write().expect("model lock");
        let values = state.models.entry(spec.model_id.clone()).or_default();
        values.retain(|old| !(old.node_id == spec.node_id && old.backend == spec.backend));
        values.push(spec);
    }

    pub fn register_remote(&self, node_id: &str, models: &[Value]) -> Result<()> {
        self.register_snapshot(node_id, models, None)
    }
    pub fn register_snapshot(
        &self,
        node_id: &str,
        models: &[Value],
        admission: Option<&Value>,
    ) -> Result<()> {
        let admission = admission
            .map(
                |raw| -> Result<eefn::model_lifecycle_policy::RequestAdmission> {
                    let snapshot: eefn::model_lifecycle_policy::RequestAdmission =
                        serde_json::from_value(raw.clone())?;
                    snapshot.validate()?;
                    Ok(snapshot)
                },
            )
            .transpose()?;
        // Validate the complete snapshot before replacing any live entries.
        let record = eefn::network::PeerRecord::from_registration(
            &json!({"node_id":node_id,"models":models}),
        )?;
        let mut replacements = Vec::new();
        for (advertised, entry) in record.models.into_iter().zip(models) {
            let metadata = advertised.normalized_metadata();
            let modality = if metadata.supports("vlm.analyze") {
                Modality::Vlm
            } else if metadata.supports("llm.infer") {
                Modality::Text
            } else {
                Modality::Other
            };
            replacements.push(ModelSpec {
                model_id: advertised.model_id,
                modality,
                family: entry
                    .get("family")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                quality_tier: String::new(),
                vram_needed_mb: entry.get("vram_mb").and_then(Value::as_u64).unwrap_or(0),
                backend: entry
                    .get("backend")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .into(),
                hardware: entry
                    .get("hardware")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .into(),
                node_id: node_id.into(),
                status: if matches!(
                    metadata.lifecycle,
                    Some(
                        eefn::model_metadata::ModelLifecycle::Ready
                            | eefn::model_metadata::ModelLifecycle::Active
                            | eefn::model_metadata::ModelLifecycle::WarmIdle
                    )
                ) {
                    ModelStatus::Warm
                } else {
                    ModelStatus::Unloaded
                },
                size_bytes: 0,
                model_metadata: Some(metadata),
            });
        }
        let mut state = self.state.write().expect("model lock");
        if let Some(admission) = admission {
            state.request_admission.insert(node_id.into(), admission);
        } else {
            state.request_admission.remove(node_id);
        }
        state.models.retain(|_, specs| {
            specs.retain(|spec| spec.node_id != node_id);
            !specs.is_empty()
        });
        for spec in replacements {
            state
                .models
                .entry(spec.model_id.clone())
                .or_default()
                .push(spec);
        }
        Ok(())
    }

    pub fn remove_node(&self, node_id: &str) {
        let mut state = self.state.write().expect("model lock");
        state.models.retain(|_, specs| {
            specs.retain(|spec| spec.node_id != node_id);
            !specs.is_empty()
        });
        state.loads.remove(node_id);
        state.memory.remove(node_id);
        state.request_admission.remove(node_id);
    }

    /// Includes metrics-only nodes so missed disconnects cannot retain their
    /// scheduler state indefinitely when they advertised no models.
    pub(crate) fn node_ids(&self) -> std::collections::BTreeSet<String> {
        let state = self.state.read().expect("model lock");
        state
            .models
            .values()
            .flatten()
            .map(|spec| spec.node_id.clone())
            .chain(state.loads.keys().cloned())
            .chain(state.request_admission.keys().cloned())
            .collect()
    }

    /// Retain the legacy API parameter for heartbeat collection time.
    /// It supplies no evidence of model speed or network round-trip time.
    pub fn update_metrics(&self, node_id: &str, load: f64, _collection_ms: f64) {
        self.update_heartbeat(node_id, load, None);
    }

    pub(crate) fn update_heartbeat(
        &self,
        node_id: &str,
        load: f64,
        memory: Option<(u64, Instant)>,
    ) {
        let mut state = self.state.write().expect("model lock");
        state.loads.insert(node_id.into(), load.clamp(0.0, 1.0));
        if let Some(memory) = memory {
            state.memory.insert(node_id.into(), memory);
        } else {
            state.memory.remove(node_id);
        }
    }

    pub(crate) fn clear_memory(&self, node_id: &str) {
        self.state
            .write()
            .expect("model lock")
            .memory
            .remove(node_id);
    }

    pub fn list(&self) -> Vec<ModelSpec> {
        self.state
            .read()
            .expect("model lock")
            .models
            .values()
            .flatten()
            .cloned()
            .collect()
    }

    pub fn route(
        &self,
        request: &ModelRequest,
        tiers: &BTreeMap<String, Vec<String>>,
    ) -> Vec<ModelSpec> {
        let state = self.state.read().expect("model lock");
        Self::matching_routes(&state, request, tiers, Instant::now())
            .into_iter()
            .filter(|spec| !Self::gate_full(&state, &spec.node_id))
            .collect()
    }

    fn gate_full(state: &RegistryState, node_id: &str) -> bool {
        state
            .request_admission
            .get(node_id)
            .is_some_and(|a| a.available == 0)
    }

    /// Read-only explanation from one registry snapshot. Counts are node-reported
    /// managed exchange gates, not measured resources or reserved capacity.
    pub(crate) fn preview(
        &self,
        request: &ModelRequest,
        connected: impl Fn(&str) -> bool,
        limit: usize,
    ) -> Value {
        let state = self.state.read().expect("model lock");
        let mut models = Vec::new();
        let mut matched = 0;
        let mut full_models = 0;
        let mut full_nodes = std::collections::BTreeSet::new();
        let now = Instant::now();
        for spec in Self::matching_routes(&state, request, &Default::default(), now) {
            if !connected(&spec.node_id) {
                continue;
            }
            if Self::gate_full(&state, &spec.node_id) {
                full_models += 1;
                full_nodes.insert(spec.node_id);
                continue;
            }
            matched += 1;
            if models.len() < limit {
                let mut candidate = json!({
                    "instance":{"node_id":spec.node_id,"backend":spec.backend,"model_id":spec.model_id},
                    "model_metadata":spec.model_metadata,
                });
                if let Some(hint) = cold_memory_hint(&spec, &state, now) {
                    candidate["cold_memory_hint"] = json!({
                        "status": if hint.exceeds_available() { "estimate_exceeds_available" } else { "estimate_fits" },
                        "estimated_ram_mib": hint.estimated_ram_mib,
                        "available_bytes": hint.available_bytes,
                    });
                }
                models.push(candidate);
            }
        }
        json!({"candidates":models,"matched_count":matched,"truncated":matched>limit,
            "placement_observation":{"schema_version":1,
                "status":if matched>0 {"candidates"} else if full_models>0 {"all_matching_gates_full"} else {"no_matching_model"},
                "full_gate_model_count":full_models,"full_gate_node_count":full_nodes.len(),"capacity_reserved":false}})
    }

    fn matching_routes(
        state: &RegistryState,
        request: &ModelRequest,
        tiers: &BTreeMap<String, Vec<String>>,
        now: Instant,
    ) -> Vec<ModelSpec> {
        let mut chain = Vec::new();
        let exceeds = |spec: &ModelSpec| {
            cold_memory_hint(spec, state, now).is_some_and(ColdMemoryHint::exceeds_available)
        };
        let compare = |left: &ModelSpec, right: &ModelSpec| {
            exceeds(left)
                .cmp(&exceeds(right))
                .then_with(|| model_score(right, state).total_cmp(&model_score(left, state)))
        };
        let ids = if let Some(id) = &request.model_id {
            vec![id.clone()]
        } else {
            request
                .tier
                .as_ref()
                .and_then(|tier| tiers.get(tier))
                .cloned()
                .unwrap_or_default()
        };
        for id in &ids {
            let mut providers = state.models.get(id).cloned().unwrap_or_default();
            providers.retain(|spec| available(spec, request));
            providers.sort_by(&compare);
            chain.extend(providers);
        }
        if request.model_id.is_none() {
            let mut extra = state
                .models
                .values()
                .flatten()
                .filter(|spec| !ids.contains(&spec.model_id) && available(spec, request))
                .cloned()
                .collect::<Vec<_>>();
            extra.sort_by(|left, right| {
                compare(left, right).then_with(|| left.model_id.cmp(&right.model_id))
            });
            chain.extend(extra);
        }
        chain
    }

    pub fn set_status(&self, model_id: &str, node_id: &str, backend: &str, status: ModelStatus) {
        if let Some(spec) = self
            .state
            .write()
            .expect("model lock")
            .models
            .get_mut(model_id)
            .and_then(|values| {
                values
                    .iter_mut()
                    .find(|spec| spec.node_id == node_id && spec.backend == backend)
            })
        {
            spec.status = status;
        }
    }
}

fn available(spec: &ModelSpec, request: &ModelRequest) -> bool {
    if validate_model_constraints(&request.constraints).is_err() {
        return false;
    }
    let capability = match request.modality {
        Modality::Text => "llm.infer",
        Modality::Vlm => "vlm.analyze",
        Modality::Other => return false, // No generic executor is implemented yet.
    };
    let metadata = spec.model_metadata.clone().unwrap_or_else(|| {
        eefn::model_metadata::ModelMetadata::from_legacy(Some(match spec.modality {
            Modality::Text => "text",
            Modality::Vlm => "vlm",
            Modality::Other => "other",
        }))
    });
    if spec.status == ModelStatus::Unavailable
        || !metadata.supports(capability)
        || !metadata.routable()
        || !metadata.input_modalities.iter().any(|m| m == "text")
        || !metadata.output_modalities.iter().any(|m| m == "text")
        || (request.modality == Modality::Vlm
            && !metadata.input_modalities.iter().any(|m| m == "image"))
    {
        return false;
    }
    let Some(constraints) = request.constraints.as_object() else {
        return true;
    };
    if let Some(role) = constraints.get("model_role").and_then(Value::as_str) {
        if !metadata
            .roles
            .as_ref()
            .is_some_and(|roles| roles.iter().any(|value| value == role))
        {
            return false;
        }
    }
    if let Some(backend) = constraints.get("backend") {
        if backend.as_str() != Some(spec.backend.as_str()) {
            return false;
        }
    }
    if constraints
        .get("gpu")
        .and_then(Value::as_str)
        .is_some_and(|gpu| !gpu.is_empty() && spec.hardware != gpu)
    {
        return false;
    }
    if constraints
        .get("vram_min_mb")
        .and_then(Value::as_u64)
        .is_some_and(|minimum| spec.vram_needed_mb != 0 && spec.vram_needed_mb < minimum)
    {
        return false;
    }
    if constraints
        .get("node_id")
        .and_then(Value::as_str)
        .is_some_and(|node| spec.node_id != node)
    {
        return false;
    }
    true
}

fn model_score(spec: &ModelSpec, state: &RegistryState) -> f64 {
    let warm = if spec.status == ModelStatus::Warm {
        1.0
    } else {
        0.0
    };
    let load = 1.0 - state.loads.get(&spec.node_id).copied().unwrap_or(0.0);
    warm * 2.0 + load * 0.4
}

struct ColdMemoryHint {
    estimated_ram_mib: u64,
    available_bytes: u64,
}

impl ColdMemoryHint {
    fn exceeds_available(self) -> bool {
        self.estimated_ram_mib > self.available_bytes / (1024 * 1024)
    }
}

fn cold_memory_hint(
    spec: &ModelSpec,
    state: &RegistryState,
    now: Instant,
) -> Option<ColdMemoryHint> {
    let metadata = spec.model_metadata.as_ref()?;
    if metadata.lifecycle != Some(eefn::model_metadata::ModelLifecycle::Unloaded) {
        return None;
    }
    let estimated_ram_mib = metadata
        .resource_estimates
        .ram_mb
        .filter(|amount| *amount > 0)?;
    let &(available_bytes, expires_at) = state.memory.get(&spec.node_id)?;
    (now < expires_at).then_some(ColdMemoryHint {
        estimated_ram_mib,
        available_bytes,
    })
}

#[derive(Clone)]
pub struct LlmService {
    registry: ModelRegistry,
    tiers: Arc<RwLock<BTreeMap<String, Vec<String>>>>,
    node_server: NodeServer,
    bus: EventBus,
    reviewed_only: bool,
}

impl LlmService {
    pub fn new(
        registry: ModelRegistry,
        tiers: BTreeMap<String, Vec<String>>,
        node_server: NodeServer,
        bus: EventBus,
    ) -> Arc<Self> {
        Self::with_boundary(registry, tiers, node_server, bus, true)
    }
    #[cfg(test)]
    pub(crate) fn legacy_for_test(
        registry: ModelRegistry,
        tiers: BTreeMap<String, Vec<String>>,
        node_server: NodeServer,
        bus: EventBus,
    ) -> Arc<Self> {
        Self::with_boundary(registry, tiers, node_server, bus, false)
    }
    fn with_boundary(
        registry: ModelRegistry,
        tiers: BTreeMap<String, Vec<String>>,
        node_server: NodeServer,
        bus: EventBus,
        reviewed_only: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            tiers: Arc::new(RwLock::new(tiers)),
            node_server,
            bus,
            reviewed_only,
        })
    }

    pub async fn chat(&self, messages: Value, options: ChatOptions) -> Result<String> {
        if self.reviewed_only {
            return Err(eefn::execution_boundary::ExecutionGrantRequired.into());
        }
        validate_model_constraints(&options.constraints)?;
        let modality = if options
            .images
            .as_array()
            .is_some_and(|images| !images.is_empty())
        {
            Modality::Vlm
        } else {
            options.modality.unwrap_or(Modality::Text)
        };
        let request = ModelRequest {
            modality,
            tier: (!options.tier.is_empty()).then_some(options.tier.clone()),
            model_id: options.model_id.clone(),
            constraints: options.constraints.clone(),
        };
        let chain = self
            .registry
            .route(&request, &self.tiers.read().expect("tiers lock"));
        if chain.is_empty() {
            if options.journal.is_some() {
                bail!("No selected model is available for this job")
            }
            return Ok(format!(
                "(model tier '{}' unavailable - connect a node with a selected model)",
                options.tier
            ));
        }
        let mut last = None;
        // Durable jobs get exactly one selected executor per recorded attempt.
        // Interactive legacy chat may still use its existing fallback chain;
        // it is not a durable job/grant or an acknowledged migration.
        let attempts = if options.journal.is_some() {
            1
        } else {
            usize::MAX
        };
        for spec in chain
            .into_iter()
            .filter(|spec| spec.node_id != "local")
            .take(attempts)
        {
            if let Some(journal) = &options.journal {
                journal.assign_model(&spec.node_id, &spec.backend, &spec.model_id)?;
            }
            let result = self.remote_chat(&spec, modality, &messages, &options).await;
            match result {
                Ok(content) if !content.is_empty() => {
                    self.bus.publish("model.used", json!({"node_id": spec.node_id, "model_id": spec.model_id, "tier": options.tier,"request_context":options.request_context})).await;
                    return Ok(content);
                }
                Ok(_) => last = Some(anyhow::anyhow!("empty model response")),
                Err(error) => {
                    warn!(model = %spec.model_id, node = %spec.node_id, %error, "model provider failed");
                    self.registry.set_status(
                        &spec.model_id,
                        &spec.node_id,
                        &spec.backend,
                        ModelStatus::Unavailable,
                    );
                    last = Some(error);
                }
            }
        }
        if let Some(error) = last {
            if options.journal.is_some() {
                return Err(error);
            }
            warn!(%error, "all model providers failed")
        }
        if options.journal.is_some() {
            bail!("No connected node model completed this job")
        }
        Ok(format!(
            "(model tier '{}' unavailable - connect a node with a selected model)",
            options.tier
        ))
    }

    async fn remote_chat(
        &self,
        spec: &ModelSpec,
        modality: Modality,
        messages: &Value,
        options: &ChatOptions,
    ) -> Result<String> {
        let mut messages = messages.clone();
        if !options.system_prompt.is_empty() {
            messages
                .as_array_mut()
                .context("messages must be an array")?
                .insert(
                    0,
                    json!({"role": "system", "content": options.system_prompt}),
                );
        }
        let response = self.node_server.invoke_remote_with_context(&spec.node_id, if modality == Modality::Vlm { "vlm.analyze" } else { "llm.infer" }, "run", json!({
            "model": spec.model_id, "backend":spec.backend, "messages": messages, "images": options.images, "max_tokens": options.max_tokens, "temperature": options.temperature,
        }), Duration::from_secs(60),options.request_context.as_ref()).await?;
        if response.get("success").and_then(Value::as_bool) != Some(true) {
            bail!(
                "{}",
                response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("remote model failed")
            )
        }
        response
            .pointer("/data/content")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("remote model response has no content")
    }
}

#[derive(Clone, Debug)]
pub struct ChatOptions {
    pub journal: Option<crate::jobs::AttemptJournal>,
    pub request_context: Option<eefn::context::RequestContext>,
    pub tier: String,
    pub model_id: Option<String>,
    pub modality: Option<Modality>,
    pub constraints: Value,
    pub system_prompt: String,
    pub temperature: f64,
    pub max_tokens: u64,
    pub images: Value,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            journal: None,
            request_context: None,
            tier: "fast".into(),
            model_id: None,
            modality: None,
            constraints: json!({}),
            system_prompt: String::new(),
            temperature: 0.7,
            max_tokens: 1024,
            images: json!([]),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn route_preview_distinguishes_full_gates_from_missing_models_without_reserving() {
        let registry = ModelRegistry::default();
        let model = |id| json!({"model_id":id,"backend":"ollama","modality":"text"});
        let full = json!({"schema_version":1,"limit":1,"in_flight":1,"available":0,
            "queueing":false,"provider_execution_observed":false});
        registry
            .register_snapshot("a", &[model("chosen"), model("spare")], Some(&full))
            .unwrap();
        registry.register_remote("b", &[model("chosen")]).unwrap();
        registry
            .register_snapshot("offline", &[model("chosen")], Some(&full))
            .unwrap();
        let request = text_request();
        let connected = |id: &str| id != "offline";
        let original = json!(registry.list());
        let report = registry.preview(&request, connected, 1);
        assert_eq!(report["matched_count"], 1);
        assert_eq!(report["candidates"][0]["instance"]["node_id"], "b");
        assert_eq!(
            report["placement_observation"],
            json!({"schema_version":1,"status":"candidates",
            "full_gate_model_count":2,"full_gate_node_count":1,"capacity_reserved":false})
        );
        assert_eq!(report["truncated"], false);

        let exact = ModelRequest {
            model_id: Some("chosen".into()),
            ..request.clone()
        };
        assert_eq!(
            registry.preview(&exact, connected, 1)["placement_observation"]["full_gate_model_count"],
            1
        );
        for constraints in [
            json!({"backend":"llamacpp"}),
            json!({"model_role":"absent"}),
            json!({"node_id":"offline"}),
        ] {
            let filtered = ModelRequest {
                constraints,
                ..request.clone()
            };
            let report = registry.preview(&filtered, connected, 1);
            assert_eq!(report["matched_count"], 0);
            assert_eq!(
                report["placement_observation"]["status"],
                "no_matching_model"
            );
            assert_eq!(report["placement_observation"]["full_gate_node_count"], 0);
        }
        let disconnected = registry.preview(&request, |_| false, 1);
        assert_eq!(
            disconnected["placement_observation"]["status"],
            "no_matching_model"
        );
        assert_eq!(
            disconnected["placement_observation"]["full_gate_model_count"],
            0
        );
        assert_eq!(
            json!(registry.list()),
            original,
            "Inspection must not alter model state"
        );
        assert_eq!(registry.route(&exact, &Default::default())[0].node_id, "b");

        registry
            .register_snapshot("b", &[model("chosen")], Some(&full))
            .unwrap();
        let busy = registry.preview(&request, connected, 1);
        assert_eq!(busy["matched_count"], 0);
        assert_eq!(busy["candidates"], json!([]));
        assert_eq!(
            busy["placement_observation"],
            json!({"schema_version":1,"status":"all_matching_gates_full",
            "full_gate_model_count":3,"full_gate_node_count":2,"capacity_reserved":false})
        );
        assert!(
            registry
                .route(&request, &Default::default())
                .iter()
                .all(|s| !connected(&s.node_id))
        );

        // Absent legacy capacity is unknown; it neither retains the previous full
        // observation nor establishes actual free provider resources.
        registry
            .register_remote("b", &[model("chosen"), model("spare")])
            .unwrap();
        let legacy = registry.preview(&request, connected, 1);
        assert_eq!(legacy["matched_count"], 2);
        assert_eq!(legacy["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(legacy["truncated"], true);
        assert_eq!(legacy["placement_observation"]["status"], "candidates");
        assert!(legacy.get("resources_available").is_none());
    }
    #[test]
    fn request_admission_filters_only_new_routes_and_updates_atomically_with_models() {
        use super::*;
        let registry = ModelRegistry::default();
        let model = json!({"model_id":"selected","backend":"ollama","modality":"text"});
        let full = json!({"schema_version":1,"limit":1,"in_flight":1,"available":0,"queueing":false,"provider_execution_observed":false});
        let mut free = full.clone();
        free["in_flight"] = json!(0);
        free["available"] = json!(1);
        let request = ModelRequest {
            modality: Modality::Text,
            tier: None,
            model_id: Some("selected".into()),
            constraints: json!({}),
        };
        registry.register_remote("a", &[model.clone()]).unwrap();
        registry.register_remote("b", &[model.clone()]).unwrap();
        let list = || {
            let mut rows = registry.list();
            rows.sort_by(|a, b| a.node_id.cmp(&b.node_id));
            json!(rows)
        };
        let original = list();
        registry
            .register_snapshot("a", &[model.clone()], Some(&full))
            .unwrap();
        assert_eq!(
            list(),
            original,
            "Capacity must not alter lifecycle, capabilities or model evidence"
        );
        assert_eq!(
            registry
                .route(&request, &Default::default())
                .iter()
                .map(|s| s.node_id.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        registry
            .register_snapshot("b", &[model.clone()], Some(&full))
            .unwrap();
        assert!(registry.route(&request, &Default::default()).is_empty());
        for (key, value) in [
            ("schema_version", json!(2)),
            ("limit", json!(0)),
            ("available", json!(1)),
            ("in_flight", json!(u32::MAX)),
            ("queueing", json!(true)),
            ("provider_execution_observed", json!(true)),
            ("extra", json!(true)),
        ] {
            let mut forged = full.clone();
            forged[key] = value;
            assert!(registry.register_snapshot("a", &[], Some(&forged)).is_err());
            assert_eq!(
                list(),
                original,
                "Invalid hint must not replace an existing model snapshot"
            );
            assert!(registry.route(&request, &Default::default()).is_empty());
        }
        let mut overflow = full.clone();
        overflow["in_flight"] = json!(u32::MAX);
        overflow["available"] = json!(2);
        assert!(
            registry
                .register_snapshot("a", &[], Some(&overflow))
                .is_err(),
            "Wrapping counts must not imitate a valid limit"
        );
        assert_eq!(list(), original);
        registry
            .register_snapshot("a", &[model.clone()], Some(&free))
            .unwrap();
        assert_eq!(
            registry.route(&request, &Default::default())[0].node_id,
            "a"
        );
        registry.register_remote("b", &[model.clone()]).unwrap();
        assert_eq!(
            registry.route(&request, &Default::default()).len(),
            2,
            "Missing legacy hints are unknown, not a retained full gate"
        );
        registry.remove_node("a");
        registry.register_remote("a", &[model]).unwrap();
        assert_eq!(registry.route(&request, &Default::default()).len(), 2);
    }
    #[test]
    fn resident_metadata_warms_existing_score_without_inventing_legacy_readiness() {
        use super::*;
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.availability = Some(eefn::model_metadata::ModelAvailability::Available);
        for (node, state) in [
            ("cold", eefn::model_metadata::ModelLifecycle::Unloaded),
            ("resident", eefn::model_metadata::ModelLifecycle::WarmIdle),
        ] {
            metadata.lifecycle = Some(state);
            registry.register_remote(node,&[json!({"model_id":"m","backend":"llamacpp","modality":"text","model_metadata":metadata})]).unwrap();
        }
        registry.update_metrics("resident", 1.0, 1000.0);
        registry.update_metrics("cold", 0.0, 0.0);
        let request = ModelRequest {
            modality: Modality::Text,
            tier: None,
            model_id: Some("m".into()),
            constraints: json!({}),
        };
        let ordered = registry.route(&request, &BTreeMap::new());
        assert_eq!(ordered[0].node_id, "resident");
        assert_eq!(ordered[0].status, ModelStatus::Warm);
        assert_eq!(ordered[1].status, ModelStatus::Unloaded);
        registry
            .register_remote(
                "legacy",
                &[json!({"model_id":"old","backend":"ollama","modality":"text"})],
            )
            .unwrap();
        assert_eq!(
            registry
                .list()
                .iter()
                .find(|m| m.node_id == "legacy")
                .unwrap()
                .status,
            ModelStatus::Unloaded
        );
    }
    use super::*;

    #[tokio::test]
    async fn journaled_inference_records_backend_and_never_falls_back_inside_one_attempt() {
        use crate::task::{Goal, PlanStatus, TaskPlanner, TaskStatus};
        let store = crate::jobs::JobStore::in_memory().unwrap();
        let job = TaskPlanner
            .plan(Goal {
                request_context: None,
                description: "assignment fixture".into(),
                template: "generate_text".into(),
                params: json!({"prompt":"fixture"}),
                constraints: json!({}),
            })
            .unwrap();
        let id = job.plan_id.clone();
        let task_id = job.tasks[0].task_id.clone();
        store.insert(job).unwrap();
        store
            .change(&id, "fixture attempt", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Running;
                p.tasks[0].attempts = 1;
                Ok(())
            })
            .unwrap();
        let registry = ModelRegistry::default();
        for (node, backend, status) in [
            ("first", "ollama", ModelStatus::Warm),
            ("next", "llamacpp", ModelStatus::Unloaded),
        ] {
            registry.register(ModelSpec {
                model_id: "same-id".into(),
                modality: Modality::Text,
                family: String::new(),
                quality_tier: String::new(),
                vram_needed_mb: 0,
                backend: backend.into(),
                hardware: "cpu".into(),
                node_id: node.into(),
                status,
                size_bytes: 0,
                model_metadata: None,
            });
        }
        // No sockets/models are started. Offline first provider must leave the
        // second untouched, rather than assigning both within the same attempt.
        let service = LlmService::legacy_for_test(
            registry.clone(),
            BTreeMap::new(),
            NodeServer::new("fixture-secret", "127.0.0.1", 0).unwrap(),
            EventBus::new(20),
        );
        let journal = crate::jobs::AttemptJournal {
            store: store.clone(),
            job_id: id.clone(),
            task_id,
            attempt: 1,
        };
        let options = ChatOptions {
            journal: Some(journal.clone()),
            ..Default::default()
        };
        assert!(
            service
                .chat(
                    json!([{ "role":"user","content":"fixture"}]),
                    options.clone()
                )
                .await
                .is_err()
        );
        let saved = store.get(&id).unwrap();
        let assignments = &saved.tasks[0].assignments;
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].node_id, "first");
        assert_eq!(assignments[0].backend.as_deref(), Some("ollama"));
        assert_eq!(
            registry
                .list()
                .iter()
                .find(|s| s.node_id == "next")
                .unwrap()
                .status,
            ModelStatus::Unloaded
        );
        let before = json!(saved);
        assert!(
            service
                .chat(json!([]), options)
                .await
                .unwrap_err()
                .to_string()
                .contains("already assigned")
        );
        assert_eq!(json!(store.get(&id).unwrap()), before);
        assert_eq!(
            registry
                .list()
                .iter()
                .find(|s| s.node_id == "next")
                .unwrap()
                .status,
            ModelStatus::Unloaded
        );
        store
            .change(&id, "fixture next attempt", |p| {
                p.tasks[0].attempts = 2;
                Ok(())
            })
            .unwrap();
        assert!(
            service
                .chat(
                    json!([]),
                    ChatOptions {
                        journal: Some(crate::jobs::AttemptJournal {
                            attempt: 2,
                            ..journal
                        }),
                        ..Default::default()
                    }
                )
                .await
                .is_err()
        );
        let saved = store.get(&id).unwrap();
        assert_eq!(saved.tasks[0].assignments.len(), 2);
        assert_eq!(saved.tasks[0].assignments[1].node_id, "next");
        assert_eq!(
            saved.tasks[0].assignments[1].backend.as_deref(),
            Some("llamacpp")
        );
    }

    #[test]
    fn role_and_backend_constraints_never_fall_back_to_unlabelled_models() {
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.roles = Some(vec!["request_interpreter".into()]);
        registry
            .register_remote(
                "a",
                &[
                    json!({"model_id":"fast-legacy","backend":"ollama","modality":"text"}),
                    json!({"model_id":"labelled","backend":"ollama","model_metadata":metadata}),
                    json!({"model_id":"labelled","backend":"llamacpp","model_metadata":metadata}),
                ],
            )
            .unwrap();
        let mut request = text_request();
        request.tier = Some("fast".into());
        request.constraints = json!({"model_role":"request_interpreter","backend":"ollama"});
        let tiers = BTreeMap::from([("fast".into(), vec!["fast-legacy".into()])]);
        let route = registry.route(&request, &tiers);
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].model_id, "labelled");
        assert_eq!(route[0].backend, "ollama");
        request.constraints["model_role"] = json!("missing");
        assert!(registry.route(&request, &tiers).is_empty());
        request.constraints["model_role"] = json!(null);
        assert!(validate_model_constraints(&request.constraints).is_err());
        assert!(registry.route(&request, &tiers).is_empty());
    }

    #[test]
    fn route_queries_are_bounded_and_reject_unimplemented_capabilities() {
        for value in [
            json!({"capability":"ocr"}),
            json!({"capability":"llm.infer","role":"*"}),
            json!({"capability":"llm.infer","limit":33}),
            json!({"capability":"llm.infer","backend":"bad backend"}),
        ] {
            assert!(
                serde_json::from_value::<RouteQuery>(value)
                    .unwrap()
                    .request()
                    .is_err()
            );
        }
        assert!(
            serde_json::from_value::<RouteQuery>(json!({"capability":"llm.infer","grant":true}))
                .is_err()
        );
        let query: RouteQuery =
            serde_json::from_value(json!({"capability":"llm.infer","role":"request_interpreter"}))
                .unwrap();
        assert_eq!(
            query.request().unwrap().constraints["model_role"],
            "request_interpreter"
        );
    }

    fn text_request() -> ModelRequest {
        ModelRequest {
            modality: Modality::Text,
            tier: None,
            model_id: None,
            constraints: json!({}),
        }
    }

    #[test]
    fn explicit_capabilities_override_legacy_and_unknown_models_remain_visible() {
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("vlm"));
        metadata.capabilities = vec!["vlm.analyze".into(), "ocr".into()];
        registry.register_remote("node", &[
            json!({"model_id":"multi","backend":"ollama","modality":"text","model_metadata":metadata}),
            json!({"model_id":"legacy","backend":"ollama","modality":"text"}),
            json!({"model_id":"unknown","backend":"future"}),
        ]).unwrap();
        assert_eq!(registry.list().len(), 3);
        assert_eq!(
            registry
                .route(&text_request(), &BTreeMap::new())
                .iter()
                .map(|s| s.model_id.as_str())
                .collect::<Vec<_>>(),
            vec!["legacy"]
        );
        let vision = ModelRequest {
            modality: Modality::Vlm,
            ..text_request()
        };
        assert_eq!(
            registry.route(&vision, &BTreeMap::new())[0].model_id,
            "multi"
        );
    }

    #[test]
    fn replacement_removes_stale_models_and_invalid_snapshot_is_atomic() {
        let registry = ModelRegistry::default();
        let m = json!({"model_id":"m","backend":"ollama","modality":"text"});
        registry.register_remote("a", &[m.clone()]).unwrap();
        registry.register_remote("b", &[m.clone()]).unwrap();
        assert!(
            registry
                .register_remote("a", &[m.clone(), m.clone()])
                .is_err()
        );
        assert_eq!(registry.list().len(), 2);
        let mut invalid = m.clone();
        invalid["model_metadata"] = json!({"schema_version":99});
        assert!(registry.register_remote("a", &[invalid]).is_err());
        assert_eq!(registry.list().len(), 2);
        registry.register_remote("a", &[]).unwrap();
        assert_eq!(registry.list().len(), 1);
        assert_eq!(registry.list()[0].node_id, "b");
    }

    #[test]
    fn unavailable_reports_and_backend_specific_failures_are_not_routed() {
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.lifecycle = Some(eefn::model_metadata::ModelLifecycle::Error);
        registry
            .register_remote(
                "a",
                &[
                    json!({"model_id":"m","backend":"ollama","modality":"text"}),
                    json!({"model_id":"m","backend":"llamacpp","modality":"text"}),
                    json!({"model_id":"broken","backend":"ollama","model_metadata":metadata}),
                ],
            )
            .unwrap();
        registry.set_status("m", "a", "ollama", ModelStatus::Unavailable);
        let route = registry.route(&text_request(), &BTreeMap::new());
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].backend, "llamacpp");
    }

    #[test]
    fn declared_io_modalities_must_support_the_chat_contract() {
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("vlm"));
        metadata.input_modalities = vec!["text".into()];
        registry
            .register_remote("a", &[json!({"model_id":"m","model_metadata":metadata})])
            .unwrap();
        assert_eq!(registry.route(&text_request(), &BTreeMap::new()).len(), 1);
        assert!(
            registry
                .route(
                    &ModelRequest {
                        modality: Modality::Vlm,
                        ..text_request()
                    },
                    &BTreeMap::new()
                )
                .is_empty()
        );
        metadata.output_modalities = vec!["audio".into()];
        registry
            .register_remote("a", &[json!({"model_id":"m","model_metadata":metadata})])
            .unwrap();
        assert!(registry.route(&text_request(), &BTreeMap::new()).is_empty());
    }

    #[test]
    fn routing_uses_load_without_treating_heartbeat_collection_time_as_latency() {
        let registry = ModelRegistry::default();
        for node in ["busy", "idle"] {
            registry.register(ModelSpec {
                model_id: "model".into(),
                modality: Modality::Text,
                family: "m".into(),
                quality_tier: String::new(),
                vram_needed_mb: 0,
                backend: "llamacpp".into(),
                hardware: "cpu".into(),
                node_id: node.into(),
                status: ModelStatus::Unloaded,
                size_bytes: 0,
                model_metadata: None,
            });
        }
        registry.update_metrics("busy", 0.9, 1.0);
        registry.update_metrics("idle", 0.1, 1000.0);
        let route = registry.route(
            &ModelRequest {
                modality: Modality::Text,
                tier: None,
                model_id: Some("model".into()),
                constraints: json!({}),
            },
            &BTreeMap::new(),
        );
        assert_eq!(route[0].node_id, "idle");
        let mut vision = route[0].clone();
        vision.modality = Modality::Vlm;
        let text_request = ModelRequest {
            modality: Modality::Text,
            tier: None,
            model_id: None,
            constraints: json!({}),
        };
        assert!(available(&vision, &text_request));
        let vision_request = ModelRequest {
            modality: Modality::Vlm,
            ..text_request
        };
        assert!(!available(&route[0], &vision_request));
    }

    #[test]
    fn fresh_host_memory_demotes_only_cold_routes_with_insufficient_estimated_ram() {
        let registry = ModelRegistry::default();
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.lifecycle = Some(eefn::model_metadata::ModelLifecycle::Unloaded);
        metadata.resource_estimates.ram_mb = Some(2048);
        for node in ["quiet", "busy"] {
            registry
                .register_remote(
                    node,
                    &[json!({"model_id":"shared","backend":"llamacpp","model_metadata":metadata})],
                )
                .unwrap();
        }
        let available = 2048 * 1024 * 1024u64;
        let now = Instant::now();
        registry.update_heartbeat(
            "quiet",
            0.1,
            Some((available - 1, now + Duration::from_secs(60))),
        );
        registry.update_heartbeat(
            "busy",
            0.8,
            Some((available, now + Duration::from_secs(60))),
        );
        let mut request = text_request();
        request.model_id = Some("shared".into());
        let route = |request: &ModelRequest| registry.route(request, &Default::default());
        assert_eq!(route(&request)[0].node_id, "busy");
        let preview = registry.preview(&request, |_| true, 8);
        assert_eq!(
            preview["candidates"][0]["cold_memory_hint"]["status"],
            "estimate_fits"
        );
        assert_eq!(
            preview["candidates"][1]["cold_memory_hint"]["status"],
            "estimate_exceeds_available"
        );
        let exact = ModelRequest {
            constraints: json!({"node_id":"quiet"}),
            ..request.clone()
        };
        assert_eq!(route(&exact)[0].node_id, "quiet");

        registry.update_heartbeat("quiet", 0.1, Some((0, now)));
        assert_eq!(route(&request)[0].node_id, "quiet");
        assert!(
            registry.preview(&request, |_| true, 8)["candidates"][0]
                .get("cold_memory_hint")
                .is_none()
        );

        metadata.lifecycle = Some(eefn::model_metadata::ModelLifecycle::WarmIdle);
        registry
            .register_remote(
                "quiet",
                &[json!({"model_id":"shared","backend":"llamacpp","model_metadata":metadata})],
            )
            .unwrap();
        registry.update_heartbeat("quiet", 0.1, Some((0, now + Duration::from_secs(60))));
        assert_eq!(route(&request)[0].node_id, "quiet");
        assert!(
            registry.preview(&request, |_| true, 8)["candidates"][0]
                .get("cold_memory_hint")
                .is_none()
        );
    }
}
