//! Runtime-owned node state and operations; no HTTP or browser dependency.
use crate::NodePolicy;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
pub(crate) const SECRET_PLACEHOLDER: &str = "__KEEP_EXISTING_SECRET__";

/// Bounded startup diagnostics, not raw paths/errors or ongoing health claims.
#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupIssue {
    LlamacppStartupFailed,
    PythonRuntimeUnavailable,
    BuiltinPluginUnavailable,
    CustomPluginUnavailable,
}

#[derive(Clone)]
pub struct NodeService {
    pub(crate) config_path: PathBuf,
    pub(crate) node_id: String,
    pub live: Arc<std::sync::Mutex<Value>>,
    pub restart: Arc<tokio::sync::Notify>,
    pub submissions: Arc<crate::submission::SubmissionMailbox>,
    config_lock: Arc<std::sync::Mutex<()>>,
    started: std::time::Instant,
}

impl NodeService {
    pub fn record_startup_issue(&self, issue: StartupIssue) {
        let mut live = self.live.lock().unwrap();
        let mut issues = startup_issues(&live);
        if !issues.contains(&issue) {
            issues.push(issue);
        }
        live["startup_issues"] = json!(issues);
    }

    pub fn new(config_path: PathBuf, node_id: String) -> Arc<Self> {
        Arc::new(Self {
            config_path,
            node_id,
            live: Arc::new(std::sync::Mutex::new(
                json!({"connection":{"state":"starting"},"pending_restart":false,"download":{"state":"idle"}}),
            )),
            restart: Arc::new(tokio::sync::Notify::new()),
            submissions: Arc::new(crate::submission::SubmissionMailbox::default()),
            config_lock: Arc::new(std::sync::Mutex::new(())),
            started: std::time::Instant::now(),
        })
    }

    pub(crate) fn read_config(&self) -> Result<Value> {
        if !self.config_path.is_file() {
            return Ok(json!({
                "node_id": self.node_id,
                "psk": "",
                "endpoints": [],
                "permissions": NodePolicy::default(),
                "models": {
                    "provider": "auto",
                    "ollama": {"base_url": "http://127.0.0.1:11434", "selected": []},
                    "llamacpp": {"binary": "tools/llama-server.exe", "slots": []}
                }
            }));
        }
        let value: Value = serde_json::from_slice(&std::fs::read(&self.config_path)?)?;
        if !value.is_object() {
            bail!("node configuration must be a JSON object")
        }
        Ok(value)
    }

    pub(crate) fn save_config(&self, value: Value) -> Result<()> {
        let _guard = self.config_lock.lock().expect("config lock");
        self.save_config_locked(value)
    }

    fn save_config_locked(&self, mut value: Value) -> Result<()> {
        if !value.is_object() {
            bail!("node configuration must be a JSON object")
        }
        let old = self.read_config()?;
        if let Some(id) = old.get("node_id") {
            value["node_id"] = id.clone();
        }
        if value.get("name").is_some() {
            crate::setup::validate_name(value["name"].as_str().unwrap_or(""))?;
        }
        if value.get("psk").and_then(Value::as_str) == Some(SECRET_PLACEHOLDER) {
            if let Some(secret) = old.get("psk") {
                value["psk"] = secret.clone();
            } else if let Some(object) = value.as_object_mut() {
                object.remove("psk");
            }
        }
        validate_config(&value)?;
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if self.config_path.is_file() {
            std::fs::copy(
                &self.config_path,
                self.config_path.with_extension("json.bak"),
            )?;
        }
        let filename = self
            .config_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config.json");
        let next = self.config_path.with_file_name(format!("{filename}.next"));
        std::fs::write(&next, serde_json::to_vec_pretty(&value)?)?;
        std::fs::rename(&next, &self.config_path).or_else(|_| {
            std::fs::copy(&next, &self.config_path)?;
            std::fs::remove_file(&next)
        })?;
        Ok(())
    }

    pub fn discover_local(&self, path: &std::path::Path) -> Result<()> {
        let _guard = self.config_lock.lock().expect("config lock");
        if crate::setup::pair_local(path)? {
            self.restart.notify_one();
        }
        Ok(())
    }

    /// Local-owner configuration view, shared by HTTP and deterministic commands.
    pub fn configuration(&self) -> Result<Value> {
        let mut config = self.read_config()?;
        if config["psk"]
            .as_str()
            .is_some_and(|secret| !secret.is_empty())
        {
            config["psk"] = json!(SECRET_PLACEHOLDER);
        }
        Ok(
            json!({"config":config,"path":self.config_path.display().to_string(),
            "note":"Changes apply after EEFN restarts"}),
        )
    }

