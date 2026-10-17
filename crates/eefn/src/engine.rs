use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::model_server::ModelServer;
use crate::python::{PythonPlugin, PythonRuntime};
use crate::updater;

pub const CAPABILITIES: &[&str] = &["system.ping", "system.info", "node.update"];

const MODEL_CAPABILITIES: &[&str] = &["llm.infer", "vlm.analyze"];
const POLICY_CAPABILITIES: &[&str] = &[
    "filesystem",
    "process.exec",
    "application.control",
    "launch_application",
    "http.request",
    "stt.transcribe",
    "network.wol",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodePolicy {
    /// Explicit feature-wide grants. Empty on upgrade: legacy scopes stay limited.
    #[serde(default)]
    pub full_access: Vec<String>,
    #[serde(default)]
    pub remote_updates: bool,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub process: ProcessPolicy,
    #[serde(default)]
    pub applications: ApplicationPolicy,
    #[serde(default)]
    pub http: HttpPolicy,
    #[serde(default)]
    pub stt: SttPolicy,
    #[serde(default)]
    pub wake_on_lan: WakeOnLanPolicy,
    #[serde(default)]
    pub media: MediaPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default = "default_file_bytes")]
    pub max_read_bytes: usize,
    #[serde(default = "default_file_bytes")]
    pub max_write_bytes: usize,
    #[serde(default = "default_list_entries")]
    pub max_list_entries: usize,
}

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self {
            read: false,
            write: false,
            roots: Vec::new(),
            max_read_bytes: default_file_bytes(),
            max_write_bytes: default_file_bytes(),
            max_list_entries: default_list_entries(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_executables: Vec<String>,
    #[serde(default = "default_process_timeout")]
    pub max_seconds: u64,
    #[serde(default = "default_output_bytes")]
    pub max_output_bytes: usize,
}

impl Default for ProcessPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_executables: Vec::new(),
            max_seconds: default_process_timeout(),
            max_output_bytes: default_output_bytes(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ApplicationPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default = "default_http_methods")]
    pub methods: Vec<String>,
    #[serde(default)]
    pub allow_private_networks: bool,
    #[serde(default = "default_http_response_bytes")]
    pub max_response_bytes: usize,
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_hosts: Vec::new(),
            methods: default_http_methods(),
            allow_private_networks: false,
            max_response_bytes: default_http_response_bytes(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SttPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key_env: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WakeOnLanPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_macs: Vec<String>,
    #[serde(default)]
    pub broadcast_addresses: Vec<Ipv4Addr>,
    #[serde(default)]
    pub ports: Vec<u16>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MediaPolicy {
    #[serde(default)]
    pub microphone: bool,
    #[serde(default)]
    pub audio_output: bool,
    #[serde(default)]
    pub tts: bool,
    #[serde(default)]
    pub camera: bool,
    #[serde(default)]
    pub screen_capture: bool,
    #[serde(default)]
    pub input_control: bool,
}

impl NodePolicy {
    fn unrestricted(&self, feature: &str) -> bool {
        self.full_access.iter().any(|grant| grant == feature)
    }

    pub fn validate(self) -> Result<Self> {
        normalize_policy(self)
    }
}

const fn default_process_timeout() -> u64 {
    30
}

const fn default_file_bytes() -> usize {
    4 * 1024 * 1024
}

const fn default_list_entries() -> usize {
    10_000
}

const fn default_output_bytes() -> usize {
    1024 * 1024
}

fn default_http_methods() -> Vec<String> {
    vec!["GET".into()]
}

const fn default_http_response_bytes() -> usize {
    4 * 1024 * 1024
}

pub struct NodeEngine {
    policy: NodePolicy,
    pub model_server: Option<Arc<ModelServer>>,
    python: Option<Arc<PythonRuntime>>,
    plugins: HashMap<String, PythonPlugin>,
    install_dir: PathBuf,
    update_manifest: Option<String>,
    current_version: String,
    http: reqwest::Client,
    ollama_url: String,
    ollama_models: HashMap<String, crate::model_metadata::ModelMetadata>,
    ollama_keep_alive_seconds: Option<i64>,
    thinking_mode: crate::model_lifecycle_policy::ThinkingMode,
    interpreter_sampling: crate::interpreter_sampling::InterpreterSampling,
    service: Option<Arc<crate::NodeService>>,
    resource_owner: String,
    resources: Vec<crate::context::Resource>,
    granted_model_config: Option<Value>,
    pipeline_enabled: bool,
}

impl NodeEngine {
    pub fn new(
        allow_write: bool,
        allowed_roots: Vec<PathBuf>,
        install_dir: PathBuf,
    ) -> Result<Self> {
        let policy = NodePolicy {
            filesystem: FilesystemPolicy {
                read: !allowed_roots.is_empty(),
                write: allow_write && !allowed_roots.is_empty(),
                roots: allowed_roots,
                ..FilesystemPolicy::default()
            },
            ..NodePolicy::default()
        };
        let current_version = std::fs::read_to_string(install_dir.join("current.txt"))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| crate::VERSION.into());
        Ok(Self {
            policy: normalize_policy(policy)?,
            model_server: None,
            python: None,
            plugins: HashMap::new(),
            install_dir,
            update_manifest: None,
            current_version,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            ollama_url: String::new(),
            ollama_models: HashMap::new(),
            ollama_keep_alive_seconds: None,
            thinking_mode: Default::default(),
            interpreter_sampling: Default::default(),
            service: None,
            granted_model_config: None,
            pipeline_enabled: false,
            resource_owner: String::new(),
            resources: Vec::new(),
        })
    }

    pub fn with_policy(mut self, policy: NodePolicy) -> Result<Self> {
        self.policy = normalize_policy(policy)?;
        Ok(self)
    }

    pub(crate) fn applied_permissions(&self) -> &NodePolicy {
        &self.policy
    }

    pub fn with_resources(
        mut self,
        owner: String,
        metadata: crate::context::NodeMetadata,
    ) -> Result<Self> {
        metadata.validate()?;
        self.resource_owner = owner;
        self.resources = metadata.resources;
        Ok(self)
    }
    pub fn with_service(mut self, service: Arc<crate::NodeService>) -> Self {
        self.granted_model_config = service.read_config().ok().map(|c| c["models"].clone());
        self.service = Some(service);
        self
    }

    /// Compatibility alias; the owned object is a runtime service, not a UI.
    pub fn with_dashboard(self, service: Arc<crate::NodeService>) -> Self {
        self.with_service(service)
    }

    pub fn with_model_server(mut self, server: Arc<ModelServer>) -> Self {
        self.model_server = Some(server);
        self
    }
    /// The compiler/public command integration must opt in explicitly. A config
    /// value, model output, or plugin name cannot enable compound authority.
    pub fn with_pipeline_runtime(mut self) -> Self {
        self.pipeline_enabled = true;
        self
    }
    pub(crate) fn record_pipeline_progress(&self, g: &crate::grants::ExecutionGrant, value: Value) {
        if let Some(service) = &self.service {
            let mut live = service.live.lock().unwrap();
            if live["last_workload"]["grant_id"] == json!(g.grant_id) {
                live["last_workload"]["pipeline"] = value;
            }
        }
    }

    pub fn with_update_manifest(mut self, manifest: Option<String>) -> Self {
        self.update_manifest = manifest;
        self
    }

    /// Applied owner preference for shared backend requests, not a background
    /// unload loop or permission to stop other applications' model service.
    pub fn with_ollama_residency(
        self,
        policy: &crate::model_lifecycle_policy::ModelLifecyclePolicy,
    ) -> Result<Self> {
        self.with_model_request_preferences(policy)
    }
    /// Snapshot applied owner hints. Saving policy does not mutate this runtime.
    pub fn with_model_request_preferences(
        mut self,
        policy: &crate::model_lifecycle_policy::ModelLifecyclePolicy,
    ) -> Result<Self> {
        policy.validate()?;
        self.ollama_keep_alive_seconds = policy.ollama_keep_alive_seconds;
        self.thinking_mode = policy.thinking_mode;
        self.interpreter_sampling = policy.interpreter_sampling.clone();
        Ok(self)
    }
    pub(crate) fn applied_thinking_mode(&self) -> crate::model_lifecycle_policy::ThinkingMode {
        self.thinking_mode
    }
    pub(crate) fn applied_interpreter_sampling(
        &self,
    ) -> &crate::interpreter_sampling::InterpreterSampling {
        &self.interpreter_sampling
    }
    pub(crate) fn applied_ollama_keep_alive_seconds(&self) -> Option<i64> {
        self.ollama_keep_alive_seconds
    }

    pub fn with_ollama(
        mut self,
        url: String,
        models: impl IntoIterator<Item = crate::SelectedModel>,
    ) -> Result<Self> {
        self.ollama_url = url;
        for model in models {
            model.validate()?;
            self.ollama_models
                .insert(model.model_id.clone(), model.selection_metadata()?);
        }
        Ok(self)
    }

    pub async fn add_python_plugin(
        &mut self,
        runtime: Arc<PythonRuntime>,
        path: PathBuf,
    ) -> Result<()> {
        let plugin = runtime.inspect(path).await?;
        if self.plugins.contains_key(&plugin.capability)
            || plugin.capability == "node.configure"
            || CAPABILITIES.contains(&plugin.capability.as_str())
            || MODEL_CAPABILITIES.contains(&plugin.capability.as_str())
            || POLICY_CAPABILITIES.contains(&plugin.capability.as_str())
        {
            bail!("duplicate capability '{}'", plugin.capability);
        }
        self.plugins.insert(plugin.capability.clone(), plugin);
        self.python = Some(runtime);
        Ok(())
    }

    pub fn capabilities(&self) -> Vec<String> {
        let mut values = CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        if self.pipeline_enabled {
            values.push("pipeline.run".into());
        }
        if self.service.is_some() {
            values.push("node.configure".into());
        }
        if self.policy.filesystem.read || self.policy.filesystem.write {
            values.push("filesystem".into());
        }
        if self.policy.process.enabled {
            values.push("process.exec".into());
        }
        if self.policy.applications.enabled {
            values.push("application.control".into());
            values.push("launch_application".into());
        }
        if self.policy.http.enabled {
            values.push("http.request".into());
        }
        if self.policy.stt.enabled {
            values.push("stt.transcribe".into());
        }
        if self.policy.wake_on_lan.enabled {
            values.push("network.wol".into());
        }
        values.extend(self.plugins.keys().cloned());
        let has_text = self
            .ollama_models
            .values()
            .any(|value| value.supports("llm.infer"))
            || self.model_server.as_ref().is_some_and(|server| {
                server.slots.iter().any(|slot| {
                    server.can_infer(Some(&slot.model_id))
                        && slot
                            .selection_metadata()
                            .is_ok_and(|m| m.supports("llm.infer"))
                })
            });
        let has_vlm = self
            .ollama_models
            .values()
            .any(|value| value.supports("vlm.analyze"))
            || self.model_server.as_ref().is_some_and(|server| {
                server.slots.iter().any(|slot| {
                    server.can_infer(Some(&slot.model_id))
                        && slot
                            .selection_metadata()
                            .is_ok_and(|m| m.supports("vlm.analyze"))
                })
            });
        if has_text {
            values.push("llm.infer".into());
        }
        if has_vlm {
            values.push("vlm.analyze".into());
        }
        values.sort();
        values
    }

    pub fn python_plugins(&self) -> Vec<&PythonPlugin> {
        self.plugins.values().collect()
    }

    /// Grants never widen applied node policy. Changed relevant saved scopes fail
    /// closed until restart, and revocation is checked before/pending execution.
    pub(crate) fn check_grant_policy(&self, g: &crate::grants::ExecutionGrant) -> Result<()> {
        self.check_parent_policy(g, false)
    }
    pub(crate) fn check_grant_admission(&self, g: &crate::grants::ExecutionGrant) -> Result<()> {
        self.check_parent_policy(g, true)
    }
    fn check_parent_policy(
        &self,
        g: &crate::grants::ExecutionGrant,
        admission: bool,
    ) -> Result<()> {
        if g.capability == "pipeline.run" {
            if !self.pipeline_enabled {
                bail!("Pipeline compiler/runtime integration is not enabled")
            }
            let definition = crate::pipeline::FinitePipeline::from_grant(g)?;
            for step in &definition.steps {
                let params = step.bound_params()?;
                self.check_operation_policy(step.operation(&params), &g.limits, admission)?;
            }
            Ok(())
        } else {
            self.check_operation_policy(
                crate::grants::OperationShape {
                    capability: &g.capability,
                    action: &g.action,
                    params: &g.params,
                    resource_id: g.resource_id.as_deref(),
                    model_id: g.model_id.as_deref(),
                    backend: g.backend.as_deref(),
                    model_role: g.model_role.as_deref(),
                },
                &g.limits,
                true,
            )
        }
    }
    /// Applied/current local operation intersection, not separate child grants.
    pub(crate) fn check_operation_policy(
        &self,
        g: crate::grants::OperationShape<'_>,
        granted_limits: &crate::grants::GrantLimits,
        require_ready: bool,
    ) -> Result<()> {
        let model_operation = matches!(g.capability, "llm.infer" | "vlm.analyze");
        if g.capability == "filesystem"
            && if g.action == "write" {
                !self.policy.filesystem.write
            } else {
                !self.policy.filesystem.read
            }
        {
            bail!("Granted filesystem action is not enabled on this node")
        }
        if (require_ready || !model_operation)
            && !self.capabilities().iter().any(|c| c == g.capability)
        {
            bail!("Granted capability is not enabled on this node")
        }
        let section = match g.capability {
            "system.info" | "system.ping" => None,
            "filesystem" => Some("filesystem"),
            "process.exec" => Some("process"),
            "application.control" => Some("applications"),
            "http.request" => Some("http"),
            "stt.transcribe" => Some("stt"),
            "network.wol" => Some("wake_on_lan"),
            "audio.capture" | "audio.play" | "tts.speak" | "camera.capture" | "screen.capture"
            | "input.control" => Some("media"),
            "llm.infer" | "vlm.analyze" => Some("models"),
            _ => bail!("Capability has no supported grant enforcement contract"),
        };
        if let Some(service) = &self.service {
            service.inspect_grant_configuration(|config| {
                let limits: crate::grants::GrantPolicy = serde_json::from_value(
                    config.get("grants").cloned().unwrap_or_else(|| json!({})),
                )?;
                limits.validate()?;
                if granted_limits.max_execution_ms > limits.max_duration_ms
                    || granted_limits.max_result_bytes > limits.max_result_bytes
                {
                    bail!("Saved node grant limits were reduced")
                }
                if section == Some("models") {
                    if self.granted_model_config.as_ref() != Some(&config["models"]) {
                        bail!("Model selection changed; restart/replan required")
                    }
                } else if let Some(section) = section {
                    let current: NodePolicy = serde_json::from_value(
                        config
                            .get("permissions")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    )?;
                    let current = normalize_policy(current)?;
                    if grant_policy_scope(&self.policy, g.capability, g.action, section)
                        != grant_policy_scope(&current, g.capability, g.action, section)
                    {
                        bail!("Relevant local permissions changed; restart/replan required")
                    }
                }
                if g.resource_id.is_some() {
                    let latest: crate::context::NodeMetadata = serde_json::from_value(
                        config.get("metadata").cloned().unwrap_or_else(|| json!({})),
                    )?;
                    if json!(latest.resources) != json!(self.resources) {
                        bail!("Local resource bindings changed; restart/replan required")
                    }
                }
                Ok(())
            })?;
        }
        if let Some(model) = g.model_id {
            let metadata = if g.backend == Some("ollama") {
                self.ollama_models
                    .get(model)
                    .context("Granted Ollama model is not selected")?
                    .clone()
            } else {
                let server = self
                    .model_server
                    .as_ref()
                    .context("Granted local model is not configured")?;
                if require_ready && !server.can_infer(Some(model)) {
                    bail!("Granted local model is not ready")
                }
                server
                    .slot(Some(model))
                    .context("Granted local model is not selected")?
                    .selection_metadata()?
            };
            if !metadata.supports(g.capability)
                || !metadata.routable()
                || g.model_role.is_some_and(|role| {
                    metadata
                        .roles
                        .as_ref()
                        .is_none_or(|r| !r.iter().any(|r| r == role))
                })
            {
                bail!("Granted model capability/role is not enabled")
            }
        }
        if let Some(id) = g.resource_id {
            if !self.resources.iter().any(|r| {
                crate::context::resource_id(&self.resource_owner, &r.id) == id
                    && r.available
                    && r.capability == g.capability
            }) {
                bail!(
                    "Granted physical resource is missing, unavailable or bound to another operation"
                )
            }
        }
        Ok(())
    }

    pub async fn execute(
        &self,
        capability: &str,
        action: &str,
        mut params: Value,
    ) -> Result<Value> {
        if capability == "pipeline.run" {
            return Err(crate::execution_boundary::ExecutionGrantRequired.into());
        }
        if let Some(id) = params.get("resource_id") {
            let id = id.as_str().context("resource_id must be a string")?;
            let resource = self
                .resources
                .iter()
                .find(|resource| {
                    crate::context::resource_id(&self.resource_owner, &resource.id) == id
                })
                .context("resource does not belong to this node")?;
            if !resource.available || resource.capability != capability {
                bail!("resource is unavailable for this capability")
            }
            let object = params
                .as_object_mut()
                .context("resource parameters must be an object")?;
            object.remove("resource_id");
            // Local adapter bindings cannot be redirected by request parameters.
            object.extend(resource.parameters.clone());
        }
        match capability {
            "node.configure" => {
                self.service
                    .as_ref()
                    .context("Device management is unavailable")?
                    .remote(action, params)
                    .await
            }
            "system.ping" => Ok(json!({
                "pong": true,
                "time": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64(),
                "runtime": "rust",
                "version": crate::VERSION,
            })),
            "system.info" => self.system_info(action).await,
            "filesystem" => self.filesystem(action, params).await,
            "process.exec" => self.process_exec(action, params).await,
            "application.control" | "launch_application" => {
                self.application_control(action, params).await
            }
            "http.request" => self.http_request(action, params).await,
            "stt.transcribe" => self.stt_transcribe(action, params).await,
            "network.wol" => self.wake_on_lan(action, params).await,
            "llm.infer" | "vlm.analyze" => self.infer(capability, params).await,
            "node.update" => self.node_update(action, params).await,
            other => {
                let plugin = self
                    .plugins
                    .get(other)
                    .with_context(|| format!("unknown capability '{other}'"))?;
                let runtime = self
                    .python
                    .as_ref()
                    .context("Python runtime is unavailable")?;
                runtime.invoke(plugin, action, params).await
            }
        }
    }

    async fn filesystem(&self, action: &str, params: Value) -> Result<Value> {
        let raw = params.get("path").and_then(Value::as_str).unwrap_or("");
        if raw.is_empty() {
            bail!("filesystem requires 'path'")
        }
        let feature = match action {
            "read" | "list" => "filesystem.read",
            "write" | "mkdir" => "filesystem.write",
            _ => bail!("unknown filesystem action '{action}'"),
        };
        let path = self.resolve(raw, self.policy.unrestricted(feature))?;
        match action {
            "read" => {
                if !self.policy.filesystem.read {
                    bail!("filesystem.read is not enabled on this node")
                }
                let bytes = read_bounded_file(&path, self.policy.filesystem.max_read_bytes).await?;
                let content = String::from_utf8(bytes)
                    .context("filesystem.read supports UTF-8 text files only")?;
                Ok(json!({"path": path, "content": content}))
            }
            "list" => {
                if !self.policy.filesystem.read {
                    bail!("filesystem.list is not enabled on this node")
                }
                let mut directory = tokio::fs::read_dir(&path).await?;
                let mut entries = Vec::new();
                let mut truncated = false;
                while let Some(entry) = directory.next_entry().await? {
                    if entries.len() >= self.policy.filesystem.max_list_entries {
                        truncated = true;
                        break;
                    }
                    entries.push(entry.path().to_string_lossy().into_owned());
                }
                entries.sort();
                Ok(json!({"path": path, "entries": entries, "truncated": truncated}))
            }
            "write" => {
                if !self.policy.filesystem.write {
                    bail!("filesystem.write is not enabled on this node")
                }
                let content = params.get("content").and_then(Value::as_str).unwrap_or("");
                if content.len() > self.policy.filesystem.max_write_bytes {
                    bail!("filesystem.write content exceeds the configured byte limit")
                }
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?
                }
                tokio::fs::write(&path, content).await?;
                Ok(json!({"path": path, "written": true}))
            }
            "mkdir" => {
                if !self.policy.filesystem.write {
                    bail!("filesystem.mkdir is not enabled on this node")
                }
                tokio::fs::create_dir_all(&path).await?;
                Ok(json!({"path": path, "created": true}))
            }
            _ => bail!("unknown filesystem action '{action}'"),
        }
    }

    fn resolve(&self, raw: &str, full_access: bool) -> Result<PathBuf> {
        let path = normalize_path(Path::new(raw))?;
        if full_access {
            return Ok(path);
        }
        if self.policy.filesystem.roots.is_empty() {
            bail!("filesystem access requires at least one allowed root")
        }
        if !path_is_scoped(&path, &self.policy.filesystem.roots)? {
            bail!("path '{}' outside allowed roots", path.display());
        }
        Ok(path)
    }

    async fn application_control(&self, action: &str, params: Value) -> Result<Value> {
        if !self.policy.applications.enabled {
            bail!("application control is not enabled on this node")
        }
        if action == "list" {
            let system = sysinfo::System::new_all();
            let mut processes = system
                .processes()
                .iter()
                .filter_map(|(pid, process)| {
                    let name = process.name().to_string_lossy().into_owned();
                    let executable = process.exe()?.to_string_lossy().into_owned();
                    (self.policy.unrestricted("application.control")
                        || is_allowed_program(&executable, &self.policy.applications.allowed))
                    .then(|| json!({"pid": pid.as_u32(), "name": name, "executable": executable}))
                })
                .collect::<Vec<_>>();
            processes.sort_by_key(|value| value["pid"].as_u64().unwrap_or(0));
            return Ok(json!({"processes": processes}));
        }
        let application = params
            .get("application")
            .or_else(|| params.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if application.is_empty() {
            bail!("application control requires 'application'")
        }
        if !self.policy.unrestricted("application.control")
            && !is_allowed_program(application, &self.policy.applications.allowed)
        {
            bail!("application '{application}' is not in the node allowlist")
        }
        if action == "terminate" {
            let pid = params
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .context("terminate requires a valid pid")?;
            let system = sysinfo::System::new_all();
            let process = system
                .process(sysinfo::Pid::from_u32(pid))
                .context("process is not running")?;
            let running = process.exe().context("process executable is unavailable")?;
            if !running.to_string_lossy().eq_ignore_ascii_case(application) {
                bail!("pid {pid} does not belong to '{application}'")
            }
            return Ok(json!({"pid": pid, "terminated": process.kill()}));
        }
        if action != "launch" {
            bail!("unknown application control action '{action}'")
        }
        let mut command = Command::new(application);
        if let Some(args) = params.get("args") {
            command.args(validated_arguments(args)?);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        let child = command
            .spawn()
            .with_context(|| format!("could not launch '{application}'"))?;
        Ok(json!({"launched": application, "pid": child.id()}))
    }

    async fn system_info(&self, action: &str) -> Result<Value> {
        if !matches!(action, "get" | "read" | "run") {
            bail!("unknown system.info action '{action}'")
        }
        crate::client::system_snapshot().await
    }

    async fn process_exec(&self, action: &str, params: Value) -> Result<Value> {
        if action != "run" || !self.policy.process.enabled {
            bail!("process.exec is not enabled for this action")
        }
        let program = params
            .get("program")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("process.exec requires 'program'")?;
        if !self.policy.unrestricted("process.exec")
            && !is_allowed_program(program, &self.policy.process.allowed_executables)
        {
            bail!("program '{program}' is not in the node allowlist")
        }
        let mut command = Command::new(program);
        if let Some(arguments) = params.get("args") {
            command.args(validated_arguments(arguments)?);
        }
        if let Some(raw) = params.get("cwd").and_then(Value::as_str) {
            command.current_dir(self.resolve(raw, self.policy.unrestricted("process.exec"))?);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let (mut child, tree) = crate::process_lifetime::spawn(&mut command)
            .with_context(|| format!("could not start allowed program '{program}'"))?;
        let mut tree = Some(tree);
        let stdout = child.stdout.take().context("process stdout unavailable")?;
        let stderr = child.stderr.take().context("process stderr unavailable")?;
        let limit = self.policy.process.max_output_bytes;
        let seconds = params
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(self.policy.process.max_seconds)
            .clamp(1, self.policy.process.max_seconds);
        // Readers belong to this future rather than detached Tokio tasks. The
        // timeout covers root wait AND pipe collection; cancellation drops both
        // readers, the root child and the Windows tree lifetime handle.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let collected = tokio::time::timeout_at(deadline, async {
            let wait = async {
                let result = child.wait().await;
                // A completed bounded command cannot leave a persistent child
                // holding its output pipes. Keep ownership until a bounded OS
                // observation; no new timeout/lease or PID-based termination.
                if let Some(tree) = tree.take() {
                    #[cfg(windows)]
                    tree.terminate_and_observe_until(deadline).await?;
                    drop(tree);
                }
                result.map_err(anyhow::Error::from)
            };
            let (status, stdout, stderr) = tokio::join!(
                wait,
                read_limited(stdout, limit),
                read_limited(stderr, limit)
            );
            Ok::<_, anyhow::Error>((status?, stdout?, stderr?))
        })
        .await;
        let (status, (stdout, stdout_truncated), (stderr, stderr_truncated)) = match collected {
            Ok(result) => result?,
            Err(_) => {
                drop(tree.take());
                let _ = child.kill().await;
                let _ = child.wait().await;
                bail!("allowed process exceeded the {seconds}-second timeout")
            }
        };
        Ok(json!({
            "success": status.success(),
            "exit_code": status.code(),
            "stdout": String::from_utf8_lossy(&stdout),
            "stderr": String::from_utf8_lossy(&stderr),
            "truncated": stdout_truncated || stderr_truncated,
            "process_tree": {
                "mode": if cfg!(windows) {"windows_owned_job"} else {"root_child_only"},
                "empty_observed": if cfg!(windows) {Some(true)} else {None::<bool>},
                "external_effects_stop_confirmed": false,
            },
        }))
    }

    async fn http_request(&self, action: &str, params: Value) -> Result<Value> {
        if action != "send" || !self.policy.http.enabled {
            bail!("http.request is not enabled for this action")
        }
        let raw_url = params
            .get("url")
            .and_then(Value::as_str)
            .context("http.request requires 'url'")?;
        let url = reqwest::Url::parse(raw_url)?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("only HTTP and HTTPS URLs are supported")
        }
        let host = url.host_str().context("URL has no host")?;
        if !self.policy.unrestricted("http.request")
            && !host_is_allowed(host, &self.policy.http.allowed_hosts)
        {
            bail!("HTTP host '{host}' is not in the node allowlist")
        }
        let request_client = restricted_http_client(
            &url,
            self.policy.http.allow_private_networks || self.policy.unrestricted("http.request"),
        )
        .await?;
        let method = params
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase();
        if !self.policy.unrestricted("http.request")
            && !self
                .policy
                .http
                .methods
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(&method))
        {
            bail!("HTTP method '{method}' is not in the node allowlist")
        }
        let mut request = request_client.request(method.parse()?, url);
        if let Some(headers) = params.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    "host" | "content-length"
                ) {
                    bail!("the '{name}' header is managed by the HTTP client")
                }
                request = request.header(
                    name,
                    value.as_str().context("header values must be strings")?,
                );
            }
        }
        if let Some(body) = params.get("body") {
            request = if let Some(text) = body.as_str() {
                request.body(text.to_owned())
            } else {
                request.json(body)
            };
        }
        let response = request
            .timeout(Duration::from_secs(
                params
                    .get("timeout_seconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 120),
            ))
            .send()
            .await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let (body, truncated) =
            collect_response(response, self.policy.http.max_response_bytes).await?;
        Ok(json!({
            "status": status.as_u16(),
            "success": status.is_success(),
            "content_type": content_type,
            "body": String::from_utf8_lossy(&body),
            "truncated": truncated,
            "redirect_followed": false,
        }))
    }

    async fn stt_transcribe(&self, action: &str, params: Value) -> Result<Value> {
        if !matches!(action, "run" | "transcribe") || !self.policy.stt.enabled {
            bail!("stt.transcribe is not enabled for this action")
        }
        let audio = if let Some(encoded) = params.get("audio_base64").and_then(Value::as_str) {
            base64::engine::general_purpose::STANDARD.decode(encoded)?
        } else if let Some(path) = params.get("audio_path").and_then(Value::as_str) {
            if !self.policy.filesystem.read {
                bail!("reading a speech audio file requires Read files permission")
            }
            read_bounded_file(
                &self.resolve(path, self.policy.unrestricted("filesystem.read"))?,
                25 * 1024 * 1024,
            )
            .await?
        } else {
            bail!("stt.transcribe requires audio_base64 or audio_path")
        };
        if audio.is_empty() || audio.len() > 25 * 1024 * 1024 {
            bail!("STT audio must be between 1 byte and 25 MB")
        }
        let endpoint = reqwest::Url::parse(&self.policy.stt.endpoint)?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            bail!("STT endpoint must use HTTP or HTTPS")
        }
        let filename = params
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("audio.wav");
        let part = reqwest::multipart::Part::bytes(audio)
            .file_name(filename.to_owned())
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new()
            .text("model", self.policy.stt.model.clone())
            .part("file", part);
        let mut request = self.http.post(endpoint).multipart(form);
        if !self.policy.stt.api_key_env.is_empty() {
            let token = std::env::var(&self.policy.stt.api_key_env)
                .with_context(|| format!("{} is not set", self.policy.stt.api_key_env))?;
            request = request.bearer_auth(token);
        }
        let response = request
            .timeout(Duration::from_secs(180))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(json!({
            "text": response.get("text").and_then(Value::as_str).unwrap_or(""),
            "provider_response": response,
        }))
    }

    async fn wake_on_lan(&self, action: &str, params: Value) -> Result<Value> {
        if !matches!(action, "send" | "wake") || !self.policy.wake_on_lan.enabled {
            bail!("network.wol is not enabled for this action")
        }
        let raw_mac = params
            .get("mac")
            .and_then(Value::as_str)
            .context("network.wol requires 'mac'")?;
        let mac = parse_mac(raw_mac)?;
        let allowed_mac = self
            .policy
            .wake_on_lan
            .allowed_macs
            .iter()
            .filter_map(|value| parse_mac(value).ok())
            .any(|allowed| allowed == mac);
        if !self.policy.unrestricted("network.wol") && !allowed_mac {
            bail!("MAC address is not in the Wake-on-LAN allowlist")
        }
        let broadcast = params
            .get("broadcast")
            .and_then(Value::as_str)
            .context("network.wol requires 'broadcast'")?
            .parse::<Ipv4Addr>()?;
        if !self.policy.unrestricted("network.wol")
            && !self
                .policy
                .wake_on_lan
                .broadcast_addresses
                .contains(&broadcast)
        {
            bail!("broadcast address is not in the Wake-on-LAN allowlist")
        }
        let port = params
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .context("network.wol requires a valid 'port'")?;
        if port == 0 {
            bail!("Wake-on-LAN port zero is invalid")
        }
        if !self.policy.unrestricted("network.wol")
            && !self.policy.wake_on_lan.ports.contains(&port)
        {
            bail!("UDP port is not in the Wake-on-LAN allowlist")
        }
        let packet = wol_packet(mac);
        let socket = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.set_broadcast(true)?;
        let sent = socket.send_to(&packet, (broadcast, port)).await?;
        Ok(json!({
            "sent": sent == packet.len(),
            "bytes": sent,
            "mac": format_mac(mac),
            "broadcast": broadcast,
            "port": port,
        }))
    }

    async fn infer(&self, capability: &str, params: Value) -> Result<Value> {
        self.infer_with_schema(capability, params, None, None).await
    }

    /// The schema is internally generated, not a public caller-supplied payload.
    async fn infer_with_schema(
        &self,
        capability: &str,
        params: Value,
        schema: Option<Value>,
        interpreter_sampling: Option<&crate::interpreter_sampling::InterpreterSampling>,
    ) -> Result<Value> {
        let backend = params
            .get("backend")
            .map(|value| {
                value
                    .as_str()
                    .filter(|backend| matches!(*backend, "ollama" | "llamacpp"))
                    .context("backend must be ollama or llamacpp")
            })
            .transpose()?;
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .context("model is required and must be selected in the node configuration")?;
        if let Some(server) = &self.model_server
            && server.slot(Some(model)).is_some()
            && backend != Some("ollama")
        {
            if !server
                .slot(Some(model))
                .unwrap()
                .selection_metadata()?
                .supports(capability)
            {
                bail!("This capability is not enabled for the selected local model")
            }
            return self
                .llamacpp(server, &params, schema, interpreter_sampling)
                .await;
        }
        if backend == Some("llamacpp") {
            bail!("model is not selected on the requested llama.cpp backend")
        }
        let metadata = self
            .ollama_models
            .get(model)
            .with_context(|| format!("model '{model}' is not selected on this node"))?;
        if !metadata.supports(capability) {
            bail!("This capability is not enabled for the selected Ollama model")
        }
        self.ollama(&params, schema, interpreter_sampling).await
    }

    pub(crate) async fn preview_interpretation(
        &self,
        text: &str,
        capabilities: &[String],
    ) -> std::result::Result<Value, &'static str> {
        let messages = crate::interpretation::messages(text, capabilities)
            .map_err(|_| "invalid_interpreter_context")?;
        self.interpretation_response(text, capabilities, messages)
            .await
    }

    pub(crate) async fn resolve_interpretation(
        &self,
        draft: &crate::request_drafts::DraftRecord,
        capabilities: &[String],
    ) -> std::result::Result<Value, &'static str> {
        let messages = crate::interpretation::clarification_messages(draft, capabilities)
            .map_err(|_| "invalid_clarification_context")?;
        self.interpretation_response(&draft.original_text, capabilities, messages)
            .await
    }

    async fn interpretation_response(
        &self,
        text: &str,
        capabilities: &[String],
        messages: Value,
    ) -> std::result::Result<Value, &'static str> {
        let schema = crate::interpretation::output_schema(capabilities)
            .map_err(|_| "invalid_interpreter_context")?;
        let response = self
            .selected_interpreter_response(messages, 512, Some(schema))
            .await?;
        let output = response["output"]
            .as_str()
            .ok_or("interpretation_invalid")?;
        let interpretation = crate::interpretation::validate(text, output.as_bytes(), capabilities)
            .map_err(crate::interpretation::rejection_engine_code)?;
        Ok(json!({"model":response["model"],"interpretation":interpretation}))
    }

    /// Produce only a bounded untrusted method/clarification candidate. Callers
    /// still need runtime invalidation, fresh context and atomic EEF checkpointing.
    pub async fn prepare_method_candidate(
        &self,
        job: &str,
        revision: u64,
        draft: &crate::request_drafts::DraftRecord,
        capabilities: &[String],
    ) -> std::result::Result<crate::request_preparation::PreparedMethod, &'static str> {
        crate::request_drafts::validate_id(job).map_err(|_| "invalid_preparation_source")?;
        if revision == 0
            || revision == u64::MAX
            || revision < (draft.proposals.len() + draft.answers.len()) as u64
        {
            return Err("invalid_preparation_source");
        }
        let messages = crate::request_preparation::messages(draft, capabilities)
            .map_err(|_| "invalid_preparation_context")?;
        let response = self
            .selected_interpreter_response(messages, 1024, None)
            .await?;
        crate::request_preparation::PreparedMethod::from_selected(
            job,
            revision,
            draft,
            capabilities,
            response["output"]
                .as_str()
                .ok_or("interpretation_invalid")?
                .as_bytes(),
            response["model"]["backend"]
                .as_str()
                .ok_or("interpretation_invalid")?,
            response["model"]["model_id"]
                .as_str()
                .ok_or("interpretation_invalid")?,
        )
        .map_err(|_| "method_proposal_invalid")
    }

    /// The same applied candidates used by interpretation; no inference/probe/load.
    pub(crate) fn interpreter_candidates(&self) -> Vec<(&'static str, String)> {
        let has_role = |metadata: &crate::model_metadata::ModelMetadata| {
            metadata.supports("llm.infer")
                && metadata
                    .roles
                    .as_ref()
                    .is_some_and(|roles| roles.iter().any(|role| role == "request_interpreter"))
        };
        let mut candidates = self
            .ollama_models
            .iter()
            .filter(|(_, metadata)| has_role(metadata))
            .map(|(id, _)| ("ollama", id.clone()))
            .collect::<Vec<_>>();
        if let Some(server) = &self.model_server {
            for slot in &server.slots {
                if slot
                    .selection_metadata()
                    .is_ok_and(|metadata| has_role(&metadata))
                {
                    candidates.push(("llamacpp", slot.model_id.clone()));
                }
            }
        }
        candidates.sort();
        candidates
    }

    async fn selected_interpreter_response(
        &self,
        messages: Value,
        max_tokens: u64,
        schema: Option<Value>,
    ) -> std::result::Result<Value, &'static str> {
        let candidates = self.interpreter_candidates();
        if candidates.is_empty() {
            return Err("interpreter_unavailable");
        }
        if candidates.len() != 1 {
            return Err("interpreter_ambiguous");
        }
        let (backend, model) = &candidates[0];
        let response = self
            .infer_with_schema(
                "llm.infer",
                json!({"backend":backend,"model":model,"messages":messages,
            "temperature":0,"max_tokens":max_tokens,"timeout":30}),
                schema,
                Some(&self.interpreter_sampling),
            )
            .await
            .map_err(|_| "interpreter_backend_failed")?;
        if response["completion_complete"] != true {
            return Err("interpretation_incomplete");
        }
        let output = response["content"]
            .as_str()
            .ok_or("interpretation_invalid")?;
        if output.len() > crate::interpretation::MAX_PROPOSAL_BYTES {
            return Err(crate::interpretation::RejectionReason::OutputTooLarge.engine_code());
        }
        Ok(json!({"model":{"backend":backend,"model_id":model},"output":output}))
    }

    fn ollama_payload(&self, params: &Value, schema: Option<Value>) -> Result<Value> {
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .context("model is required")?;
        let mut messages = params.get("messages").cloned().unwrap_or_else(|| json!([
            {"role": "user", "content": params.get("prompt").and_then(Value::as_str).unwrap_or("")}
        ]));
        if let (Some(images), Some(last)) = (
            params.get("images"),
            messages.as_array_mut().and_then(|items| items.last_mut()),
        ) && let Some(last) = last.as_object_mut()
        {
            last.insert("images".into(), images.clone());
        }
        let mut payload = json!({"model": model, "messages": messages, "stream": false,
            "options": {"temperature": params.get("temperature").and_then(Value::as_f64).unwrap_or(0.7),
                "num_predict": params.get("max_tokens").and_then(Value::as_u64).unwrap_or(1024)}});
        if let Some(seconds) = self.ollama_keep_alive_seconds {
            payload["keep_alive"] = json!(seconds);
        }
        if let Some(enabled) = self.thinking_mode.hint() {
            payload["think"] = json!(enabled);
        }
        if let Some(schema) = schema {
            payload["format"] = schema;
        }
        Ok(payload)
    }

    async fn ollama(
        &self,
        params: &Value,
        schema: Option<Value>,
        sampling: Option<&crate::interpreter_sampling::InterpreterSampling>,
    ) -> Result<Value> {
        let mut payload = self.ollama_payload(params, schema)?;
        if let Some(sampling) = sampling {
            Self::apply_interpreter_sampling(&mut payload, sampling, true);
        }
        let response = self
            .http
            .post(format!("{}/api/chat", self.ollama_url))
            .timeout(Duration::from_secs(
                params.get("timeout").and_then(Value::as_u64).unwrap_or(60),
            ))
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;
        crate::model_response::chat(response, crate::model_selection::Backend::Ollama).await
    }

    async fn llamacpp(
        &self,
        server: &ModelServer,
        params: &Value,
        schema: Option<Value>,
        sampling: Option<&crate::interpreter_sampling::InterpreterSampling>,
    ) -> Result<Value> {
        let model = params.get("model").and_then(Value::as_str);
        let _use_pin = match server.pin(model) {
            Ok(pin) => pin,
            Err(_) => {
                self.service
                    .as_ref()
                    .context("selected model not ready")?
                    .acquire_demand_model(server, model.context("explicit demand model required")?)
                    .await?
            }
        };
        let base = server
            .base_url(model)
            .context("no llama.cpp model configured")?;
        let mut payload = self.llamacpp_payload(params, schema);
        if let Some(sampling) = sampling {
            Self::apply_interpreter_sampling(&mut payload, sampling, false);
        }
        let response = self
            .http
            .post(format!("{base}/v1/chat/completions"))
            .timeout(Duration::from_secs(
                params.get("timeout").and_then(Value::as_u64).unwrap_or(60),
            ))
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;
        crate::model_response::chat(response, crate::model_selection::Backend::Llamacpp).await
    }

    fn llamacpp_payload(&self, params: &Value, schema: Option<Value>) -> Value {
        let images = params.get("images").and_then(Value::as_array);
        let raw_messages = params.get("messages").and_then(Value::as_array).cloned().unwrap_or_else(|| vec![json!({
            "role": "user", "content": params.get("prompt").and_then(Value::as_str).unwrap_or("")
        })]);
        let messages = raw_messages
            .into_iter()
            .map(|mut message| {
                if message.get("role").and_then(Value::as_str) == Some("user")
                    && let Some(images) = images
                {
                    let text = message.get("content").and_then(Value::as_str).unwrap_or("");
                    let mut parts = vec![json!({"type": "text", "text": text})];
                    parts.extend(images.iter().filter_map(Value::as_str).map(|image| {
                        json!({
                            "type": "image_url", "image_url": {"url": ensure_data_url(image)}
                        })
                    }));
                    message["content"] = Value::Array(parts);
                }
                message
            })
            .collect::<Vec<_>>();
        let mut payload = json!({"messages": messages,
            "temperature": params.get("temperature").and_then(Value::as_f64).unwrap_or(0.7),
            "max_tokens": params.get("max_tokens").and_then(Value::as_u64).unwrap_or(1024),"stream":false});
        if let Some(schema) = schema {
            payload["response_format"] = json!({"type":"json_object","schema":schema});
        }
        if let Some(enabled) = self.thinking_mode.hint() {
            payload["chat_template_kwargs"] = json!({"enable_thinking":enabled});
            if !enabled {
                payload["reasoning_effort"] = json!("none");
            }
        }
        payload
    }

    fn apply_interpreter_sampling(
        payload: &mut Value,
        sampling: &crate::interpreter_sampling::InterpreterSampling,
        ollama: bool,
    ) {
        let destination = if ollama {
            payload["options"].as_object_mut().unwrap()
        } else {
            payload.as_object_mut().unwrap()
        };
        destination.extend(sampling.options().as_object().unwrap().clone());
    }

    async fn node_update(&self, action: &str, params: Value) -> Result<Value> {
        if action == "status" {
            let selected = updater::installed_version(&self.install_dir)?;
            return Ok(
                json!({"schema_version":1,"report_type":"update_status","success":true,
                "current_version":crate::VERSION,"installed_version":selected,
                "restart_required":selected.as_deref().is_some_and(|v| v != crate::VERSION)}),
            );
        }
        if matches!(action, "apply" | "apply_explicit") && !self.policy.remote_updates {
            bail!("remote updates are not allowed; approve updates in the local node app")
        }
        if action == "apply_explicit" {
            let request: updater::ExplicitUpdate = serde_json::from_value(params)?;
            request.validate()?;
            let service = self
                .service
                .as_ref()
                .context("explicit update service unavailable")?;
            if request.expected_node_id != service.node_id {
                bail!("explicit update targets a different node")
            }
            let applied = updater::apply_explicit_for(
                &request,
                &self.install_dir,
                crate::VERSION,
                Duration::from_secs(120),
                "eefn",
                || service.require_remote_update_permission(),
            )
            .await?;
            let mut live = service.live.lock().unwrap();
            live["pending_restart"] = json!(true);
            live["update"]["installed_version"] = json!(applied.new_version);
            live["update"]["restart_required"] = json!(true);
            return Ok(json!({"schema_version":1,"report_type":"explicit_update",
                "success":true,"node_id":service.node_id,"request_id":request.request_id,
                "applied":applied,"automatic_policy_changed":false}));
        }
        let manifest = self
            .update_manifest
            .as_deref()
            .context("no owner-selected update feed configured")?;
        if params
            .get("manifest_url")
            .is_some_and(|value| value.as_str() != Some(manifest))
        {
            bail!("a remote request cannot replace the owner-selected update feed")
        }
        match action {
            "check" => Ok(serde_json::to_value(
                updater::check(manifest, &self.current_version, Duration::from_secs(30)).await?,
            )?),
            "apply" => Ok(serde_json::to_value(
                updater::apply(manifest, &self.install_dir, Duration::from_secs(120)).await?,
            )?),
            _ => bail!("unknown node.update action '{action}'"),
        }
    }
}

