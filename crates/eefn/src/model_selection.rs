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
    /// Explicitly move only the request_interpreter role to one existing selection.
    Interpreter {
        backend: Backend,
        model_id: String,
    },
    /// Remove only the interpreter role; retain model selections and other roles.
    ClearInterpreter {},
    CpuThreads {
        model_id: String,
        #[serde(deserialize_with = "required_cpu_threads")]
        threads: Option<u16>,
    },
    MemoryLimit {
        model_id: String,
        #[serde(deserialize_with = "required_memory_limit")]
        mib: Option<u64>,
    },
}

fn required_memory_limit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(deserializer)
}

fn required_cpu_threads<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u16>, D::Error> {
    Option::<u16>::deserialize(deserializer)
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
        ModelCommand::Interpreter { backend, model_id } => {
            assign_interpreter(config, Some((*backend, model_id)))?;
        }
        ModelCommand::ClearInterpreter {} => assign_interpreter(config, None)?,
        ModelCommand::MemoryLimit { model_id, mib } => {
            validate_id(model_id)?;
            crate::model_memory::supported(*mib)?;
            let mut staged = config.clone();
            let slots = staged
                .pointer_mut("/models/llamacpp/slots")
                .and_then(Value::as_array_mut)
                .context("Local GGUF model must already be selected")?;
            if slots.iter().filter(|m| m["model_id"] == *model_id).count() != 1 {
                bail!("Memory limit command requires one exact existing local GGUF selection")
            }
            let slot = slots
                .iter_mut()
                .find(|m| m["model_id"] == *model_id)
                .unwrap();
            if let Some(mib) = mib {
                slot["job_memory_limit_mib"] = json!(mib);
            } else {
                slot.as_object_mut()
                    .context("Model must be an object")?
                    .remove("job_memory_limit_mib");
            }
            selection_view(&staged)?;
            *config = staged;
        }
        ModelCommand::CpuThreads { model_id, threads } => {
            validate_id(model_id)?;
            crate::model_server::validate_cpu_threads(*threads)?;
            let mut staged = config.clone();
            let slots = staged
                .pointer_mut("/models/llamacpp/slots")
                .and_then(Value::as_array_mut)
                .context("local GGUF model must already be selected")?;
            if slots.iter().filter(|m| m["model_id"] == *model_id).count() != 1 {
                bail!("CPU thread command requires one exact existing local GGUF selection")
            }
            let slot = slots
                .iter_mut()
                .find(|m| m["model_id"] == *model_id)
                .unwrap();
            if let Some(threads) = threads {
                slot["cpu_threads"] = json!(threads);
            } else {
                slot.as_object_mut()
                    .context("model must be an object")?
                    .remove("cpu_threads");
            }
            selection_view(&staged)?;
            *config = staged;
        }
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
            if slot.cpu_threads.is_none() {
                slot.cpu_threads = config
                    .pointer("/models/llamacpp/slots")
                    .and_then(Value::as_array)
                    .and_then(|slots| slots.iter().find(|m| m["model_id"] == slot.model_id))
                    .map(|old| serde_json::from_value::<ModelSlot>(old.clone()))
                    .transpose()?
                    .and_then(|old| old.cpu_threads);
            }
            if slot.job_memory_limit_mib.is_none() {
                slot.job_memory_limit_mib = config
                    .pointer("/models/llamacpp/slots")
                    .and_then(Value::as_array)
                    .and_then(|slots| slots.iter().find(|m| m["model_id"] == slot.model_id))
                    .map(|old| serde_json::from_value::<ModelSlot>(old.clone()))
                    .transpose()?
                    .and_then(|old| old.job_memory_limit_mib);
            }
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

/// Stage a role move in one validated configuration, never a series of client
/// hint writes. Do not expand capabilities, select files, change provider or load.
fn assign_interpreter(config: &mut Value, target: Option<(Backend, &str)>) -> Result<()> {
    if let Some((_, id)) = target {
        validate_id(id)?;
    }
    let selections = selection_view(config)?;
    let mut identities = std::collections::BTreeSet::new();
    for selection in &selections {
        let identity = (
            selection["backend"].as_str().context("missing backend")?,
            selection["model_id"].as_str().context("missing model ID")?,
        );
        if !identities.insert(identity) {
            bail!("duplicate model selections must be resolved before assigning an interpreter")
        }
    }
    if let Some((backend, id)) = target {
        let selected = selections
            .iter()
            .find(|m| m["backend"] == backend.name() && m["model_id"] == id)
            .context("interpreter model must already be selected on the specified backend")?;
        let metadata: ModelMetadata = serde_json::from_value(selected["model_metadata"].clone())?;
        if !metadata.supports("llm.infer")
            || !metadata.input_modalities.iter().any(|m| m == "text")
            || !metadata.output_modalities.iter().any(|m| m == "text")
        {
            bail!(
                "interpreter selection must already support text llm.infer; capabilities were not expanded"
            )
        }
    }
    let mut staged = config.clone();
    for backend in [Backend::Ollama, Backend::Llamacpp] {
        // Missing backends/selection hints are not manufactured when clearing.
        let Some(raw) =
            staged.pointer_mut(&format!("/models/{}/{}", backend.name(), backend.key()))
        else {
            continue;
        };
        let models = raw
            .as_array_mut()
            .context("selected models must be a list")?;
        for model in models {
            let is_target = target.is_some_and(|(b, id)| b == backend && model["model_id"] == id);
            let had_role = model
                .pointer("/selection/roles")
                .and_then(Value::as_array)
                .is_some_and(|roles| roles.iter().any(|r| r == "request_interpreter"));
            if is_target == had_role {
                continue;
            }
            let hints = model
                .as_object_mut()
                .context("model must be an object")?
                .entry("selection")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .context("selection must be an object")?;
            let mut roles = hints
                .get("roles")
                .filter(|v| !v.is_null())
                .map(|v| serde_json::from_value::<Vec<String>>(v.clone()))
                .transpose()?
                .unwrap_or_default();
            roles.retain(|role| role != "request_interpreter");
            if is_target {
                roles.push("request_interpreter".into());
            }
            hints.insert("roles".into(), json!(roles));
        }
    }
    // Includes bounded roles and full adapter restrictions. Failed edits leave
    // even the caller's in-memory value unchanged, not just the saved file.
    selection_view(&staged)?;
    *config = staged;
    Ok(())
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
            let mut value = json!({"backend":"llamacpp","model_id":slot.model_id,"model_metadata":slot.selection_metadata()?});
            if let Some(threads) = slot.cpu_threads {
                value["cpu_threads"] = json!(threads);
            }
            if let Some(limit) = slot.job_memory_limit_mib {
                value["job_memory_limit_mib"] = json!(limit);
            }
            selections.push(value);
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

/// Confirm explicit role/CPU operations. Saved settings never attest to
/// applied readiness, model quality or execution authority.
pub fn confirmed_interpreter(value: &Value, node: &str, command: &ModelCommand) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Selection {
        backend: Backend,
        model_id: String,
        model_metadata: ModelMetadata,
        #[serde(default)]
        cpu_threads: Option<u16>,
        #[serde(default)]
        job_memory_limit_mib: Option<u64>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Report {
        schema_version: u32,
        success: bool,
        report_type: String,
        node_id: String,
        changed: bool,
        saved_selections: Vec<Selection>,
        registered_models: Option<Vec<Selection>>,
        remote_management_allowed: bool,
        provider: String,
        restart_required: bool,
        note: String,
    }
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let _ = (
        report.changed,
        report.remote_management_allowed,
        report.restart_required,
    );
    if report.schema_version != 1
        || !report.success
        || report.report_type != "model_selections"
        || report.node_id != node
        || report.note.len() > 2048
        || !matches!(report.provider.as_str(), "auto" | "ollama" | "llamacpp")
        || report.saved_selections.len() > 64
        || report
            .registered_models
            .as_ref()
            .is_some_and(|m| m.len() > 64)
    {
        return false;
    }
    for selections in
        std::iter::once(&report.saved_selections).chain(report.registered_models.iter())
    {
        let mut ids = std::collections::BTreeSet::new();
        for model in selections {
            if validate_id(&model.model_id).is_err()
                || model.model_metadata.validate().is_err()
                || crate::model_server::validate_cpu_threads(model.cpu_threads).is_err()
                || (model.cpu_threads.is_some() && model.backend != Backend::Llamacpp)
                || crate::model_memory::validate(model.job_memory_limit_mib).is_err()
                || (model.job_memory_limit_mib.is_some() && model.backend != Backend::Llamacpp)
                || !ids.insert((model.backend.name(), &model.model_id))
            {
                return false;
            }
        }
    }
    let interpreters = report
        .saved_selections
        .iter()
        .filter(|m| {
            m.model_metadata
                .roles
                .as_ref()
                .is_some_and(|roles| roles.iter().any(|r| r == "request_interpreter"))
        })
        .collect::<Vec<_>>();
    match command {
        ModelCommand::Interpreter { backend, model_id } => {
            validate_id(model_id).is_ok()
                && interpreters.len() == 1
                && interpreters[0].backend == *backend
                && interpreters[0].model_id == *model_id
                && interpreters[0].model_metadata.supports("llm.infer")
                && interpreters[0]
                    .model_metadata
                    .input_modalities
                    .iter()
                    .any(|m| m == "text")
                && interpreters[0]
                    .model_metadata
                    .output_modalities
                    .iter()
                    .any(|m| m == "text")
        }
        ModelCommand::ClearInterpreter {} => interpreters.is_empty(),
        ModelCommand::MemoryLimit { model_id, mib } => {
            validate_id(model_id).is_ok()
                && crate::model_memory::validate(*mib).is_ok()
                && report.saved_selections.iter().any(|m| {
                    m.backend == Backend::Llamacpp
                        && m.model_id == *model_id
                        && m.job_memory_limit_mib == *mib
                })
        }
        ModelCommand::CpuThreads { model_id, threads } => {
            validate_id(model_id).is_ok()
                && crate::model_server::validate_cpu_threads(*threads).is_ok()
                && report.saved_selections.iter().any(|m| {
                    m.backend == Backend::Llamacpp
                        && m.model_id == *model_id
                        && m.cpu_threads == *threads
                })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_limit_wire_reset_is_explicit_and_invalid_choices_preserve_config() {
        let original = json!({"models":{"provider":"llamacpp","llamacpp":{"slots":[{"model_id":"m","model_path":"unchanged.gguf"}]}},"unrelated":true});
        for raw in [
            json!({"operation":"memory_limit","model_id":"m"}),
            json!({"operation":"memory_limit","model_id":"m","mib":"128"}),
            json!({"operation":"memory_limit","model_id":"m","mib":128,"backend":"ollama"}),
        ] {
            assert!(serde_json::from_value::<ModelCommand>(raw).is_err());
        }
        for limit in [0, 63, 1_048_577] {
            let mut config = original.clone();
            assert!(
                apply(
                    &mut config,
                    &ModelCommand::MemoryLimit {
                        model_id: "m".into(),
                        mib: Some(limit)
                    }
                )
                .is_err()
            );
            assert_eq!(config, original);
        }
        #[cfg(windows)]
        {
            let mut config = original.clone();
            let set = ModelCommand::MemoryLimit {
                model_id: "m".into(),
                mib: Some(128),
            };
            assert!(apply(&mut config, &set).unwrap());
            assert!(!apply(&mut config, &set).unwrap());
            assert_eq!(
                selection_view(&config).unwrap()[0]["job_memory_limit_mib"],
                128
            );
            let mut duplicate = config.clone();
            duplicate["models"]["llamacpp"]["slots"]
                .as_array_mut()
                .unwrap()
                .push(json!({"model_id":"m"}));
            let before = duplicate.clone();
            assert!(apply(&mut duplicate, &set).is_err());
            assert_eq!(duplicate, before);
            assert!(
                apply(
                    &mut config,
                    &ModelCommand::MemoryLimit {
                        model_id: "m".into(),
                        mib: None
                    }
                )
                .unwrap()
            );
            assert_eq!(config, original);
        }
    }
    #[test]
    fn cpu_threads_preserve_exact_selection_and_reject_ambiguous_or_invalid_updates() {
        let mut config = interpreter_config();
        let initial = config.clone();
        let command = |id: &str, threads| ModelCommand::CpuThreads {
            model_id: id.into(),
            threads,
        };
        assert!(apply(&mut config, &command("same-id", Some(4))).unwrap());
        assert_eq!(config["models"]["llamacpp"]["slots"][0]["cpu_threads"], 4);
        assert_eq!(selection_view(&config).unwrap()[0]["cpu_threads"], 4);
        assert!(!apply(&mut config, &command("same-id", Some(4))).unwrap());
        let before = config.clone();
        for cmd in [
            command("missing", Some(4)),
            command("same-id", Some(0)),
            command("same-id", Some(257)),
        ] {
            assert!(apply(&mut config, &cmd).is_err());
            assert_eq!(config, before);
        }
        let duplicate = config["models"]["llamacpp"]["slots"][0].clone();
        config["models"]["llamacpp"]["slots"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let duplicated = config.clone();
        assert!(apply(&mut config, &command("same-id", Some(2))).is_err());
        assert_eq!(config, duplicated);
        config = before;
        assert!(apply(&mut config, &command("same-id", None)).unwrap());
        assert_eq!(
            config, initial,
            "clear only the exact explicit thread setting"
        );
        assert!(!apply(&mut config, &command("same-id", None)).unwrap());
        for raw in [
            json!({"operation":"cpu_threads","model_id":"same-id"}),
            json!({"operation":"cpu_threads","model_id":"same-id","threads":4,"backend":"ollama"}),
        ] {
            assert!(serde_json::from_value::<ModelCommand>(raw).is_err());
        }
    }
    #[tokio::test]
    #[cfg(windows)]
    async fn memory_limit_reports_reject_substitution_and_reselection_keeps_ceiling() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let path = dir.path().join("PRIVATE.gguf");
        std::fs::write(&path, b"not-a-model").unwrap();
        let slot: ModelSlot =
            serde_json::from_value(json!({"model_id":"local","model_path":path,"port":0})).unwrap();
        core.model_command(request(ModelCommand::SelectGguf { slot: slot.clone() }))
            .unwrap();
        let command = ModelCommand::MemoryLimit {
            model_id: "local".into(),
            mib: Some(128),
        };
        let before = core.read_config().unwrap();
        let denied = core
            .remote(
                "model_command",
                serde_json::to_value(request(command.clone())).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied["error_code"], "approval_required");
        assert_eq!(core.read_config().unwrap(), before);
        let report = core.model_command(request(command.clone())).unwrap();
        assert!(confirmed_interpreter(&report, "node", &command));
        assert!(report["registered_models"].is_null());
        assert!(!report.to_string().contains("PRIVATE"));
        for (key, value) in [
            ("job_memory_limit_mib", json!(null)),
            ("job_memory_limit_mib", json!(256)),
            ("job_memory_limit_mib", json!(0)),
            ("backend", json!("ollama")),
        ] {
            let mut bad = report.clone();
            bad["saved_selections"][0][key] = value;
            assert!(!confirmed_interpreter(&bad, "node", &command));
        }
        core.model_command(request(ModelCommand::SelectGguf { slot }))
            .unwrap();
        assert_eq!(
            core.read_config().unwrap()["models"]["llamacpp"]["slots"][0]["job_memory_limit_mib"],
            128
        );
        let reset = ModelCommand::MemoryLimit {
            model_id: "local".into(),
            mib: None,
        };
        assert!(confirmed_interpreter(
            &core.model_command(request(reset.clone())).unwrap(),
            "node",
            &reset
        ));
        assert!(path.is_file());
    }
    #[tokio::test]
    async fn cpu_thread_reports_and_owner_permission_do_not_claim_runtime_or_budget() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        let path = dir.path().join("PRIVATE.gguf");
        std::fs::write(&path, b"not-a-model").unwrap();
        let slot: ModelSlot = serde_json::from_value(
            json!({"model_id":"local","model_path":path,"port":0,"cpu_threads":4}),
        )
        .unwrap();
        core.model_command(request(ModelCommand::SelectGguf { slot: slot.clone() }))
            .unwrap();
        let cpu = ModelCommand::CpuThreads {
            model_id: "local".into(),
            threads: Some(2),
        };
        let before = core.read_config().unwrap();
        let denied = core
            .remote(
                "model_command",
                serde_json::to_value(request(cpu.clone())).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied["error_code"], "approval_required");
        assert_eq!(core.read_config().unwrap(), before);
        let report = core.model_command(request(cpu.clone())).unwrap();
        assert!(confirmed_interpreter(&report, "node", &cpu));
        assert!(report["registered_models"].is_null());
        assert_eq!(report["saved_selections"][0]["cpu_threads"], 2);
        assert_eq!(report["restart_required"], true);
        assert!(!report.to_string().contains("PRIVATE"));
        for (key, value) in [
            ("cpu_threads", json!(4)),
            ("backend", json!("ollama")),
            ("cpu_threads", json!(257)),
        ] {
            let mut wrong = report.clone();
            wrong["saved_selections"][0][key] = value;
            assert!(!confirmed_interpreter(&wrong, "node", &cpu));
        }
        let mut reselect = slot;
        reselect.cpu_threads = None;
        core.model_command(request(ModelCommand::SelectGguf { slot: reselect }))
            .unwrap();
        assert_eq!(
            core.read_config().unwrap()["models"]["llamacpp"]["slots"][0]["cpu_threads"],
            2,
            "reselection must not silently widen worker counts"
        );
        let clear = ModelCommand::CpuThreads {
            model_id: "local".into(),
            threads: None,
        };
        assert!(confirmed_interpreter(
            &core.model_command(request(clear.clone())).unwrap(),
            "node",
            &clear
        ));
        assert!(path.is_file());
    }
    fn interpreter_config() -> Value {
        json!({"future":{"keep":true},"models":{"provider":"auto",
            "ollama":{"base_url":"http://PRIVATE","selected":[
                {"model_id":"same-id","modality":"text","selection":{"capabilities":["llm.infer"],"roles":["request_interpreter","node_tool"]}},
                {"model_id":"other","modality":"vlm","selection":{"roles":["request_interpreter","other_role"]}}]},
            "llamacpp":{"slots":[{"model_id":"same-id","model_path":"PRIVATE.gguf","port":19000,
                "selection":{"roles":["heavy_planner"]}}]}}})
    }
    fn interpreter(backend: Backend, id: &str) -> ModelCommand {
        ModelCommand::Interpreter {
            backend,
            model_id: id.into(),
        }
    }
    #[test]
    fn interpreter_role_move_is_unique_atomic_and_preserves_other_fields() {
        let mut config = interpreter_config();
        let before = config.clone();
        assert!(apply(&mut config, &interpreter(Backend::Llamacpp, "same-id")).unwrap());
        let view = selection_view(&config).unwrap();
        assert_eq!(
            view.iter()
                .filter(|m| m["model_metadata"]["roles"]
                    .as_array()
                    .is_some_and(|r| r.contains(&json!("request_interpreter"))))
                .count(),
            1
        );
        assert_eq!(
            config["models"]["ollama"]["selected"][0]["selection"]["roles"],
            json!(["node_tool"])
        );
        assert_eq!(
            config["models"]["ollama"]["selected"][1]["selection"]["roles"],
            json!(["other_role"])
        );
        assert_eq!(
            config["models"]["llamacpp"]["slots"][0]["selection"]["roles"],
            json!(["heavy_planner", "request_interpreter"])
        );
        let saved = config.clone();
        assert!(!apply(&mut config, &interpreter(Backend::Llamacpp, "same-id")).unwrap());
        assert_eq!(config, saved);
        // Strip only roles from both values; everything else must be unchanged.
        let mut expected = before;
        for value in [&mut config, &mut expected] {
            for pointer in ["/models/ollama/selected", "/models/llamacpp/slots"] {
                for model in value.pointer_mut(pointer).unwrap().as_array_mut().unwrap() {
                    model["selection"].as_object_mut().unwrap().remove("roles");
                }
            }
        }
        assert_eq!(config, expected);
    }
    #[test]
    fn interpreter_clear_retains_selections_and_does_not_manufacture_config() {
        let mut config = interpreter_config();
        assert!(apply(&mut config, &ModelCommand::ClearInterpreter {}).unwrap());
        assert_eq!(selection_view(&config).unwrap().len(), 3);
        assert_eq!(
            config["models"]["llamacpp"]["slots"][0]["selection"]["roles"],
            json!(["heavy_planner"])
        );
        assert!(!apply(&mut config, &ModelCommand::ClearInterpreter {}).unwrap());
        let mut empty = json!({"unrelated":true});
        assert!(!apply(&mut empty, &ModelCommand::ClearInterpreter {}).unwrap());
        assert_eq!(empty, json!({"unrelated":true}));
    }
    #[test]
    fn interpreter_reports_confirm_exact_saved_role_not_applied_readiness() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        for id in ["chosen", "other"] {
            core.model_command(request(selected(id))).unwrap();
        }
        let command = interpreter(Backend::Ollama, "chosen");
        let report = core.model_command(request(command.clone())).unwrap();
        assert!(confirmed_interpreter(&report, "node", &command));
        assert!(report["registered_models"].is_null());
        assert!(!confirmed_interpreter(&report, "wrong-node", &command));
        assert!(!confirmed_interpreter(
            &report,
            "node",
            &interpreter(Backend::Llamacpp, "chosen")
        ));
        assert!(!confirmed_interpreter(
            &report,
            "node",
            &ModelCommand::ClearInterpreter {}
        ));
        for case in 0..7 {
            let mut bad = report.clone();
            match case {
                0 => {
                    bad["saved_selections"][1]["model_metadata"]["roles"] =
                        json!(["request_interpreter"])
                }
                1 => bad["saved_selections"][0]["model_metadata"]["capabilities"] = json!([]),
                2 => bad["saved_selections"][0]["model_metadata"]["input_modalities"] = json!([]),
                3 => bad["provider"] = json!("made-up"),
                4 => {
                    let duplicate = bad["saved_selections"][0].clone();
                    bad["saved_selections"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                5 => bad["execution_authorized"] = json!(true),
                _ => bad["success"] = json!(false),
            }
            assert!(
                !confirmed_interpreter(&bad, "node", &command),
                "case {case}"
            );
        }
        let clear = ModelCommand::ClearInterpreter {};
        let report = core.model_command(request(clear.clone())).unwrap();
        assert!(confirmed_interpreter(&report, "node", &clear));
        assert!(!confirmed_interpreter(&report, "node", &command));
    }
    #[test]
    fn failed_interpreter_moves_leave_even_in_memory_config_unchanged() {
        for case in 0..6 {
            let mut config = interpreter_config();
            let id = match case {
                0 => "missing",
                1 => " bad-id",
                2 => {
                    config["models"]["llamacpp"]["slots"][0]["selection"]["capabilities"] =
                        json!([]);
                    "same-id"
                }
                3 => {
                    config["models"]["llamacpp"]["slots"][0]["selection"]["roles"] =
                        json!((0..32).map(|i| format!("role_{i}")).collect::<Vec<_>>());
                    "same-id"
                }
                4 => {
                    let duplicate = config["models"]["llamacpp"]["slots"][0].clone();
                    config["models"]["llamacpp"]["slots"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                    "same-id"
                }
                _ => {
                    config["models"]["ollama"]["selected"][1]["selection"]["roles"] =
                        json!(["bad role"]);
                    "same-id"
                }
            };
            let before = config.clone();
            assert!(
                apply(&mut config, &interpreter(Backend::Llamacpp, id)).is_err(),
                "case {case}"
            );
            assert_eq!(config, before, "case {case}");
        }
    }
    #[tokio::test]
    async fn interpreter_owner_authorization_and_concurrent_moves_use_existing_config_lock() {
        let dir = tempfile::tempdir().unwrap();
        let core = service(dir.path());
        for id in ["first", "second"] {
            core.model_command(request(selected(id))).unwrap();
        }
        let before = core.read_config().unwrap();
        let denied = core
            .remote(
                "model_command",
                serde_json::to_value(request(interpreter(Backend::Ollama, "first"))).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied["error_code"], "approval_required");
        assert_eq!(core.read_config().unwrap(), before);
        let workers = ["first", "second"]
            .into_iter()
            .map(|id| {
                let core = core.clone();
                std::thread::spawn(move || {
                    core.model_command(request(interpreter(Backend::Ollama, id)))
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            assert_eq!(worker.join().unwrap()["success"], true);
        }
        let saved = core.read_config().unwrap();
        assert_eq!(
            selection_view(&saved)
                .unwrap()
                .iter()
                .filter(|m| m["model_metadata"]["roles"]
                    .as_array()
                    .is_some_and(|r| r.contains(&json!("request_interpreter"))))
                .count(),
            1
        );
        assert_eq!(saved["custom"]["retain"], true);
        let clear_denied = core
            .remote(
                "model_command",
                serde_json::to_value(request(ModelCommand::ClearInterpreter {})).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(clear_denied["error_code"], "approval_required");
        assert_eq!(core.read_config().unwrap(), saved);
    }
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