    pub fn connection_command(
        &self,
        request: crate::connection_commands::ConnectionRequest,
    ) -> Result<Value> {
        use crate::connection_commands::ConnectionCommand;
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("connection command schema or target node does not match this instance")
        }
        let restart_requested = !matches!(request.command, ConnectionCommand::Show {});
        match request.command {
            ConnectionCommand::Show {} => {}
            ConnectionCommand::Pause {} => {
                self.set_connection_paused(true)?;
            }
            ConnectionCommand::Resume {} => {
                self.set_connection_paused(false)?;
            }
            ConnectionCommand::PairLocal {} => {
                self.pair_with_local_coordinator()?;
            }
        }
        let _guard = self.config_lock.lock().expect("config lock");
        let config = self.read_config()?;
        let live = self.live.lock().unwrap();
        Ok(
            json!({"schema_version":1,"success":true,"report_type":"node_connection",
            "node_id":self.node_id,"saved_connection_enabled":config["connection_enabled"].as_bool().unwrap_or(true),
            "auto_local":config["auto_local"].as_bool().unwrap_or(true),
            "local_pairing":config["local_pairing"].as_bool().unwrap_or(true),
            "connection":live["connection"],"restart_requested":restart_requested,
            "note":"Saved connection policy is not completed reconnection. Local commands remain available; the requested node runtime restart also applies pending settings."}),
        )
    }

    pub fn status(&self) -> Value {
        let config = self.read_config().unwrap_or_else(|_| json!({}));
        let live = self.live.lock().unwrap().clone();
        json!({"node_id":self.node_id,"network":live["network"],"runtime":"rust","version":crate::VERSION,
            "node_alive":true,"coordinator_required_for_node":false,"coordinator_required_for_orchestration":true,
            "startup_issues":startup_issues(&live),
            "name":config["name"],"hostname":crate::setup::hostname(),"coordinator_name":config["coordinator_name"],"metadata":live["metadata"],
            "connection":live["connection"],"hardware":live["hardware"],"permissions":live["permissions"],"models":live["models"],
            "capabilities":live["capabilities"],"last_activity":live["last_activity"],"last_request_context":live["last_request_context"],
            "last_resource_id":live["last_resource_id"],"pending_restart":live["pending_restart"],"download":live["download"],"update":live["update"],
            "proposal_pending":self.config_path.with_extension("proposal.json").is_file()})
    }

    pub fn save_configuration(&self, value: Value) -> Result<Value> {
        self.save_config(value)?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        Ok(json!({"saved":true,"restart_required":true}))
    }

    pub fn set_connection_paused(&self, paused: bool) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        config["connection_enabled"] = json!(!paused);
        self.save_config_locked(config)?;
        self.restart.notify_one();
        Ok(json!({"paused":paused}))
    }

    pub fn restore_configuration(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let backup: Value =
            serde_json::from_slice(&std::fs::read(self.config_path.with_extension("json.bak"))?)?;
        self.save_config_locked(backup)?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        Ok(json!({"saved":true,"restart_required":true}))
    }

    pub fn reset_configuration(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = crate::setup::defaults();
        config["name"] = json!(crate::setup::hostname());
        self.save_config_locked(config)?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        Ok(json!({"saved":true,"restart_required":true}))
    }

    pub fn pair_with_local_coordinator(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        config["auto_local"] = json!(true);
        config["local_pairing"] = json!(true);
        config["endpoints"] = json!([]);
        self.save_config_locked(config)?;
        crate::setup::pair_local(&self.config_path)?;
        self.restart.notify_one();
        Ok(json!({"saved":true}))
    }

    pub fn configuration_proposal(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let mut value: Value = serde_json::from_slice(&std::fs::read(
            self.config_path.with_extension("proposal.json"),
        )?)?;
        if value.get("psk").is_some() {
            value["psk"] = json!(SECRET_PLACEHOLDER);
        }
        Ok(json!({"config":value}))
    }

    pub fn discard_configuration_proposal(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let path = self.config_path.with_extension("proposal.json");
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
        Ok(json!({"discarded":true}))
    }

    pub fn startup_status(&self) -> Result<crate::StartupStatus> {
        crate::startup_status("EEF Node")
    }

    pub fn set_startup(&self, enabled: bool) -> Result<crate::StartupStatus> {
        let current = std::env::current_exe()?;
        let stable = crate::updater::installation_root(&current)?.join("eefn.exe");
        let executable = if stable.is_file() { stable } else { current };
        crate::set_startup(
            "EEF Node",
            &executable,
            &["--config".into(), self.config_path.display().to_string()],
            enabled,
        )
    }

    pub async fn check_update(&self) -> Result<Value> {
        let config = self.read_config()?;
        let url = config
            .pointer("/update/manifest_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Configure an update manifest URL first"))?;
        Ok(json!(
            crate::updater::check(url, crate::VERSION, std::time::Duration::from_secs(30)).await?
        ))
    }

    pub async fn apply_update(&self) -> Result<Value> {
        let config = self.read_config()?;
        let url = config
            .pointer("/update/manifest_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Configure an update manifest URL first"))?;
        let root = crate::updater::installation_root(std::env::current_exe()?)?;
        if !crate::updater::check(url, crate::VERSION, std::time::Duration::from_secs(30))
            .await?
            .update_available
        {
            bail!("You are already up to date")
        }
        let result =
            crate::updater::apply_for(url, root, std::time::Duration::from_secs(600), "eefn")
                .await?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        Ok(json!(result))
    }

    /// Local owner operation shared by CLI and HTTP; no network lookup or LM.
    pub fn network_command(&self, request: crate::network::CommandRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("command schema or target node does not match this instance")
        }
        if matches!(request.command, crate::network::NetworkCommand::Diagnose) {
            return Ok(self.diagnostics());
        }
        if matches!(
            request.command,
            crate::network::NetworkCommand::Peers { .. }
        ) {
            bail!("peer inspection requires the running node's EEF connection")
        }
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        if config["node_id"].as_str() != Some(self.node_id.as_str()) {
            bail!("saved node identity does not match this instance")
        }
        let mut saved = false;
        if let crate::network::NetworkCommand::Set { changes } = request.command {
            changes.apply(&mut config)?;
            self.save_config_locked(config.clone())?;
            self.live.lock().unwrap()["pending_restart"] = json!(true);
            saved = true;
        }
        let network: crate::network::NetworkAdvertisement =
            serde_json::from_value(config.get("network").cloned().unwrap_or_else(|| json!({})))?;
        network.validate()?;
        let live = self.live.lock().unwrap();
        Ok(
            json!({"schema_version":1,"success":true,"node_id":self.node_id,
            "saved":saved,"display_name":config["name"],"network":network,
            "coordinator_endpoints":config.get("endpoints").cloned().unwrap_or_else(|| json!([])),
            "applied_network":live.get("network"),"applied_name":live.get("name"),
            "connection":live.get("connection"),"restart_required":live["pending_restart"],
            "note":"Advertisements are metadata only; no peer listener or coordinator trust is created"}),
        )
    }

    /// Local owner edits under the same lock as network/dashboard configuration.
    pub fn model_command(
        &self,
        request: crate::model_selection::ModelCommandRequest,
    ) -> Result<Value> {
        self.model_command_authorized(request, false)
    }

    fn model_command_authorized(
        &self,
        request: crate::model_selection::ModelCommandRequest,
        remote: bool,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("command schema or target node does not match this instance")
        }
        let _guard = self.config_lock.lock().expect("config lock");
        if !self.config_path.is_file() {
            bail!("saved node configuration is missing")
        }
        let mut config = self.read_config()?;
        if config["node_id"].as_str() != Some(self.node_id.as_str()) {
            bail!("saved node identity does not match this instance")
        }
        // Read and enforce current owner approval while holding the write lock.
        // A previous inspection or cached permission is never mutation authority.
        let remote_allowed = config
            .pointer("/management/allow_remote")
            .and_then(Value::as_bool)
            == Some(true);
        if remote
            && !remote_allowed
            && !matches!(
                request.command,
                crate::model_selection::ModelCommand::Show {}
            )
        {
            return Ok(
                json!({"schema_version":1,"success":false,"node_id":self.node_id,
                "error_code":"approval_required","changed":false,
                "note":"Enable management from EEF locally on this node before changing model selections."}),
            );
        }
        let changed = crate::model_selection::apply(&mut config, &request.command)?;
        let selections = crate::model_selection::selection_view(&config)?;
        let live = self.live.lock().unwrap();
        let registered = if let Some(models) = live.get("models") {
            let record = crate::network::PeerRecord::from_registration(
                &json!({"node_id":self.node_id,"models":models}),
            )?;
            Some(record.models.into_iter().map(|model|json!({"backend":model.backend,"model_id":model.model_id,"model_metadata":model.normalized_metadata()})).collect::<Vec<_>>())
        } else {
            None
        };
        drop(live);
        if changed {
            self.save_config_locked(config.clone())?;
            self.live.lock().unwrap()["pending_restart"] = json!(true);
        }
        let live = self.live.lock().unwrap();
        Ok(
            json!({"schema_version":1,"success":true,"report_type":"model_selections","node_id":self.node_id,
            "changed":changed,"saved_selections":selections,"registered_models":registered,
            "remote_management_allowed":remote_allowed,
            "provider":config.pointer("/models/provider").and_then(Value::as_str).unwrap_or("auto"),
            "restart_required":live["pending_restart"].as_bool().unwrap_or(false),
            "note":"Saved selection is not running-model readiness. Restart the node to apply changes; provider controls which backend is active. No model download/load or file deletion was performed."}),
        )
    }

    /// Explicit export only: omit names, addresses, paths, secrets and raw errors.
    pub fn diagnostics(&self) -> Value {
        let live = self.live.lock().unwrap();
        let state = live
            .pointer("/connection/state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let state = if matches!(
            state,
            "starting"
                | "waiting"
                | "paused"
                | "checking"
                | "connecting"
                | "connected"
                | "disconnected"
                | "retrying"
                | "error"
        ) {
            state
        } else {
            "unknown"
        };
        json!({"schema_version":1,"success":true,"report_type":"node_diagnostics",
            "version":crate::VERSION,"node_id":self.node_id,"metrics_available":true,"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
            "service_uptime_ms":self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            "runtime_id":live["runtime_id"].as_str().filter(|s|s.len()<=64),
            "connection_state":state,"connection_checks":live["connection_checks"].as_u64().unwrap_or(0),
            "successful_connections":live["successful_connections"].as_u64().unwrap_or(0),
            "failed_connection_attempts":live["failed_connection_attempts"].as_u64().unwrap_or(0),
            "pending_restart":live["pending_restart"].as_bool().unwrap_or(false),
            "model_count":live["models"].as_array().map(Vec::len).unwrap_or(0),
            "capability_count":live["capabilities"].as_array().map(Vec::len).unwrap_or(0),
            "startup_issues":startup_issues(&live),
            "privacy":"Includes stable node/runtime IDs for correlation. No automatic upload."})
    }

    pub async fn execute_network_command(
        self: &Arc<Self>,
        request: crate::network::CommandRequest,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("command schema or target node does not match this instance")
        }
        if let crate::network::NetworkCommand::Peers { query } = &request.command {
            query.validate()?;
            return self
                .submissions
                .request("network.peers", serde_json::to_value(query)?)
                .await;
        }
        self.network_command(request)
    }

    pub fn request_restart(
        self: &Arc<Self>,
        request: crate::local_commands::RestartRequest,
    ) -> Result<Value> {
        let current = self.live.lock().unwrap()["runtime_id"]
            .as_str()
            .map(str::to_owned);
        if request.schema_version != 1
            || request.expected_node_id != self.node_id
            || request.expected_runtime_id != current
        {
            bail!("restart command schema, node or runtime does not match this instance")
        }
        Ok(self.queue_restart())
    }

    /// Existing local-owner and approved remote restart share one runtime operation.
    pub fn queue_restart(self: &Arc<Self>) -> Value {
        let current = self.live.lock().unwrap()["runtime_id"].clone();
        let state = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            state.restart.notify_one();
        });
        json!({"schema_version":1,"success":true,"restarting":true,"restart_requested":true,"completed":false,
            "node_id":self.node_id,"previous_runtime_id":current,
            "note":"Restart requested, not completed. Other pending node settings will also apply."})
    }

    pub(crate) fn model_directory(&self) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("models")
    }

    pub(crate) fn with_remote_authority<T>(
        &self,
        operation: impl FnOnce(&Value) -> Result<T>,
    ) -> Result<T> {
        let _guard = self.config_lock.lock().expect("config lock");
        let config = self.read_config()?;
        require_remote_authority(&config)?;
        operation(&config)
    }

    fn save_remote_configuration(&self, changes: &Value) -> Result<Value> {
        if !changes.is_object() {
            bail!("node settings must be an object")
        }
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        let allowed = require_remote_authority(&config).is_ok();
        // Merge into the latest saved state while holding its lock. Never restore
        // an earlier approval value or overwrite unrelated concurrent local edits.
        for key in [
            "name",
            "permissions",
            "models",
            "update",
            "metadata",
            "network",
        ] {
            if let Some(value) = changes.get(key) {
                config[key] = value.clone();
            }
        }
        validate_config(&config)?;
        if !allowed {
            config["psk"] = json!(SECRET_PLACEHOLDER);
            crate::setup::write_json(&self.config_path.with_extension("proposal.json"), &config)?;
            return Ok(
                json!({"approval_required":true,"message":"Review and approve these changes in the node app."}),
            );
        }
        self.save_config_locked(config)?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        Ok(json!({"saved":true,"restart_required":true}))
    }

    /// Final download admission after asynchronous backend inspection. Does not
    /// cancel previously admitted downloads when approval later changes.
    pub(crate) fn begin_model_download(
        &self,
        snapshot: &Value,
        remote: bool,
        progress: Value,
    ) -> Result<()> {
        let _guard = self.config_lock.lock().expect("config lock");
        let current = self.read_config()?;
        if remote {
            require_remote_authority(&current)?;
        }
        if current.get("models") != snapshot.get("models")
            || current.get("model_catalog") != snapshot.get("model_catalog")
        {
            bail!(
                "Model settings changed during backend inspection. Inspect current settings before retrying."
            )
        }
        let mut live = self.live.lock().unwrap();
        if progress["id"].is_string() && live["download"]["id"] == progress["id"] {
            bail!("This download ID was already admitted. Inspect its status; do not replay it.")
        }
        if live["download"]["state"] == "downloading" {
            bail!("A model is already downloading. Wait or cancel it first.")
        }
        live["download"] = progress;
        Ok(())
    }

    pub async fn remote(self: &Arc<Self>, action: &str, params: Value) -> Result<Value> {
        let mut config = self.read_config()?;
        let allowed = config
            .pointer("/management/allow_remote")
            .and_then(Value::as_bool)
            == Some(true);
        match action {
            "model_command" => self.model_command_authorized(serde_json::from_value(params)?, true),
            "diagnostics" => Ok(self.diagnostics()),
            "get" => {
                config["psk"] = json!(SECRET_PLACEHOLDER);
                Ok(json!({"config":config,"remote_allowed":allowed}))
            }
            "status" => {
                let live = self.live.lock().unwrap().clone();
                Ok(
                    json!({"name":config["name"],"node_id":self.node_id,"version":crate::VERSION,"connection":live["connection"],"hardware":live["hardware"],"permissions":live["permissions"],"models":live["models"],"pending_restart":live["pending_restart"],"download":live["download"],"metadata":live["metadata"],"last_request_context":live["last_request_context"],"last_resource_id":live["last_resource_id"]}),
                )
            }
            "models" => crate::model_manager::list(self).await,
            "inspect" => crate::model_manager::inspect(self, params).await,
            "save" => self.save_remote_configuration(params.get("config").unwrap_or(&params)),
            "restart" => self.with_remote_authority(|_| Ok(self.queue_restart())),
            "install" => {
                crate::model_manager::install_remote(self.clone(), params).await?;
                Ok(json!({"started":true}))
            }
            "cancel" => self.with_remote_authority(|_| {
                Ok(json!({"requested":crate::model_manager::cancel(self, params["id"].as_str())?}))
            }),
            _ => bail!(
                "This device has not allowed remote management. Enable it in the device app’s Settings, or approve the proposed changes there."
            ),
        }
    }
}