fn grant_policy_scope(policy: &NodePolicy, capability: &str, action: &str, section: &str) -> Value {
    let p = json!(policy);
    let full = if capability == "filesystem" {
        format!(
            "filesystem.{}",
            if action == "write" { "write" } else { "read" }
        )
    } else {
        capability.to_owned()
    };
    let scope = match (section, action) {
        ("filesystem", "write") => {
            json!({"enabled":policy.filesystem.write,"roots":policy.filesystem.roots,"max_bytes":policy.filesystem.max_write_bytes})
        }
        ("filesystem", _) => {
            json!({"enabled":policy.filesystem.read,"roots":policy.filesystem.roots,"max_bytes":policy.filesystem.max_read_bytes,"max_entries":policy.filesystem.max_list_entries})
        }
        ("media", _) => p["media"][match capability {
            "audio.capture" => "microphone",
            "audio.play" => "audio_output",
            "tts.speak" => "tts",
            "camera.capture" => "camera",
            "screen.capture" => "screen_capture",
            _ => "input_control",
        }]
        .clone(),
        _ => p[section].clone(),
    };
    json!({"scope":scope,"full_access":policy.unrestricted(&full)})
}
fn normalize_policy(mut policy: NodePolicy) -> Result<NodePolicy> {
    let scoped_filesystem = (policy.filesystem.read && !policy.unrestricted("filesystem.read"))
        || (policy.filesystem.write && !policy.unrestricted("filesystem.write"));
    if scoped_filesystem {
        policy.filesystem.roots = policy
            .filesystem
            .roots
            .into_iter()
            .map(|root| {
                let normalized = normalize_path(&root)?;
                normalized.canonicalize().with_context(|| {
                    format!(
                        "filesystem root '{}' must already exist",
                        normalized.display()
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if policy.filesystem.roots.is_empty() {
            bail!("filesystem permissions require at least one explicit root")
        }
    }
    policy.filesystem.max_read_bytes = policy
        .filesystem
        .max_read_bytes
        .clamp(1_024, 64 * 1024 * 1024);
    policy.filesystem.max_write_bytes = policy
        .filesystem
        .max_write_bytes
        .clamp(1_024, 64 * 1024 * 1024);
    policy.filesystem.max_list_entries = policy.filesystem.max_list_entries.clamp(1, 100_000);
    if policy.process.enabled
        && !policy.unrestricted("process.exec")
        && policy.process.allowed_executables.is_empty()
    {
        bail!("process execution requires a non-empty executable allowlist")
    }
    if policy.process.enabled && !policy.unrestricted("process.exec") {
        for executable in &policy.process.allowed_executables {
            let path = Path::new(executable);
            if !path.is_absolute() || !path.is_file() {
                bail!(
                    "allowed process executable '{}' must be an existing absolute file",
                    path.display()
                )
            }
        }
    }
    policy.process.max_seconds = policy.process.max_seconds.clamp(1, 3_600);
    policy.process.max_output_bytes = policy
        .process
        .max_output_bytes
        .clamp(1_024, 16 * 1024 * 1024);
    if policy.applications.enabled
        && !policy.unrestricted("application.control")
        && policy.applications.allowed.is_empty()
    {
        bail!("application control requires a non-empty application allowlist")
    }
    if policy.applications.enabled && !policy.unrestricted("application.control") {
        for executable in &policy.applications.allowed {
            let path = Path::new(executable);
            if !path.is_absolute() || !path.is_file() {
                bail!(
                    "allowed application '{}' must be an existing absolute file",
                    path.display()
                )
            }
        }
    }
    if policy.http.enabled
        && !policy.unrestricted("http.request")
        && policy.http.allowed_hosts.is_empty()
    {
        bail!("HTTP requests require a non-empty host allowlist")
    }
    policy.http.max_response_bytes = policy
        .http
        .max_response_bytes
        .clamp(1_024, 32 * 1024 * 1024);
    if policy.http.methods.is_empty() {
        policy.http.methods = default_http_methods();
    }
    if policy.stt.enabled
        && (policy.stt.endpoint.trim().is_empty() || policy.stt.model.trim().is_empty())
    {
        bail!("STT requires both an endpoint and an owner-selected model")
    }
    if policy.wake_on_lan.enabled && !policy.unrestricted("network.wol") {
        if policy.wake_on_lan.allowed_macs.is_empty()
            || policy.wake_on_lan.broadcast_addresses.is_empty()
            || policy.wake_on_lan.ports.is_empty()
        {
            bail!("Wake-on-LAN requires MAC, broadcast address, and port allowlists")
        }
        for mac in &policy.wake_on_lan.allowed_macs {
            parse_mac(mac).with_context(|| format!("invalid Wake-on-LAN MAC '{mac}'"))?;
        }
        if policy.wake_on_lan.ports.contains(&0) {
            bail!("Wake-on-LAN port zero is invalid")
        }
    }
    Ok(policy)
}

fn parse_mac(value: &str) -> Result<[u8; 6]> {
    let compact = value.replace([':', '-', '.'], "");
    if compact.len() != 12
        || !compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        bail!("MAC address must contain exactly 12 hexadecimal digits")
    }
    let bytes = hex::decode(compact)?;
    Ok(bytes.try_into().expect("validated MAC length"))
}

fn format_mac(mac: [u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn wol_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0_u8; 102];
    packet[..6].fill(0xff);
    for chunk in packet[6..].as_chunks_mut::<6>().0 {
        chunk.copy_from_slice(&mac);
    }
    packet
}

fn is_allowed_program(program: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|allowed| {
        allowed.eq_ignore_ascii_case(program)
            || Path::new(allowed)
                .canonicalize()
                .ok()
                .zip(Path::new(program).canonicalize().ok())
                .is_some_and(|(allowed, program)| {
                    allowed
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&program.to_string_lossy())
                })
    })
}

pub(crate) fn validated_arguments(value: &Value) -> Result<Vec<String>> {
    let arguments = value
        .as_array()
        .context("args must be an array of strings")?;
    if arguments.len() > 128 {
        bail!("args exceeds 128 entries")
    }
    let mut total = 0_usize;
    arguments
        .iter()
        .map(|value| {
            let argument = value.as_str().context("args must contain only strings")?;
            total = total.saturating_add(argument.len());
            if argument.len() > 8_192 || total > 65_536 {
                bail!("process arguments exceed the configured safety limit")
            }
            Ok(argument.to_owned())
        })
        .collect()
}

fn path_is_scoped(path: &Path, roots: &[PathBuf]) -> Result<bool> {
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent().context("path has no existing parent")?;
    }
    let resolved = existing.canonicalize()?;
    Ok(roots.iter().any(|root| resolved.starts_with(root)))
}

fn host_is_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim().to_ascii_lowercase();
        let host = host.to_ascii_lowercase();
        allowed == host
            || allowed
                .strip_prefix("*.")
                .is_some_and(|suffix| host.ends_with(&format!(".{suffix}")))
    })
}

