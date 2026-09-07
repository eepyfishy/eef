use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use std::time::Duration;

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

#[derive(Default)]
struct RegistryState {
    models: BTreeMap<String, Vec<ModelSpec>>,
    loads: HashMap<String, f64>,
    latencies: HashMap<String, f64>,
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

    pub fn register_remote(&self, node_id: &str, models: &[Value]) {
        for entry in models {
            let Some(model_id) = entry
                .get("model_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let modality = match entry.get("modality").and_then(Value::as_str) {
                Some("vlm") => Modality::Vlm,
                Some("text") => Modality::Text,
                _ => continue,
            };
            self.register(ModelSpec {
                model_id: model_id.into(),
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
                status: ModelStatus::Unloaded,
                size_bytes: 0,
            });
        }
    }

    pub fn remove_node(&self, node_id: &str) {
        let mut state = self.state.write().expect("model lock");
        state.models.retain(|_, specs| {
            specs.retain(|spec| spec.node_id != node_id);
            !specs.is_empty()
        });
        state.loads.remove(node_id);
        state.latencies.remove(node_id);
    }

    pub fn update_metrics(&self, node_id: &str, load: f64, latency: f64) {
        let mut state = self.state.write().expect("model lock");
        state.loads.insert(node_id.into(), load.clamp(0.0, 1.0));
        state.latencies.insert(node_id.into(), latency.max(0.0));
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
        let mut chain = Vec::new();
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
            providers.sort_by(|left, right| {
                model_score(right, &state).total_cmp(&model_score(left, &state))
            });
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
                model_score(right, &state)
                    .total_cmp(&model_score(left, &state))
                    .then_with(|| left.model_id.cmp(&right.model_id))
            });
            chain.extend(extra);
        }
        chain
    }

    pub fn set_status(&self, model_id: &str, node_id: &str, status: ModelStatus) {
        if let Some(spec) = self
            .state
            .write()
            .expect("model lock")
            .models
            .get_mut(model_id)
            .and_then(|values| values.iter_mut().find(|spec| spec.node_id == node_id))
        {
            spec.status = status;
        }
    }
}

fn available(spec: &ModelSpec, request: &ModelRequest) -> bool {
    if spec.status == ModelStatus::Unavailable
        || (spec.modality != request.modality
            && !(spec.modality == Modality::Vlm && request.modality == Modality::Text))
    {
        return false;
    }
    let Some(constraints) = request.constraints.as_object() else {
        return true;
    };
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
    let latency = 1.0 / (1.0 + state.latencies.get(&spec.node_id).copied().unwrap_or(0.0) / 100.0);
    warm * 2.0 + load * 0.4 + latency * 0.6
}

#[derive(Clone)]
pub struct LlmService {
    registry: ModelRegistry,
    tiers: Arc<RwLock<BTreeMap<String, Vec<String>>>>,
    node_server: NodeServer,
    bus: EventBus,
}

impl LlmService {
    pub fn new(
        registry: ModelRegistry,
        tiers: BTreeMap<String, Vec<String>>,
        node_server: NodeServer,
        bus: EventBus,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            tiers: Arc::new(RwLock::new(tiers)),
            node_server,
            bus,
        })
    }

    pub async fn chat(&self, messages: Value, options: ChatOptions) -> Result<String> {
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
        for spec in chain.into_iter().filter(|spec| spec.node_id != "local") {
            let result = self.remote_chat(&spec, &messages, &options).await;
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
        messages: &Value,
        options: &ChatOptions,
    ) -> Result<String> {
        let mut messages = messages.clone();
        if let Some(journal) = &options.journal {
            journal.assign(&spec.node_id, None, Some(&spec.model_id))?;
        }
        if !options.system_prompt.is_empty() {
            messages
                .as_array_mut()
                .context("messages must be an array")?
                .insert(
                    0,
                    json!({"role": "system", "content": options.system_prompt}),
                );
        }
        let response = self.node_server.invoke_remote_with_context(&spec.node_id, if spec.modality == Modality::Vlm { "vlm.analyze" } else { "llm.infer" }, "run", json!({
            "model": spec.model_id, "messages": messages, "images": options.images, "max_tokens": options.max_tokens, "temperature": options.temperature,
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
    use super::*;

    #[test]
    fn load_routing_uses_live_node_metrics() {
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
            });
        }
        registry.update_metrics("busy", 0.9, 0.0);
        registry.update_metrics("idle", 0.1, 0.0);
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
}