fn validate_config(value: &Value) -> Result<()> {
    let network: crate::network::NetworkAdvertisement =
        serde_json::from_value(value.get("network").cloned().unwrap_or_else(|| json!({})))?;
    network.validate()?;
    let metadata: crate::context::NodeMetadata =
        serde_json::from_value(value.get("metadata").cloned().unwrap_or_else(|| json!({})))?;
    metadata.validate()?;
    if let Some(dashboard) = value.get("dashboard") {
        if !dashboard.is_object() {
            bail!("Dashboard settings must be an object")
        }
        for key in ["enabled", "ui_enabled"] {
            if dashboard.get(key).is_some_and(|value| !value.is_boolean()) {
                bail!("API and UI enabled settings must be boolean")
            }
        }
        if dashboard
            .get("port")
            .is_some_and(|p| !p.as_u64().is_some_and(|n| n > 0 && n <= 65535))
        {
            bail!("Dashboard port must be between 1 and 65535")
        }
    }
    if let Some(endpoints) = value.get("endpoints") {
        let endpoints: Vec<crate::CoordinatorEndpoint> = serde_json::from_value(endpoints.clone())?;
        for endpoint in endpoints {
            if endpoint.address.trim().is_empty() {
                bail!("Enter an EEF address or choose automatic connection")
            }
        }
    }
    if let Some(selected) = value.pointer("/models/ollama/selected") {
        let selected = serde_json::from_value::<Vec<crate::SelectedModel>>(selected.clone())?;
        let mut ids = std::collections::HashSet::new();
        for model in selected {
            model.validate()?;
            if model.model_id.trim().is_empty()
                || !ids.insert(model.model_id)
                || !matches!(model.modality.as_str(), "text" | "vlm")
            {
                bail!("Selected models need unique names and a text or vlm model type")
            }
        }
    }
    if let Some(slots) = value.pointer("/models/llamacpp/slots") {
        let slots = serde_json::from_value::<Vec<crate::ModelSlot>>(slots.clone())?;
        let mut ports = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for slot in slots {
            crate::model_selection::validate_id(&slot.model_id)?;
            slot.selection_metadata()?;
            if !ids.insert(slot.model_id.clone()) {
                bail!("local model IDs must be unique")
            }
            if slot.model_id.trim().is_empty() || !slot.model_path.is_file() {
                bail!("Choose an installed model file before saving")
            }
            if slot.port == 0 || !ports.insert(slot.port) {
                bail!("Each local model needs a different nonzero port")
            }
        }
    }
    crate::model_selection::selection_view(value)?;
    if value
        .get("endpoints")
        .is_some_and(|endpoints| !endpoints.is_array())
    {
        bail!("endpoints must be an array")
    }
    if value
        .get("endpoints")
        .and_then(Value::as_array)
        .is_some_and(|endpoints| !endpoints.is_empty())
        && value
            .get("psk")
            .and_then(Value::as_str)
            .is_none_or(|secret| secret.len() < 12)
    {
        bail!("a networked node requires a PSK of at least 12 characters")
    }
    if let Some(provider) = value.pointer("/models/provider").and_then(Value::as_str)
        && !matches!(provider, "auto" | "ollama" | "llamacpp")
    {
        bail!("models.provider must be auto, ollama, or llamacpp")
    }
    if let Some(permissions) = value.get("permissions") {
        serde_json::from_value::<NodePolicy>(permissions.clone())?.validate()?;
    }
    Ok(())
}