async fn restricted_http_client(
    url: &reqwest::Url,
    allow_private: bool,
) -> Result<reqwest::Client> {
    if allow_private {
        return reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(Into::into);
    }
    let host = url.host_str().context("URL has no host")?;
    let port = url.port_or_known_default().context("URL has no port")?;
    let addresses = tokio::net::lookup_host((host, port)).await?;
    let mut public = None;
    for address in addresses {
        if is_non_public(address.ip()) {
            bail!("HTTP target resolves to a private or non-routable address")
        }
        public.get_or_insert(address);
    }
    let selected = public.context("HTTP target did not resolve")?;
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, selected)
        .build()?)
}

fn is_non_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let [first, second, _, _] = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_multicast()
                || address == Ipv4Addr::BROADCAST
                || (first == 100 && (64..=127).contains(&second))
                || first == 0
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_multicast()
                || address == Ipv6Addr::LOCALHOST
        }
    }
}

async fn read_limited<R>(reader: R, limit: usize) -> Result<(Vec<u8>, bool)>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    reader
        .take(u64::try_from(limit)? + 1)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

async fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if tokio::fs::metadata(path).await?.len() > u64::try_from(limit)? {
        bail!("file exceeds the configured byte limit")
    }
    Ok(tokio::fs::read(path).await?)
}

