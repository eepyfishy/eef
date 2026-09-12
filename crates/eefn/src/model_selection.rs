//! Owner model-selection commands over the existing backend configuration.
use crate::{ModelSlot, SelectedModel, model_metadata::ModelMetadata};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<Vec<String>>,
}

impl ModelHints {
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_none() && self.roles.is_none()
    }
    pub fn metadata(&self, modality: &str) -> Result<ModelMetadata> {
        let supported = ModelMetadata::from_legacy(Some(modality));
        let mut value = supported.clone();
        if let Some(capabilities) = &self.capabilities {
            if capabilities.iter().any(|cap| !supported.supports(cap)) {
                bail!("selected capabilities are not supported by this model adapter/type")
            }
            value.capabilities = capabilities.clone();
        }
        value.roles = self.roles.clone();
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Ollama,
    Llamacpp,
}
impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Llamacpp => "llamacpp",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Ollama => "selected",
            Self::Llamacpp => "slots",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelCommand {
    Show {},
    SelectOllama {
        model: SelectedModel,
    },
    SelectGguf {
        slot: ModelSlot,
    },
    Remove {
        backend: Backend,
        model_id: String,
    },
    Hints {
        backend: Backend,
        model_id: String,
        capabilities: Option<Vec<String>>,
        roles: Option<Vec<String>>,
    },
    Provider {
        provider: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCommandRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ModelCommand,
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 256 || id.trim() != id || id.chars().any(char::is_control) {
        bail!(
            "model ID must contain 1-256 bytes without surrounding whitespace or control characters"
        )
    }
    Ok(())
}

fn entries(config: &mut Value, backend: Backend) -> Result<&mut Vec<Value>> {
    let root = config
        .as_object_mut()
        .context("configuration must be an object")?;
    let models = root
        .entry("models")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("models must be an object")?;
    let owner = models
        .entry(backend.name())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("model backend must be an object")?;
    owner
        .entry(backend.key())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("selected models must be a list")
}

/// Changes only the requested selection fields. Caller holds the configuration lock.
pub fn apply(config: &mut Value, command: &ModelCommand) -> Result<bool> {
    let before = config.clone();
    match command {
        ModelCommand::Show {} => {}
        ModelCommand::Provider { provider } => {
            if !matches!(provider.as_str(), "auto" | "ollama" | "llamacpp") {
                bail!("provider must be auto, ollama, or llamacpp")
            }
            let models = config
                .as_object_mut()
                .context("configuration must be an object")?
                .entry("models")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .context("models must be an object")?;
            models.insert("provider".into(), json!(provider));
        }
        ModelCommand::SelectOllama { model } => {
            model.validate()?;
            upsert(
                entries(config, Backend::Ollama)?,
                &model.model_id,
                serde_json::to_value(model)?,
            )?;
        }
        ModelCommand::SelectGguf { slot } => {
            let mut slot = slot.clone();
            validate_id(&slot.model_id)?;
            slot.selection_metadata()?;
            if !slot.model_path.is_absolute() || !slot.model_path.is_file() {
                bail!("select GGUF using an absolute existing model file")
            }
            if let Some(path) = &slot.mmproj_path {
                if !path.is_absolute() || !path.is_file() {
                    bail!("projector must be an absolute existing file")
                }
            }
            if slot.port == 0 {
                let slots = entries(config, Backend::Llamacpp)?;
                slot.port = slots
                    .iter()
                    .find(|m| m["model_id"] == slot.model_id)
                    .and_then(|m| m["port"].as_u64())
                    .and_then(|p| u16::try_from(p).ok())
                    .filter(|p| *p > 0)
                    .unwrap_or(0);
                for _ in 0..16 {
                    if slot.port != 0 {
                        break;
                    }
                    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
                    let candidate = listener.local_addr()?.port();
                    if !slots
                        .iter()
                        .any(|m| m["port"].as_u64() == Some(candidate.into()))
                    {
                        slot.port = candidate;
                    }
                }
                if slot.port == 0 {
                    bail!("could not allocate an internal model port")
                }
            }
            upsert(
                entries(config, Backend::Llamacpp)?,
                &slot.model_id,
                serde_json::to_value(&slot)?,
            )?;
        }
        ModelCommand::Remove { backend, model_id } => {
            validate_id(model_id)?;
            // Removing an absent entry must not manufacture empty backend config.
            if let Some(list) =
                config.pointer_mut(&format!("/models/{}/{}", backend.name(), backend.key()))
            {
                list.as_array_mut()
                    .context("selected models must be a list")?
                    .retain(|m| m["model_id"] != *model_id);
            }
        }
        ModelCommand::Hints {
            backend,
            model_id,
            capabilities,
            roles,
        } => {
            if capabilities.is_none() && roles.is_none() {
                bail!("supply capabilities or roles to update")
            }
            validate_id(model_id)?;
            let model = entries(config, *backend)?
                .iter_mut()
                .find(|m| m["model_id"] == *model_id)
                .context("model is not selected on this backend")?;
            let hints = model
                .as_object_mut()
                .context("model must be an object")?
                .entry("selection")
                .or_insert_with(|| json!({}));
            if let Some(value) = capabilities {
                hints["capabilities"] = json!(value);
            }
            if let Some(value) = roles {
                hints["roles"] = json!(value);
            }
            match backend {
                Backend::Ollama => {
                    serde_json::from_value::<SelectedModel>(model.clone())?.validate()?
                }
                Backend::Llamacpp => {
                    serde_json::from_value::<ModelSlot>(model.clone())?.selection_metadata()?;
                }
            }
        }
    }
    Ok(*config != before)
}

fn upsert(list: &mut Vec<Value>, id: &str, mut model: Value) -> Result<()> {
    if let Some(existing) = list.iter_mut().find(|m| m["model_id"] == id) {
        // Selection refresh preserves owner roles/restrictions unless explicitly replaced.
        if model.get("selection").is_none() && existing.get("selection").is_some() {
            model["selection"] = existing["selection"].clone();
        }
        *existing = model;
    } else {
        if list.len() >= 64 {
            bail!("selected model list exceeds 64 entries")
        }
        list.push(model);
    }
    Ok(())
}

/// Public selection view never exposes local GGUF/projector paths or service URLs.
pub fn selection_view(config: &Value) -> Result<Vec<Value>> {
    let mut selections = Vec::new();
    if let Some(raw) = config.pointer("/models/ollama/selected") {
        let models: Vec<SelectedModel> = serde_json::from_value(raw.clone())?;
        for model in models {
            model.validate()?;
            selections.push(json!({"backend":"ollama","model_id":model.model_id,"model_metadata":model.selection_metadata()?}));
        }
    }
    if let Some(raw) = config.pointer("/models/llamacpp/slots") {
        let slots: Vec<ModelSlot> = serde_json::from_value(raw.clone())?;
        for slot in slots {
            selections.push(json!({"backend":"llamacpp","model_id":slot.model_id,"model_metadata":slot.selection_metadata()?}));
        }
    }
    if selections.len() > 64 {
        bail!("total model selection exceeds 64 entries")
    }
    selections.sort_by_key(|m| {
        (
            m["backend"].as_str().unwrap().to_owned(),
            m["model_id"].as_str().unwrap().to_owned(),
        )
    });
    Ok(selections)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selected(id: &str) -> ModelCommand {
        ModelCommand::SelectOllama {
            model: SelectedModel {
                model_id: id.into(),
                modality: "vlm".into(),
                hints: ModelHints::default(),
            },
        }
    }
    fn request(command: ModelCommand) -> ModelCommandRequest {
        ModelCommandRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            command,
        }
    }
    fn service(dir: &std::path::Path) -> std::sync::Arc<crate::NodeService> {
        let path = dir.join("node.json");
        std::fs::write(&path,serde_json::to_vec(&json!({"node_id":"node","name":"owner","psk":"PRIVATE-SECRET","custom":{"retain":true},"models":{"provider":"auto"}})).unwrap()).unwrap();
        crate::NodeService::new(path, "node".into())
    }
    #[test]
    fn selections_preserve_unrelated_config_and_report_saved_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let result = core.model_command(request(selected("m"))).unwrap();
        assert_eq!(result["changed"], true);
        assert_eq!(result["restart_required"], true);
        assert!(result["registered_models"].is_null());
        assert!(!result.to_string().contains("PRIVATE"));
        let saved = core.read_config().unwrap();
        assert_eq!(saved["custom"]["retain"], true);
        assert_eq!(saved["psk"], "PRIVATE-SECRET");
        let hints = ModelCommand::Hints {
            backend: Backend::Ollama,
            model_id: "m".into(),
            capabilities: Some(vec!["llm.infer".into()]),
            roles: Some(vec!["request_interpreter".into()]),
        };
        core.model_command(request(hints)).unwrap();
        assert_eq!(
            core.model_command(request(selected("m"))).unwrap()["changed"],
            false
        );
        let view = core.model_command(request(ModelCommand::Show {})).unwrap();
        assert_eq!(
            view["saved_selections"][0]["model_metadata"]["roles"],
            json!(["request_interpreter"])
        );
        assert_eq!(
            view["saved_selections"][0]["model_metadata"]["capabilities"],
            json!(["llm.infer"])
        );
    }
    #[test]
    fn invalid_commands_and_noops_do_not_write_or_expand_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let path = dir.path().join("node.json");
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            core.model_command(request(ModelCommand::Remove {
                backend: Backend::Ollama,
                model_id: "absent".into()
            }))
            .unwrap()["changed"],
            false
        );
        assert!(!path.with_extension("json.bak").exists());
        for command in [
            ModelCommand::Provider {
                provider: "magic".into(),
            },
            ModelCommand::SelectOllama {
                model: SelectedModel {
                    model_id: "m".into(),
                    modality: "text".into(),
                    hints: ModelHints {
                        capabilities: Some(vec!["vlm.analyze".into()]),
                        roles: None,
                    },
                },
            },
            ModelCommand::Hints {
                backend: Backend::Ollama,
                model_id: "missing".into(),
                roles: Some(vec![]),
                capabilities: None,
            },
        ] {
            assert!(core.model_command(request(command)).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        let mut wrong = request(selected("m"));
        wrong.expected_node_id = "wrong".into();
        assert!(core.model_command(wrong).is_err());
        let mut wrong = request(selected("m"));
        wrong.schema_version = 2;
        assert!(core.model_command(wrong).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn gguf_selection_allocates_port_preserves_private_files_and_removes_only_selection() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let path = dir.path().join("PRIVATE.gguf");
        std::fs::write(&path, b"fixture-not-model").unwrap();
        let slot: ModelSlot =
            serde_json::from_value(json!({"model_id":"m","model_path":path,"port":0})).unwrap();
        let result = core
            .model_command(request(ModelCommand::SelectGguf { slot: slot.clone() }))
            .unwrap();
        assert!(!result.to_string().contains("PRIVATE"));
        let saved = core.read_config().unwrap();
        assert!(
            saved["models"]["llamacpp"]["slots"][0]["port"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            core.model_command(request(ModelCommand::SelectGguf { slot }))
                .unwrap()["changed"],
            false
        );
        core.model_command(request(ModelCommand::Remove {
            backend: Backend::Llamacpp,
            model_id: "m".into(),
        }))
        .unwrap();
        assert!(path.is_file());
        assert!(
            selection_view(&core.read_config().unwrap())
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn concurrent_selection_edits_merge_and_corrupt_or_missing_config_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let workers = (0..8)
            .map(|i| {
                let core = core.clone();
                std::thread::spawn(move || {
                    core.model_command(request(selected(&format!("m{i}"))))
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(
            selection_view(&core.read_config().unwrap()).unwrap().len(),
            8
        );
        std::fs::write(dir.path().join("node.json"), b"broken").unwrap();
        assert!(core.model_command(request(selected("later"))).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("node.json")).unwrap(),
            b"broken"
        );
        let missing = crate::NodeService::new(dir.path().join("missing.json"), "node".into());
        assert!(
            missing
                .model_command(request(ModelCommand::Show {}))
                .is_err()
        );
        assert!(!dir.path().join("missing.json").exists());
    }
    #[test]
    fn owner_hints_are_bounded_and_empty_capabilities_are_explicit() {
        let hints = ModelHints {
            capabilities: Some(vec![]),
            roles: Some(vec!["request_interpreter".into()]),
        };
        let metadata = hints.metadata("vlm").unwrap();
        assert!(metadata.capabilities.is_empty());
        let invalid = ModelHints {
            capabilities: None,
            roles: Some(vec!["bad role".into()]),
        };
        assert!(invalid.metadata("text").is_err());
        assert!(serde_json::from_value::<ModelHints>(json!({"lifecycle":"ready"})).is_err());
        assert!(serde_json::from_value::<ModelCommandRequest>(json!({"schema_version":1,"expected_node_id":"node","command":{"operation":"show","extra":1}})).is_err());
    }
}