fn require_remote_authority(config: &Value) -> Result<()> {
    if config
        .pointer("/management/allow_remote")
        .and_then(Value::as_bool)
        != Some(true)
    {
        bail!(
            "This node has not allowed remote management. Enable it locally before requesting changes."
        )
    }
    Ok(())
}

fn startup_issues(live: &Value) -> Vec<StartupIssue> {
    live["startup_issues"]
        .as_array()
        .into_iter()
        .flatten()
        .take(4)
        .filter_map(|value| serde_json::from_value(value.clone()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_issue_reports_are_bounded_deduplicated_and_do_not_leak_errors() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("node.json"), "stable".into());
        service.live.lock().unwrap()["startup_issues"] = json!(["PRIVATE-PATH-ERROR"]);
        for _ in 0..20 {
            service.record_startup_issue(StartupIssue::LlamacppStartupFailed);
        }
        assert_eq!(
            service.diagnostics()["startup_issues"],
            json!(["llamacpp_startup_failed"])
        );
        assert_eq!(
            service.status()["startup_issues"],
            json!(["llamacpp_startup_failed"])
        );
        assert!(!service.diagnostics().to_string().contains("PRIVATE"));
    }

    #[test]
    fn diagnostic_projection_never_includes_raw_config_or_errors() {
        let dir = tempfile::tempdir().unwrap();
        let core = NodeService::new(dir.path().join("node.json"), "stable-node".into());
        *core.live.lock().unwrap() = json!({"name":"PRIVATE","runtime_id":"runtime-1","connection":{"state":"connected","address":"PRIVATE","last_error":"PRIVATE"},"hardware":{"name":"PRIVATE"},"models":[{"path":"PRIVATE"}],"successful_connections":2});
        let report = core.diagnostics();
        assert!(!report.to_string().contains("PRIVATE"));
        assert_eq!(report["node_id"], "stable-node");
        assert_eq!(report["model_count"], 1);
        assert_eq!(report["successful_connections"], 2);
    }

    #[test]
    fn network_commands_preserve_identity_endpoints_secrets_and_applied_state() {
        use crate::network::{CommandRequest, NetworkChanges, NetworkCommand};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        crate::setup::write_json(&path, &json!({"node_id":"device-stable","name":"Original", "psk":"private-network-secret", "endpoints":["127.0.0.1:51335"], "management":{"allow_remote":false}, "future":{"keep":true}})).unwrap();
        let core = NodeService::new(path.clone(), "device-stable".into());
        core.live.lock().unwrap()["network"] = json!({"advertised_address":"192.0.2.1"});
        let request = |command| CommandRequest {
            schema_version: 1,
            expected_node_id: "device-stable".into(),
            command,
        };
        let changed = core
            .network_command(request(NetworkCommand::Set {
                changes: NetworkChanges {
                    name: Some("Laptop".into()),
                    advertised_address: Some("26.1.2.3".into()),
                    ..Default::default()
                },
            }))
            .unwrap();
        assert_eq!(changed["node_id"], "device-stable");
        assert_eq!(changed["network"]["advertised_address"], "26.1.2.3");
        assert_eq!(
            changed["applied_network"]["advertised_address"],
            "192.0.2.1"
        );
        assert_eq!(changed["restart_required"], true);
        assert!(!changed.to_string().contains("private-network-secret"));
        let saved = core.read_config().unwrap();
        assert_eq!(saved["endpoints"], json!(["127.0.0.1:51335"]));
        assert_eq!(saved["future"], json!({"keep":true}));
        assert_eq!(saved["psk"], "private-network-secret");
        assert_eq!(saved["management"]["allow_remote"], false);
        let before = std::fs::read(&path).unwrap();
        assert!(
            core.network_command(request(NetworkCommand::Set {
                changes: NetworkChanges {
                    name: Some("Must not save".into()),
                    advertised_address: Some("http://bad".into()),
                    ..Default::default()
                }
            }))
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            core.network_command(CommandRequest {
                schema_version: 1,
                expected_node_id: "other".into(),
                command: NetworkCommand::Show
            })
            .is_err()
        );
        assert!(
            core.network_command(CommandRequest {
                schema_version: 2,
                expected_node_id: "device-stable".into(),
                command: NetworkCommand::Show
            })
            .is_err()
        );
        let cleared = core
            .network_command(request(NetworkCommand::Set {
                changes: NetworkChanges {
                    clear_advertised_address: true,
                    ..Default::default()
                },
            }))
            .unwrap();
        assert!(cleared["network"]["advertised_address"].is_null());
        assert_eq!(core.read_config().unwrap()["node_id"], "device-stable");
    }

    #[test]
    fn corrupt_configuration_is_not_replaced_by_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "broken {").unwrap();
        let core = NodeService::new(path.clone(), "node".into());
        assert!(core.save_config(json!({"name":"replacement"})).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "broken {");
    }

    #[test]
    fn connection_controls_and_configuration_work_without_http_or_dashboard() {
        use crate::connection_commands::{ConnectionCommand, ConnectionRequest};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let original = json!({"node_id":"stable","name":"Original","psk":"secret","future":{"keep":true},"connection_enabled":true});
        crate::setup::write_json(&path, &original).unwrap();
        let service = NodeService::new(path.clone(), "stable".into());
        let request = |command| ConnectionRequest {
            schema_version: 1,
            expected_node_id: "stable".into(),
            command,
        };
        let shown = service
            .connection_command(request(ConnectionCommand::Show {}))
            .unwrap();
        assert_eq!(shown["saved_connection_enabled"], true);
        assert_eq!(shown["restart_requested"], false);
        assert_eq!(
            service
                .connection_command(request(ConnectionCommand::Pause {}))
                .unwrap()["saved_connection_enabled"],
            false
        );
        assert_eq!(service.read_config().unwrap()["future"], original["future"]);
        assert_eq!(
            service.configuration().unwrap()["config"]["psk"],
            SECRET_PLACEHOLDER
        );
        assert_eq!(service.status()["node_id"], "stable");
        service.restore_configuration().unwrap();
        assert_eq!(service.read_config().unwrap(), original);
        let mut wrong = request(ConnectionCommand::Pause {});
        wrong.expected_node_id = "other".into();
        assert!(service.connection_command(wrong).is_err());
        assert_eq!(service.read_config().unwrap(), original);
        assert!(
            serde_json::from_value::<ConnectionCommand>(
                json!({"operation":"resume","permissions":true})
            )
            .is_err()
        );
        service
            .connection_command(request(ConnectionCommand::Resume {}))
            .unwrap();
        assert_eq!(service.read_config().unwrap()["psk"], "secret");
    }

    #[test]
    fn concurrent_connection_and_model_changes_preserve_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":"stable","name":"Original","future":{"keep":true}}),
        )
        .unwrap();
        let service = NodeService::new(path, "stable".into());
        let first = service.clone();
        let second = service.clone();
        let a = std::thread::spawn(move || first.set_connection_paused(true).unwrap());
        let b = std::thread::spawn(move || {
            second
                .model_command(crate::model_selection::ModelCommandRequest {
                    schema_version: 1,
                    expected_node_id: "stable".into(),
                    command: crate::model_selection::ModelCommand::Provider {
                        provider: "ollama".into(),
                    },
                })
                .unwrap()
        });
        a.join().unwrap();
        b.join().unwrap();
        let saved = service.read_config().unwrap();
        assert_eq!(saved["models"]["provider"], "ollama");
        assert_eq!(saved["connection_enabled"], false);
        assert_eq!(saved["future"]["keep"], true);
    }

    #[test]
    fn legacy_remote_settings_cannot_restore_revoked_approval_or_lose_local_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"stable","name":"Original","psk":"private","management":{"allow_remote":true}})).unwrap();
        let service = NodeService::new(path, "stable".into());
        let remote = service.clone();
        let local = service.clone();
        let gate = Arc::new(std::sync::Barrier::new(2));
        let remote_gate = gate.clone();
        let worker = std::thread::spawn(move || {
            remote_gate.wait();
            remote.save_remote_configuration(&json!({"name":"Remote","management":{"allow_remote":true},"psk":"replacement","node_id":"replacement"})).unwrap()
        });
        let guard = local.config_lock.lock().unwrap();
        gate.wait();
        let mut config = local.read_config().unwrap();
        config["management"]["allow_remote"] = json!(false);
        config["connection_enabled"] = json!(false);
        config["future"] = json!({"keep":true});
        local.save_config_locked(config.clone()).unwrap();
        drop(guard);
        assert_eq!(worker.join().unwrap()["approval_required"], true);
        assert_eq!(service.read_config().unwrap(), config);
        assert_eq!(
            service.configuration_proposal().unwrap()["config"]["psk"],
            SECRET_PLACEHOLDER
        );
        assert!(service.with_remote_authority(|_| Ok(())).is_err());
    }

    #[test]
    fn download_admission_rechecks_approval_and_model_snapshot_without_starting_transfer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let snapshot = json!({"node_id":"stable","management":{"allow_remote":true},"models":{"provider":"ollama"}});
        crate::setup::write_json(&path, &snapshot).unwrap();
        let service = NodeService::new(path, "stable".into());
        let progress = json!({"state":"downloading","id":"fixture"});
        let mut revoked = snapshot.clone();
        revoked["management"]["allow_remote"] = json!(false);
        service.save_config(revoked).unwrap();
        let before = service.live.lock().unwrap().clone();
        assert!(
            service
                .begin_model_download(&snapshot, true, progress.clone())
                .is_err()
        );
        assert_eq!(*service.live.lock().unwrap(), before);
        // Local ownership still permits explicit installation after remote revocation.
        service
            .begin_model_download(&snapshot, false, progress.clone())
            .unwrap();
        assert!(
            service
                .begin_model_download(&snapshot, false, progress.clone())
                .is_err()
        );
        service.live.lock().unwrap()["download"] = json!({"state":"idle"});
        let mut changed = snapshot.clone();
        changed["models"]["provider"] = json!("llamacpp");
        service.save_config(changed).unwrap();
        assert!(
            service
                .begin_model_download(&snapshot, true, progress.clone())
                .is_err()
        );
        assert!(
            service
                .begin_model_download(&snapshot, false, progress)
                .is_err()
        );
        assert_eq!(service.live.lock().unwrap()["download"]["state"], "idle");
    }

    #[tokio::test]
    async fn remote_management_requires_local_approval_and_cannot_grant_itself_trust() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        crate::setup::write_json(&path,&json!({"node_id":"stable","name":"Original","psk":"private-network-secret","endpoints":[]})).unwrap();
        let dashboard = NodeService::new(path.clone(), "stable".into());
        let response=dashboard.remote("save",json!({"config":{"name":"Proposed","node_id":"replacement","management":{"allow_remote":true}}})).await.unwrap();
        assert_eq!(response["approval_required"], true);
        assert_eq!(dashboard.read_config().unwrap()["name"], "Original");
        assert!(dashboard.remote("restart", json!({})).await.is_err());
        assert_eq!(
            dashboard.remote("get", json!({})).await.unwrap()["config"]["psk"],
            SECRET_PLACEHOLDER
        );
        let mut local = dashboard.read_config().unwrap();
        local["management"] = json!({"allow_remote":true});
        dashboard.save_config(local).unwrap();
        dashboard.remote("save",json!({"config":{"name":"Approved","node_id":"replacement","psk":"replacement-secret","management":{"allow_remote":false}}})).await.unwrap();
        let saved = dashboard.read_config().unwrap();
        assert_eq!(saved["name"], "Approved");
        assert_eq!(saved["node_id"], "stable");
        assert_eq!(saved["psk"], "private-network-secret");
        assert_eq!(saved["management"]["allow_remote"], true);
        let metadata = json!({"area":["Home","Study"],"resources":[{"id":"camera","capability":"camera.capture","parameters":{"device":1}}]});
        dashboard
            .remote("save", json!({"config":{"metadata":metadata}}))
            .await
            .unwrap();
        assert_eq!(dashboard.read_config().unwrap()["metadata"], metadata);
        assert!(
            dashboard
                .remote(
                    "save",
                    json!({"config":{"metadata":{"area":["invalid/path"]}}})
                )
                .await
                .is_err()
        );
        assert_eq!(dashboard.read_config().unwrap()["metadata"], metadata);
        let mut local = dashboard.read_config().unwrap();
        local["management"]["allow_remote"] = json!(false);
        dashboard.save_config(local).unwrap();
        let proposed = json!({"area":["Changed"]});
        assert_eq!(
            dashboard
                .remote("save", json!({"config":{"metadata":proposed}}))
                .await
                .unwrap()["approval_required"],
            true
        );
        assert_eq!(dashboard.read_config().unwrap()["metadata"], metadata);
        let proposal: Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("proposal.json")).unwrap())
                .unwrap();
        assert_eq!(proposal["metadata"], proposed);
    }

    #[test]
    fn rejects_unknown_model_provider() {
        assert!(validate_config(&json!({"dashboard":{"enabled":false,"ui_enabled":true}})).is_ok());
        assert!(validate_config(&json!({"dashboard":{"ui_enabled":"false"}})).is_err());
        assert!(validate_config(&json!({"models": {"provider": "magic"}})).is_err());
        assert!(validate_config(&json!({"models": {"provider": "auto"}})).is_ok());
        assert!(validate_config(&json!({"models":{"ollama":{"selected":[{"model_id":"fixture","modality":"unknown"}]}}})).is_err());
    }

    #[tokio::test]
    async fn typed_remote_models_recheck_owner_approval_and_preserve_unrelated_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let initial = json!({"node_id":"stable","psk":"private","future":{"keep":true},"management":{"allow_remote":false}});
        crate::setup::write_json(&path, &initial).unwrap();
        let service = NodeService::new(path.clone(), "stable".into());
        let request = |command: Value| json!({"schema_version":1,"expected_node_id":"stable","command":command});
        let change = request(
            json!({"operation":"select_ollama","model":{"model_id":"fixture","modality":"text"}}),
        );
        let show = service
            .remote("model_command", request(json!({"operation":"show"})))
            .await
            .unwrap();
        assert_eq!(show["remote_management_allowed"], false);
        assert!(!show.to_string().contains("private"));
        assert_eq!(
            service
                .remote("model_command", change.clone())
                .await
                .unwrap()["error_code"],
            "approval_required"
        );
        assert_eq!(service.read_config().unwrap(), initial);
        let mut allowed = initial.clone();
        allowed["management"]["allow_remote"] = json!(true);
        service.save_config(allowed).unwrap();
        assert_eq!(
            service
                .remote("model_command", change.clone())
                .await
                .unwrap()["changed"],
            true
        );
        assert_eq!(
            service
                .remote("model_command", change.clone())
                .await
                .unwrap()["changed"],
            false
        );
        let mut saved = service.read_config().unwrap();
        assert_eq!(saved["psk"], initial["psk"]);
        assert_eq!(saved["future"], initial["future"]);
        // A successful earlier inspection is not authority after owner revocation.
        assert_eq!(
            service
                .remote("model_command", request(json!({"operation":"show"})))
                .await
                .unwrap()["remote_management_allowed"],
            true
        );
        saved["management"]["allow_remote"] = json!(false);
        service.save_config(saved.clone()).unwrap();
        assert_eq!(
            service
                .remote(
                    "model_command",
                    request(json!({"operation":"remove","backend":"ollama","model_id":"fixture"}))
                )
                .await
                .unwrap()["error_code"],
            "approval_required"
        );
        assert_eq!(service.read_config().unwrap(), saved);
        let mut wrong = change;
        wrong["expected_node_id"] = json!("other");
        assert!(service.remote("model_command", wrong).await.is_err());
        let mut spoof = request(json!({"operation":"show"}));
        spoof["remote"] = json!(false);
        assert!(service.remote("model_command", spoof).await.is_err());
    }

    #[test]
    fn dashboard_save_preserves_redacted_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, br#"{"psk":"secret-value","endpoints":[]}"#).unwrap();
        let dashboard = NodeService::new(path.clone(), "node".into());
        dashboard
            .save_config(json!({"psk": SECRET_PLACEHOLDER, "endpoints": []}))
            .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["psk"], "secret-value");
    }
}