async fn collect_response(response: reqwest::Response, limit: usize) -> Result<(Vec<u8>, bool)> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, false))
}

fn ensure_data_url(image: &str) -> String {
    if image.starts_with("data:image")
        || image.starts_with("http://")
        || image.starts_with("https://")
        || image.starts_with("file://")
    {
        image.into()
    } else {
        format!("data:image/jpeg;base64,{image}")
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str())
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!("path escapes filesystem root")
                }
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resource_bindings_are_owner_selected_and_never_grant_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.txt");
        std::fs::write(&path, "owner binding").unwrap();
        let metadata:crate::context::NodeMetadata=serde_json::from_value(json!({"resources":[{"id":"file","capability":"filesystem","parameters":{"path":path}},{"id":"disabled","capability":"filesystem","available":false}]})).unwrap();
        let engine = NodeEngine::new(true, vec![dir.path().into()], dir.path().into())
            .unwrap()
            .with_resources("owner".into(), metadata.clone())
            .unwrap();
        let result = engine
            .execute(
                "filesystem",
                "read",
                json!({"resource_id":"owner::file","path":"wrong"}),
            )
            .await
            .unwrap();
        assert_eq!(result["content"], "owner binding");
        for id in ["other::file", "owner::disabled", "owner::missing"] {
            assert!(
                engine
                    .execute("filesystem", "read", json!({"resource_id":id}))
                    .await
                    .is_err()
            );
        }
        assert!(
            engine
                .execute("system.info", "run", json!({"resource_id":"owner::file"}))
                .await
                .is_err()
        );
        let denied = NodeEngine::new(false, vec![dir.path().into()], dir.path().into())
            .unwrap()
            .with_policy(NodePolicy::default())
            .unwrap()
            .with_resources("owner".into(), metadata)
            .unwrap();
        assert!(
            denied
                .execute("filesystem", "read", json!({"resource_id":"owner::file"}))
                .await
                .is_err()
        );
    }

    #[test]
    fn feature_grants_are_explicit_and_legacy_policies_do_not_widen() {
        let legacy: NodePolicy = serde_json::from_value(
            json!({"http":{"enabled":true,"allowed_hosts":["example.com"]}}),
        )
        .unwrap();
        assert!(!legacy.unrestricted("http.request"));
        assert!(!legacy.remote_updates);
        assert!(legacy.validate().is_ok());
        let broad: NodePolicy = serde_json::from_value(json!({
            "full_access":["filesystem.read","filesystem.write","process.exec","application.control","http.request","network.wol"],
            "filesystem":{"read":true,"write":true},"process":{"enabled":true},
            "applications":{"enabled":true},"http":{"enabled":true},"wake_on_lan":{"enabled":true}
        })).unwrap();
        assert!(broad.validate().is_ok());
    }

    #[tokio::test]
    async fn full_file_read_never_grants_write_or_bypasses_disabled_switch() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("fixture.txt");
        std::fs::write(&path, "fixture").unwrap();
        let mut policy = NodePolicy::default();
        policy.filesystem.roots = vec![dir.path().into()];
        policy.filesystem.read = true;
        policy.filesystem.write = true;
        policy.full_access = vec!["filesystem.read".into()];
        let engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_policy(policy.clone())
            .unwrap();
        assert_eq!(
            engine
                .filesystem("read", json!({"path":path}))
                .await
                .unwrap()["content"],
            "fixture"
        );
        assert!(
            engine
                .filesystem("write", json!({"path":path,"content":"changed"}))
                .await
                .is_err()
        );
        policy.filesystem.read = false;
        let engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_policy(policy)
            .unwrap();
        assert!(
            engine
                .filesystem("read", json!({"path":path}))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "fixture");
    }

    #[tokio::test]
    async fn explicit_update_checks_applied_and_current_grant_and_exact_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path.clone(), "fixture-node".into());
        let mut engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_service(service);
        let mut request = json!({"schema_version":1,"expected_node_id":"fixture-node",
            "request_id":uuid::Uuid::new_v4().to_string(),"version":"99.0.0-alpha.1",
            "url":"https://example.invalid/never-downloaded","sha256":"a".repeat(64),
            "size_bytes":100,"allow_prerelease":true});
        assert!(
            engine
                .node_update("apply_explicit", request.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
        engine.policy.remote_updates = true;
        request["expected_node_id"] = json!("another-node");
        assert!(
            engine
                .node_update("apply_explicit", request.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("different node")
        );
        request["expected_node_id"] = json!("fixture-node");
        // Applied grant is still true, but the owner has revoked it on disk.
        std::fs::write(&path, br#"{"permissions":{"remote_updates":false}}"#).unwrap();
        assert!(
            engine
                .node_update("apply_explicit", request.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
        std::fs::write(&path, "broken configuration").unwrap();
        assert!(engine.node_update("apply_explicit", request).await.is_err());
        assert!(!dir.path().join("versions").exists());
        std::fs::write(dir.path().join("current.txt"), "99.0.0-alpha.1\n").unwrap();
        let status = engine.node_update("status", json!({})).await.unwrap();
        assert_eq!(status["installed_version"], "99.0.0-alpha.1");
        assert_eq!(status["restart_required"], true);
        std::fs::write(dir.path().join("current.txt"), "../invalid").unwrap();
        assert!(engine.node_update("status", json!({})).await.is_err());
    }

    #[tokio::test]
    async fn remote_update_requires_grant_and_cannot_choose_feed() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_update_manifest(Some("https://example.invalid/owner-feed.json".into()));
        assert!(
            engine
                .node_update("apply", json!({}))
                .await
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
        engine.policy.remote_updates = true;
        assert!(
            engine
                .node_update(
                    "apply",
                    json!({"manifest_url":"https://example.invalid/other.json"})
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("cannot replace")
        );
    }

    #[tokio::test]
    async fn filesystem_is_permission_gated_and_rooted() {
        let dir = tempfile::tempdir().unwrap();
        let denied = NodeEngine::new(false, vec![dir.path().into()], dir.path().into()).unwrap();
        assert!(
            denied
                .execute(
                    "filesystem",
                    "write",
                    json!({"path": dir.path().join("x"), "content": "a"})
                )
                .await
                .is_err()
        );
        let allowed = NodeEngine::new(true, vec![dir.path().into()], dir.path().into()).unwrap();
        allowed
            .execute(
                "filesystem",
                "write",
                json!({"path": dir.path().join("x"), "content": "a"}),
            )
            .await
            .unwrap();
        let read = allowed
            .execute("filesystem", "read", json!({"path": dir.path().join("x")}))
            .await
            .unwrap();
        assert_eq!(read["content"], "a");
        assert!(
            allowed
                .execute(
                    "filesystem",
                    "read",
                    json!({"path": dir.path().join("../outside")})
                )
                .await
                .is_err()
        );
        assert!(!allowed.capabilities().contains(&"llm.infer".to_owned()));
        assert!(!allowed.capabilities().contains(&"vlm.analyze".to_owned()));
    }

    #[tokio::test]
    async fn explicit_model_backend_cannot_silently_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let engine = NodeEngine::new(false, vec![], dir.path().into()).unwrap();
        for backend in [json!("future"), json!(null), json!(1)] {
            let error = engine
                .infer("llm.infer", json!({"model":"m","backend":backend}))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("backend must be"));
        }
        let error = engine
            .infer("llm.infer", json!({"model":"m","backend":"llamacpp"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("requested llama.cpp backend"));
    }

    #[tokio::test]
    async fn model_capability_restrictions_are_enforced_without_calling_backend() {
        let dir = tempfile::tempdir().unwrap();
        let selected:crate::SelectedModel=serde_json::from_value(json!({"model_id":"m","modality":"vlm","selection":{"capabilities":["llm.infer"],"roles":["request_interpreter"]}})).unwrap();
        let engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_ollama("http://127.0.0.1:1".into(), [selected])
            .unwrap();
        assert!(engine.capabilities().contains(&"llm.infer".to_owned()));
        assert!(!engine.capabilities().contains(&"vlm.analyze".to_owned()));
        let error = engine
            .infer("vlm.analyze", json!({"model":"m","backend":"ollama"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not enabled"));
    }

    #[test]
    fn ollama_residency_is_applied_owner_preference_not_request_override() {
        let directory = tempfile::tempdir().unwrap();
        for seconds in [None, Some(-1), Some(0), Some(120), Some(86400)] {
            let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
                ollama_keep_alive_seconds: seconds,
                ..Default::default()
            };
            let engine = NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_ollama_residency(&policy)
                .unwrap();
            let schema = json!({"type":"object"});
            let payload = engine.ollama_payload(&json!({"model":"selected","prompt":"owner approved prompt",
                "images":["synthetic"],"keep_alive":999999,"ollama_keep_alive_seconds":999999,"max_tokens":16}),Some(schema.clone())).unwrap();
            assert_eq!(
                payload.get("keep_alive"),
                seconds.map(|v| json!(v)).as_ref()
            );
            assert_eq!(payload["model"], "selected");
            assert_eq!(payload["messages"][0]["content"], "owner approved prompt");
            assert_eq!(payload["messages"][0]["images"], json!(["synthetic"]));
            assert_eq!(payload["format"], schema);
            assert_eq!(payload["options"]["num_predict"], 16);
            assert_eq!(payload["stream"], false);
        }
    }

    #[test]
    fn interpreter_sampling_payloads_preserve_default_and_request_bounds() {
        use crate::interpreter_sampling::InterpreterSampling;
        let directory = tempfile::tempdir().unwrap();
        let engine = NodeEngine::new(false, vec![], directory.path().into()).unwrap();
        let params = json!({"model":"selected","prompt":"unchanged request","max_tokens":512,"temperature":0});
        let schema = json!({"type":"object"});
        for ollama in [false, true] {
            let old = if ollama {
                engine
                    .ollama_payload(&params, Some(schema.clone()))
                    .unwrap()
            } else {
                engine.llamacpp_payload(&params, Some(schema.clone()))
            };
            let mut default = old.clone();
            NodeEngine::apply_interpreter_sampling(
                &mut default,
                &InterpreterSampling::default(),
                ollama,
            );
            assert_eq!(default, old, "Legacy deterministic payload is unchanged");
            let chosen = InterpreterSampling {
                temperature_milli: 1000,
                top_p_milli: Some(1000),
                top_k: Some(20),
                min_p_milli: Some(0),
                presence_penalty_milli: Some(2000),
                repeat_penalty_milli: Some(1000),
                seed: Some(42),
            };
            chosen.validate().unwrap();
            let mut payload = old.clone();
            NodeEngine::apply_interpreter_sampling(&mut payload, &chosen, ollama);
            let options = if ollama {
                &payload["options"]
            } else {
                &payload
            };
            for (key, value) in chosen.options().as_object().unwrap() {
                assert_eq!(&options[key], value);
            }
            assert_eq!(payload["messages"], old["messages"]);
            if ollama {
                assert_eq!(payload["options"]["num_predict"], 512);
                assert_eq!(payload["format"], schema);
            } else {
                assert_eq!(payload["max_tokens"], 512);
                assert_eq!(payload["response_format"], old["response_format"]);
            }
            assert_eq!(payload["stream"], false);
        }
    }
    #[tokio::test]
    async fn applied_sampling_reaches_interpreter_but_not_ordinary_inference() {
        use crate::interpreter_sampling::InterpreterSampling;
        let (send, mut receive) = tokio::sync::mpsc::channel(1);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app=axum::Router::new().route("/api/chat",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
            let send=send.clone();async move {send.send(body).await.unwrap();
                axum::Json(json!({"message":{"content":"synthetic completion"},"done":true,"done_reason":"stop"}))}
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let selected: crate::SelectedModel =
            serde_json::from_value(json!({"model_id":"selected","modality":"text",
            "selection":{"roles":["request_interpreter"]}}))
            .unwrap();
        let sampling = InterpreterSampling {
            temperature_milli: 1000,
            top_p_milli: Some(1000),
            top_k: Some(20),
            min_p_milli: Some(0),
            presence_penalty_milli: Some(2000),
            repeat_penalty_milli: Some(1000),
            seed: Some(42),
        };
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            interpreter_sampling: sampling.clone(),
            ..Default::default()
        };
        let engine = NodeEngine::new(false, vec![], directory.path().into())
            .unwrap()
            .with_model_request_preferences(&policy)
            .unwrap()
            .with_ollama(format!("http://{address}"), [selected])
            .unwrap();
        let messages =
            crate::interpretation::messages("Check this node.", &["system.info".into()]).unwrap();
        let schema = json!({"type":"object"});
        engine
            .selected_interpreter_response(messages.clone(), 512, Some(schema.clone()))
            .await
            .unwrap();
        let actual = receive.recv().await.unwrap();
        assert_eq!(actual["messages"], messages);
        assert_eq!(actual["format"], schema);
        assert_eq!(actual["options"]["num_predict"], 512);
        for (key, value) in sampling.options().as_object().unwrap() {
            assert_eq!(&actual["options"][key], value);
        }
        engine.infer("llm.infer",json!({"model":"selected","backend":"ollama","prompt":"ordinary reviewed input",
            "temperature":0.25,"max_tokens":16,"interpreter_sampling":{"temperature_milli":0},"top_k":999,"seed":999})).await.unwrap();
        let ordinary = receive.recv().await.unwrap();
        assert_eq!(
            ordinary["options"],
            json!({"temperature":0.25,"num_predict":16})
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    #[test]
    fn thinking_hints_come_only_from_applied_owner_policy_for_both_backends() {
        use crate::model_lifecycle_policy::{ModelLifecyclePolicy, ThinkingMode};
        let directory = tempfile::tempdir().unwrap();
        let schema = json!({"type":"object","properties":{"answer":{"type":"string"}}});
        for mode in [
            ThinkingMode::Inherit,
            ThinkingMode::Disabled,
            ThinkingMode::Enabled,
        ] {
            let policy = ModelLifecyclePolicy {
                thinking_mode: mode,
                ..Default::default()
            };
            let engine = NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_model_request_preferences(&policy)
                .unwrap();
            let hostile = json!({"model":"arbitrary-model-not-matched-by-name","messages":[
                {"role":"system","content":"unchanged system"},{"role":"user","content":"unchanged input"}],
                "temperature":0,"max_tokens":24,"thinking_mode":"enabled","think":true,
                "chat_template_kwargs":{"enable_thinking":true},"reasoning_effort":"high","reasoning_format":"none"});
            for output_schema in [None, Some(schema.clone())] {
                let ollama = engine
                    .ollama_payload(&hostile, output_schema.clone())
                    .unwrap();
                let llama = engine.llamacpp_payload(&hostile, output_schema.clone());
                assert_eq!(ollama.get("think"), mode.hint().map(|v| json!(v)).as_ref());
                assert_eq!(
                    llama.get("chat_template_kwargs"),
                    mode.hint().map(|v| json!({"enable_thinking":v})).as_ref()
                );
                assert_eq!(
                    llama.get("reasoning_effort"),
                    (mode == ThinkingMode::Disabled)
                        .then(|| json!("none"))
                        .as_ref()
                );
                assert!(llama.get("reasoning_format").is_none());
                assert_eq!(ollama["messages"], hostile["messages"]);
                assert_eq!(llama["messages"], hostile["messages"]);
                assert_eq!(ollama["options"]["num_predict"], 24);
                assert_eq!(llama["max_tokens"], 24);
                assert_eq!(ollama.get("format"), output_schema.as_ref());
                assert_eq!(
                    llama.get("response_format"),
                    output_schema
                        .map(|s| json!({"type":"json_object","schema":s}))
                        .as_ref()
                );
                assert_eq!(engine.applied_thinking_mode(), mode);
            }
        }
    }
    #[tokio::test]
    async fn ollama_thinking_hint_reaches_backend_and_reasoning_is_not_final_content() {
        use crate::model_lifecycle_policy::{ModelLifecyclePolicy, ThinkingMode};
        let (send, mut receive) = tokio::sync::mpsc::channel(1);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let send = send.clone();
                async move {
                    send.send(body).await.unwrap();
                    axum::Json(
                        json!({"message":{"content":"final answer","thinking":"private reasoning"},
                        "done":true,"done_reason":"stop"}),
                    )
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        for mode in [
            ThinkingMode::Inherit,
            ThinkingMode::Disabled,
            ThinkingMode::Enabled,
        ] {
            let selected: crate::SelectedModel =
                serde_json::from_value(json!({"model_id":"selected","modality":"text"})).unwrap();
            let policy = ModelLifecyclePolicy {
                thinking_mode: mode,
                ..Default::default()
            };
            let engine = NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_model_request_preferences(&policy)
                .unwrap()
                .with_ollama(format!("http://{address}"), [selected])
                .unwrap();
            let reply = engine
                .infer(
                    "llm.infer",
                    json!({"backend":"ollama","model":"selected",
                "prompt":"public request","max_tokens":24,"think":true,"thinking_mode":"enabled"}),
                )
                .await
                .unwrap();
            let received = receive.recv().await.unwrap();
            assert_eq!(
                received.get("think"),
                mode.hint().map(|v| json!(v)).as_ref()
            );
            assert_eq!(received["options"]["num_predict"], 24);
            assert_eq!(reply["content"], "final answer");
            assert_eq!(reply["completion_complete"], true);
            assert!(!reply.to_string().contains("private reasoning"));
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
    #[test]
    fn invalid_ollama_owner_preference_never_creates_an_applied_engine() {
        let directory = tempfile::tempdir().unwrap();
        for seconds in [-2, 86401, i64::MIN, i64::MAX] {
            let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
                ollama_keep_alive_seconds: Some(seconds),
                ..Default::default()
            };
            assert!(
                NodeEngine::new(false, vec![], directory.path().into())
                    .unwrap()
                    .with_ollama_residency(&policy)
                    .is_err()
            );
        }
    }

    #[test]
    fn privileged_capabilities_are_default_deny() {
        let dir = tempfile::tempdir().unwrap();
        let engine = NodeEngine::new(false, vec![], dir.path().into()).unwrap();
        let capabilities = engine.capabilities();
        for capability in [
            "filesystem",
            "process.exec",
            "application.control",
            "http.request",
            "stt.transcribe",
            "network.wol",
        ] {
            assert!(!capabilities.contains(&capability.to_owned()));
        }
        assert!(capabilities.contains(&"system.info".to_owned()));
    }

    #[test]
    fn process_policy_requires_exact_existing_absolute_files() {
        let dir = tempfile::tempdir().unwrap();
        let relative = NodePolicy {
            process: ProcessPolicy {
                enabled: true,
                allowed_executables: vec!["cmd.exe".into()],
                ..ProcessPolicy::default()
            },
            ..NodePolicy::default()
        };
        assert!(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(relative)
                .is_err()
        );
    }

    #[test]
    fn http_host_allowlist_is_exact_or_explicit_wildcard() {
        let hosts = vec!["api.example.com".into(), "*.services.example".into()];
        assert!(host_is_allowed("api.example.com", &hosts));
        assert!(host_is_allowed("one.services.example", &hosts));
        assert!(!host_is_allowed("example.com", &hosts));
        assert!(!host_is_allowed("services.example", &hosts));
        assert!(!host_is_allowed("api.example.com.attacker.test", &hosts));
    }

    #[test]
    fn private_and_non_routable_addresses_are_detected() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.2.3",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(is_non_public(address.parse().unwrap()), "{address}");
        }
        assert!(!is_non_public("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn wake_on_lan_packet_and_mac_formats_are_validated() {
        let mac = parse_mac("01:23:45:67:89:ab").unwrap();
        assert_eq!(mac, [1, 0x23, 0x45, 0x67, 0x89, 0xab]);
        assert_eq!(parse_mac("01-23-45-67-89-AB").unwrap(), mac);
        assert!(parse_mac("01:23:45").is_err());
        let packet = wol_packet(mac);
        assert_eq!(&packet[..6], &[0xff; 6]);
        assert!(
            packet[6..]
                .as_chunks::<6>()
                .0
                .iter()
                .all(|chunk| *chunk == mac)
        );
    }
}
