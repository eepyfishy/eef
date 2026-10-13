//! Resolve whole finite graphs using the existing registry/ranking and immutable
//! source. No execution, inferred stage identities, provider fallback or grant.
use crate::{
    capability::CapabilityRegistry,
    model::{Modality, ModelRegistry, ModelRequest},
};
use anyhow::{Context, Result, bail};
use eefn::{network::PeerRecord, pipeline::FinitePipeline};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn definition(params: &Value, node: Option<&str>) -> Result<FinitePipeline> {
    if params
        .as_object()
        .is_none_or(|p| p.len() != 1 || !p.contains_key("pipeline"))
    {
        bail!("Expected one exact pipeline definition")
    }
    let definition: FinitePipeline = serde_json::from_value(params["pipeline"].clone())?;
    definition.validate_for_executor(node)?;
    Ok(definition)
}
pub(crate) fn validate_peer(
    definition: &FinitePipeline,
    peer: &PeerRecord,
    registry: &CapabilityRegistry,
    models: &ModelRegistry,
    context: Option<&eefn::context::RequestContext>,
) -> Result<()> {
    definition.validate_for_executor(Some(&peer.node_id))?;
    if !peer.capabilities.iter().any(|c| c == "pipeline.run") {
        bail!("Node does not advertise finite pipeline support")
    }
    let one = BTreeSet::from([peer.node_id.clone()]);
    for step in &definition.steps {
        if !peer.capabilities.contains(&step.capability) {
            bail!("Pipeline stage capability is withdrawn")
        }
        let mut constraints = json!({"node_id":peer.node_id});
        if let Some(model) = &step.model_id {
            constraints["backend"] = json!(step.backend);
            if let Some(role) = &step.model_role {
                constraints["model_role"] = json!(role);
            }
            let candidates = models.route(
                &ModelRequest {
                    modality: if step.capability == "vlm.analyze" {
                        Modality::Vlm
                    } else {
                        Modality::Text
                    },
                    tier: None,
                    model_id: Some(model.clone()),
                    constraints,
                },
                &Default::default(),
            );
            if !candidates.iter().any(|m| {
                m.node_id == peer.node_id
                    && peer
                        .models
                        .iter()
                        .any(|p| crate::request_planning::current_model_matches(p, m))
            }) {
                bail!("Exact pipeline model/backend/role is not currently registered and available")
            }
        } else {
            if let Some(resource) = &step.resource_id {
                constraints["resource_id"] = json!(resource);
            }
            let provider = registry
                .find_best_scoped(
                    &step.capability,
                    &step.action,
                    &constraints,
                    context,
                    Some(&one),
                )
                .context("Pipeline stage provider is unavailable on this executor")?;
            if provider.properties["selected_resource_id"].as_str() != step.resource_id.as_deref() {
                bail!(
                    "Pipeline resource-backed steps require an exact declared resource; no implicit resource substitution"
                )
            }
            validate_literals(&provider.properties, &step.bound_params()?, &step.inputs)?;
        }
    }
    Ok(())
}
/// Binding fields are explicitly deferred data, not fabricated placeholder
/// values. Their types/budgets are checked after local resolution by the runtime.
fn validate_literals(
    properties: &Value,
    params: &Value,
    inputs: &[eefn::pipeline::DataInput],
) -> Result<()> {
    let mut properties = properties.clone();
    if let Some(schema) = properties.get_mut("params").and_then(Value::as_object_mut) {
        for input in inputs {
            schema.remove(&input.parameter);
        }
    }
    CapabilityRegistry::validate_properties_params(&properties, params)
}
pub(crate) async fn select(
    params: &Value,
    constraints: &Value,
    context: Option<&eefn::context::RequestContext>,
    registry: &CapabilityRegistry,
    models: &ModelRegistry,
    server: &eefn::NodeServer,
    permitted: Option<&BTreeSet<String>>,
) -> Result<String> {
    let definition = definition(params, None)?;
    let mut suitable = BTreeSet::new();
    let candidates = registry.find("pipeline.run", "run");
    if candidates.len() > 256 {
        bail!("Too many pipeline candidates; constrain the route")
    }
    for provider in candidates {
        if permitted.is_some_and(|allowed| !allowed.contains(&provider.node_id)) {
            continue;
        }
        let Ok(peer) = crate::request_planning::live_peer(
            server,
            &provider.node_id,
            "pipeline.run",
            registry.scheduling_policy().freshness_seconds,
        )
        .await
        else {
            continue;
        };
        if validate_peer(&definition, &peer, registry, models, context).is_ok() {
            suitable.insert(provider.node_id);
        }
    }
    registry
        .find_best_scoped("pipeline.run", "run", constraints, context, Some(&suitable))
        .map(|provider| provider.node_id)
        .context("No disclosed current executor supports every exact pipeline stage")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityProvider;
    fn fixture() -> (
        FinitePipeline,
        PeerRecord,
        CapabilityRegistry,
        ModelRegistry,
    ) {
        let definition=definition(&json!({"pipeline":{"schema_version":1,"max_retained_bytes":1024,"steps":[
            {"id":"read","capability":"filesystem","action":"read","params":{"path":"fixed.txt"}},
            {"id":"infer","capability":"llm.infer","action":"run","model_id":"selected","backend":"ollama","params":{},
                "inputs":[{"parameter":"prompt","source_step":"read","pointer":"/content"}]}
        ]}}),None).unwrap();
        let raw = json!({"node_id":"node-a","capabilities":["pipeline.run","filesystem","llm.infer"],
            "models":[{"model_id":"selected","backend":"ollama","modality":"text"}]});
        let peer = PeerRecord::from_registration(&raw).unwrap();
        let models = ModelRegistry::default();
        models
            .register_remote("node-a", raw["models"].as_array().unwrap())
            .unwrap();
        let registry = CapabilityRegistry::default();
        let mut provider = CapabilityProvider::local("filesystem", "*", json!({}));
        provider.node_id = "node-a".into();
        registry.register(provider);
        (definition, peer, registry, models)
    }
    #[test]
    fn whole_graph_needs_exact_current_capabilities_models_roles_and_resources() {
        let (definition, peer, registry, models) = fixture();
        validate_peer(&definition, &peer, &registry, &models, None).unwrap();
        let mut withdrawn = peer.clone();
        withdrawn.capabilities.retain(|c| c != "filesystem");
        assert!(validate_peer(&definition, &withdrawn, &registry, &models, None).is_err());
        let mut absent = peer.clone();
        absent.models.clear();
        assert!(validate_peer(&definition, &absent, &registry, &models, None).is_err());
        let mut other = definition.clone();
        other.steps[1].backend = Some("llamacpp".into());
        assert!(validate_peer(&other, &peer, &registry, &models, None).is_err());
        let mut other = definition.clone();
        other.steps[1].model_role = Some("request_interpreter".into());
        assert!(validate_peer(&other, &peer, &registry, &models, None).is_err());
        let mut other = definition.clone();
        other.steps[0].resource_id = Some("node-b::files".into());
        assert!(validate_peer(&other, &peer, &registry, &models, None).is_err());
    }
    #[test]
    fn only_bound_data_fields_defer_provider_type_validation() {
        let input: eefn::pipeline::DataInput = serde_json::from_value(
            json!({"parameter":"content","source_step":"read","pointer":"/content"}),
        )
        .unwrap();
        let properties = json!({"params":{"path":{"type":"string","required":true},"content":{"type":"string","required":true}}});
        assert!(validate_literals(&properties, &json!({"path":"fixed"}), &[]).is_err());
        validate_literals(&properties, &json!({"path":"fixed"}), &[input.clone()]).unwrap();
        assert!(validate_literals(&properties, &json!({"path":false}), &[input]).is_err());
    }
}
