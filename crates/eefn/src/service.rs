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
    PeerRuntimeUnavailable,
}

#[derive(Clone)]
pub struct NodeService {
    pub(crate) config_path: PathBuf,
    pub(crate) node_id: String,
    pub live: Arc<std::sync::Mutex<Value>>,
    pub restart: Arc<tokio::sync::Notify>,
    pub submissions: Arc<crate::submission::SubmissionMailbox>,
    pub(crate) preview: Arc<crate::request_preview::PreviewRuntime>,
    pub(crate) model_runtime:
        Arc<std::sync::Mutex<Option<Arc<crate::model_lifecycle::ModelRuntime>>>>,
    config_lock: Arc<std::sync::Mutex<()>>,
    pub(crate) local_enrollment: Arc<tokio::sync::Mutex<()>>,
    pub(crate) installed_selection: Arc<tokio::sync::Mutex<()>>,
    pub(crate) download_receipts: Arc<std::sync::Mutex<crate::download_receipts::Journal>>,
    started: std::time::Instant,
}

impl NodeService {
    pub(crate) fn save_input_policy(
        &self,
        prepare_on_input: Option<bool>,
        resolve_on_answer: Option<bool>,
    ) -> Result<bool> {
        let _guard = self.config_lock.lock().expect("config lock");
        if prepare_on_input.is_none() && resolve_on_answer.is_none() {
            bail!("Choose at least one input workflow policy field")
        }
        let mut config = self.read_config()?;
        let before: crate::request_input::InputPolicy = serde_json::from_value(
            config
                .get("input_workflow")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        let policy = crate::request_input::InputPolicy {
            prepare_on_input: prepare_on_input.unwrap_or(before.prepare_on_input),
            resolve_on_answer: resolve_on_answer.unwrap_or(before.resolve_on_answer),
        };
        if before == policy {
            return Ok(false);
        }
        config["input_workflow"] = json!(policy);
        self.save_config_locked(config)?;
        Ok(true)
    }
    pub(crate) fn inspect_grant_configuration<T>(
        &self,
        check: impl FnOnce(&Value) -> Result<T>,
    ) -> Result<T> {
        let _guard = self.config_lock.lock().expect("config lock");
        check(&self.read_config()?)
    }
    pub(crate) fn guard_grant_configuration(
        &self,
        check: impl FnOnce(&Value) -> Result<()>,
    ) -> Result<std::sync::MutexGuard<'_, ()>> {
        let guard = self.config_lock.lock().expect("config lock");
        check(&self.read_config()?)?;
        Ok(guard)
    }
    pub(crate) fn require_remote_update_permission(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        let guard = self.config_lock.lock().expect("config lock");
        if self.read_config()?.pointer("/permissions/remote_updates") != Some(&json!(true)) {
            bail!("remote updates are not allowed; approve updates in the local node app")
        }
        Ok(guard)
    }

    pub fn record_startup_issue(&self, issue: StartupIssue) {
        let mut live = self.live.lock().unwrap();
        let mut issues = startup_issues(&live);
        if !issues.contains(&issue) {
            issues.push(issue);
        }
        live["startup_issues"] = json!(issues);
    }

    pub fn new(config_path: PathBuf, node_id: String) -> Arc<Self> {
        let (download_receipts, recovered) =
            crate::download_receipts::Journal::recover(&config_path, &node_id);
        Arc::new(Self {
            config_path,
            node_id,
            model_runtime: Arc::new(std::sync::Mutex::new(None)),
            live: Arc::new(std::sync::Mutex::new(
                json!({"connection":{"state":"starting"},"pending_restart":false,"download":recovered.unwrap_or_else(||json!({"state":"idle"}))}),
            )),
            restart: Arc::new(tokio::sync::Notify::new()),
            submissions: Arc::new(crate::submission::SubmissionMailbox::default()),
            preview: Arc::new(crate::request_preview::PreviewRuntime::default()),
            config_lock: Arc::new(std::sync::Mutex::new(())),
            local_enrollment: Arc::new(tokio::sync::Mutex::new(())),
            installed_selection: Arc::new(tokio::sync::Mutex::new(())),
            download_receipts: Arc::new(std::sync::Mutex::new(download_receipts)),
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

    pub(crate) fn save_config_locked(&self, mut value: Value) -> Result<()> {
        if !value.is_object() {
            bail!("node configuration must be a JSON object")
        }
        let old = self.read_config()?;
        // Backend-owned version survives generic saves/restore as well as the
        // typed policy command. Content rollback must not revive an old CAS.
        crate::model_bootstrap::preserve_revision(&old, &mut value)?;
        let previous_policy =
            crate::model_lifecycle_policy::ModelLifecyclePolicy::from_config(&old)?;
        let incoming_policy =
            serde_json::from_value::<crate::model_lifecycle_policy::ModelLifecyclePolicy>(
                value
                    .get("model_lifecycle")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
            )?;
        incoming_policy.validate()?;
        let previous_revision =
            crate::model_lifecycle_policy::ModelLifecyclePolicy::revision_from_config(&old)?;
        let revision = if previous_policy != incoming_policy {
            previous_revision.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!("Model policy revision exhausted; no wrap or reset")
            })?
        } else {
            previous_revision
        };
        if revision != 0 || old.get("model_lifecycle_revision").is_some() {
            value["model_lifecycle_revision"] = json!(revision);
        } else if let Some(object) = value.as_object_mut() {
            object.remove("model_lifecycle_revision");
        }
        if value.get("auth_mode").is_none() {
            if let Some(mode) = old.get("auth_mode") {
                value["auth_mode"] = mode.clone();
            }
        }
        if value.get("coordinator_certificate").is_none() {
            if let Some(certificate) = old.get("coordinator_certificate") {
                value["coordinator_certificate"] = certificate.clone();
            }
        }
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
        validate_config_shape(&value)?;
        validate_model_references(&value, &old, &self.config_path)?;
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
        let restart_requested = matches!(
            request.command,
            ConnectionCommand::Pause {}
                | ConnectionCommand::Resume {}
                | ConnectionCommand::PairLocal {}
                | ConnectionCommand::Pair { .. }
        );
        match request.command {
            ConnectionCommand::Show {} => {}
            ConnectionCommand::Limits { work, control } => {
                return self.request_limits_command(work, control);
            }
            ConnectionCommand::Pause {} => {
                self.set_connection_paused(true)?;
            }
            ConnectionCommand::Resume {} => {
                self.set_connection_paused(false)?;
            }
            ConnectionCommand::PairLocal {} => {
                self.pair_with_local_coordinator()?;
            }
            ConnectionCommand::Pair { code_file } => {
                self.pair_with_credential_file(&code_file)?;
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
            "saved_auth_mode":config["auth_mode"].as_str().unwrap_or("legacy"),
            "saved_coordinator_fingerprint":config["coordinator_certificate"].as_str()
                .and_then(|certificate|crate::tls::certificate_fingerprint(certificate).ok()),
            "connection":live["connection"],"restart_requested":restart_requested,
            "note":"Saved connection policy is not completed reconnection. Local commands remain available; the requested node runtime restart also applies pending settings."}),
        )
    }

    pub fn status(&self) -> Value {
        let saved = self.read_config();
        let config_available = saved.is_ok();
        let config = saved.unwrap_or_else(|_| json!({}));
        let live = self.live.lock().unwrap().clone();
        let bridge: Option<crate::python::BridgeLimits> = serde_json::from_value(
            config
                .get("python_bridge")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )
        .ok()
        .filter(|limits: &crate::python::BridgeLimits| {
            config_available && limits.validate().is_ok()
        });
        json!({"node_id":self.node_id,"network":live["network"],"runtime":"rust","version":crate::VERSION,
            "python_bridge":{"saved":bridge,"saved_valid":bridge.is_some(),"applied":live["python_bridge"],
                "enabled":live["python_bridge"].is_object(),"restart_required":live["python_bridge"].is_object() && json!(bridge)!=live["python_bridge"]},
            "peer_transport":live["peer_transport"],"peer_advertisement":live["peer_transfer"],
            "registration":crate::registration::State::observe(&live,&self.node_id,self.submissions.state()),
            "node_alive":true,"coordinator_required_for_node":false,"coordinator_required_for_orchestration":true,
            "startup_issues":startup_issues(&live),
            "name":config["name"],"hostname":crate::setup::hostname(),"coordinator_name":config["coordinator_name"],"metadata":live["metadata"],
            "connection":live["connection"],"input_channel":self.submissions.state(),"hardware":live["hardware"],"permissions":live["permissions"],"models":live["models"],
            "capabilities":live["capabilities"],"last_activity":live["last_activity"],"last_request_context":live["last_request_context"],
            "last_resource_id":live["last_resource_id"],"pending_restart":live["pending_restart"],"download":live["download"],"update":live["update"],
            "request_limits":live["request_limits"],"busy_rejections":live["busy_rejections"].as_u64().unwrap_or(0),
            "execution_grants":live["execution_grants"],"last_grant":live["last_grant"],"last_workload":live["last_workload"],
            "proposal_pending":self.config_path.with_extension("proposal.json").is_file()})
    }

    fn request_limits_command(&self, work: Option<usize>, control: Option<usize>) -> Result<Value> {
        use crate::request_limits::RequestLimits;
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        let original: RequestLimits = serde_json::from_value(
            config
                .get("request_limits")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        original.validate()?;
        let limits = RequestLimits {
            work: work.unwrap_or(original.work),
            control: control.unwrap_or(original.control),
        };
        limits.validate()?;
        let changed = limits != original;
        if changed {
            config["request_limits"] = json!(limits);
            self.save_config_locked(config)?;
        }
        let mut live = self.live.lock().unwrap();
        if changed {
            live["pending_restart"] = json!(true);
        }
        let applied = serde_json::from_value::<RequestLimits>(live["request_limits"].clone())
            .ok()
            .filter(|value| value.validate().is_ok());
        Ok(
            json!({"schema_version":1,"success":true,"report_type":"node_request_limits","node_id":self.node_id,
            "changed":changed,"saved_request_limits":limits,"applied_request_limits":applied,
            "restart_requested":false,"restart_required":live["pending_restart"].as_bool().unwrap_or(false),
            "note":"Saved limits apply after an explicit node restart. No work was stopped and no restart was requested."}),
        )
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

    pub(crate) fn save_credential_pair(
        &self,
        address: &str,
        secret: &str,
        certificate: &str,
    ) -> Result<()> {
        self.save_credential_pair_for_runtime(address, secret, certificate, None)
            .map(|_| ())
    }

    pub(crate) fn save_credential_pair_for_runtime(
        &self,
        address: &str,
        secret: &str,
        certificate: &str,
        expected_runtime: Option<&str>,
    ) -> Result<bool> {
        self.save_credential_pair_checked(address, secret, certificate, expected_runtime, None)
    }
    pub(crate) fn save_credential_pair_checked(
        &self,
        address: &str,
        secret: &str,
        certificate: &str,
        expected_runtime: Option<&str>,
        expected_connection: Option<&Value>,
    ) -> Result<bool> {
        let _guard = self.config_lock.lock().expect("config lock");
        if expected_runtime
            .is_some_and(|runtime| self.live.lock().unwrap()["runtime_id"] != runtime)
        {
            bail!("pairing command runtime changed")
        }
        let mut config = self.read_config()?;
        if expected_connection
            .is_some_and(|snapshot| crate::enrollment::connection_snapshot(&config) != *snapshot)
        {
            bail!("node connection settings changed; enrollment was not applied")
        }
        let connection_enabled = config["connection_enabled"].as_bool().unwrap_or(true);
        if expected_runtime.is_some() && config["node_id"] != self.node_id {
            bail!("saved node identity changed; pairing was not applied")
        }
        config["auth_mode"] = json!("node_credential");
        config["psk"] = json!(secret);
        config["coordinator_certificate"] = json!(certificate);
        config["endpoints"] = json!([{"address":address,"priority":100}]);
        config["auto_local"] = json!(false);
        config["local_pairing"] = json!(false);
        config["coordinator_name"] = Value::Null;
        self.save_config_locked(config)?;
        self.live.lock().unwrap()["pending_restart"] = json!(true);
        self.restart.notify_one();
        Ok(connection_enabled)
    }

    pub fn pair_with_local_coordinator(&self) -> Result<Value> {
        let _guard = self.config_lock.lock().expect("config lock");
        let mut config = self.read_config()?;
        if config["auth_mode"] == "node_credential" {
            bail!(
                "independently paired nodes require an exact-node invitation; local shared-key pairing was not applied"
            )
        }
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
        self.model_command_checked(request, remote, None)
    }

    /// Hashing is asynchronous and precedes the short, snapshot-checked save.
    pub async fn dispatch_model_command(
        &self,
        request: crate::model_selection::ModelCommandRequest,
        remote: bool,
    ) -> Result<Value> {
        let crate::model_selection::ModelCommand::SelectInstalledGguf {
            model_id,
            interpreter,
            request_id,
            activate_backend,
        } = &request.command
        else {
            return self.model_command_authorized(request, remote);
        };
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("command schema or target node does not match this instance")
        }
        if request_id.is_some() && !crate::installed_selection::correlated(&request.command) {
            bail!("installed selection request ID must be a canonical non-nil UUID")
        }
        if *activate_backend && !*interpreter {
            bail!("backend activation requires explicit interpreter assignment")
        }
        if !self.config_path.is_file() {
            bail!("saved node configuration is missing")
        }
        let snapshot = self.inspect_grant_configuration(|config| Ok(config.clone()))?;
        if snapshot["node_id"] != self.node_id {
            bail!("saved node identity does not match this instance")
        }
        if remote && snapshot.pointer("/management/allow_remote") != Some(&json!(true)) {
            return Ok(self.model_approval_required());
        }
        let Ok(_verification) = self.installed_selection.try_lock() else {
            return crate::installed_selection::busy_rejection(&self.node_id, &request.command);
        };
        let prepared = crate::installed_selection::prepare(
            self.model_directory(),
            snapshot,
            model_id.clone(),
            *interpreter,
        )
        .await;
        // This barrier precedes model_command_checked and every configuration
        // write. Later save/transport failures deliberately remain uncertain.
        let mut prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return crate::installed_selection::rejection(
                    &self.node_id,
                    &request.command,
                    error,
                );
            }
        };
        prepared.activate_backend = *activate_backend;
        let resolved = crate::model_selection::ModelCommandRequest {
            schema_version: request.schema_version,
            expected_node_id: request.expected_node_id,
            command: crate::model_selection::ModelCommand::SelectGguf {
                slot: prepared.slot.clone(),
            },
        };
        let mut result = self.model_command_checked(resolved, remote, Some(&prepared))?;
        if result["success"] == true {
            result["installed_artifact"] = prepared.proof();
        }
        Ok(result)
    }

    fn model_approval_required(&self) -> Value {
        json!({"schema_version":1,"success":false,"node_id":self.node_id,
            "error_code":"approval_required","changed":false,
            "note":"Enable management from EEF locally on this node before changing model selections."})
    }

    fn model_command_checked(
        &self,
        request: crate::model_selection::ModelCommandRequest,
        remote: bool,
        prepared: Option<&crate::installed_selection::Prepared>,
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
            return Ok(self.model_approval_required());
        }
        if let Some(prepared) = prepared {
            if config.get("models") != prepared.snapshot.get("models")
                || config.get("model_catalog") != prepared.snapshot.get("model_catalog")
            {
                bail!("Model settings changed during verification; inspect before retrying")
            }
            crate::installed_selection::check_index(
                &self.model_directory(),
                &prepared.index_entry,
            )?;
        }
        // A matching existing selection is already configured; retain even
        // extension fields rather than serializing/replacing its owner settings.
        let mut changed = if prepared.is_some_and(|p| p.existing) {
            false
        } else {
            crate::model_selection::apply(&mut config, &request.command)?
        };
        if let Some(prepared) = prepared.filter(|p| p.interpreter) {
            changed |= crate::model_selection::apply(
                &mut config,
                &crate::model_selection::ModelCommand::Interpreter {
                    backend: crate::model_selection::Backend::Llamacpp,
                    model_id: prepared.slot.model_id.clone(),
                    activate_backend: prepared.activate_backend,
                },
            )?;
        }
        let selections = crate::model_selection::selection_view(&config)?;
        let live = self.live.lock().unwrap();
        let registered = if let Some(models) = live.get("models") {
            let record = crate::network::PeerRecord::from_registration(
                &json!({"node_id":self.node_id,"models":models}),
            )?;
            Some(record.models.into_iter().map(|model| {
                let mut value = json!({"backend":model.backend,"model_id":model.model_id,"model_metadata":model.normalized_metadata()});
                if let Some(threads) = model.cpu_threads { value["cpu_threads"] = json!(threads); }
                if let Some(limit)=model.job_memory_limit_mib {value["job_memory_limit_mib"]=json!(limit);}
                value
            }).collect::<Vec<_>>())
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
        let limits = serde_json::from_value::<crate::request_limits::RequestLimits>(
            live["request_limits"].clone(),
        )
        .ok()
        .filter(|limits| limits.validate().is_ok());
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
            "registration":crate::registration::State::observe(&live,&self.node_id,self.submissions.state()),
            "version":crate::VERSION,"node_id":self.node_id,"metrics_available":true,"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
            "service_uptime_ms":self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            "runtime_id":live["runtime_id"].as_str().filter(|s|s.len()<=64),
            "connection_state":state,"input_channel":self.submissions.state(),"connection_checks":live["connection_checks"].as_u64().unwrap_or(0),
            "successful_connections":live["successful_connections"].as_u64().unwrap_or(0),
            "failed_connection_attempts":live["failed_connection_attempts"].as_u64().unwrap_or(0),
            "pending_restart":live["pending_restart"].as_bool().unwrap_or(false),
            "model_count":live["models"].as_array().map(Vec::len).unwrap_or(0),
            "capability_count":live["capabilities"].as_array().map(Vec::len).unwrap_or(0),
            "request_limits":limits,"busy_rejections":live["busy_rejections"].as_u64().unwrap_or(0),
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
        let previous = config.clone();
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
            "python_bridge",
            "peer_destination_address",
        ] {
            if let Some(value) = changes.get(key) {
                config[key] = value.clone();
            }
        }
        validate_config_shape(&config)?;
        validate_model_references(&config, &previous, &self.config_path)?;
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
        let live = self.live.lock().unwrap();
        if progress["id"].is_string() && live["download"]["id"] == progress["id"] {
            bail!("This download ID was already admitted. Inspect its status; do not replay it.")
        }
        if live["download"]["state"] == "downloading" {
            bail!("A model is already downloading. Wait or cancel it first.")
        }
        // Config ownership serializes admissions. Never hold the general live
        // status mutex across a receipt fsync.
        drop(live);
        self.download_receipts
            .lock()
            .map_err(|_| anyhow::anyhow!("Model download receipt owner unavailable"))?
            .begin(&self.config_path, &self.node_id, &current, &progress)?;
        self.live.lock().unwrap()["download"] = progress;
        Ok(())
    }

    pub async fn remote(self: &Arc<Self>, action: &str, params: Value) -> Result<Value> {
        let mut config = self.read_config()?;
        let allowed = config
            .pointer("/management/allow_remote")
            .and_then(Value::as_bool)
            == Some(true);
        match action {
            "bootstrap_policy" => {
                self.model_bootstrap_policy(serde_json::from_value(params)?, true)
            }
            "bootstrap_policy_status" => {
                let request: crate::model_bootstrap::Request = serde_json::from_value(params)?;
                if !matches!(request.command, crate::model_bootstrap::Command::Show) {
                    bail!("Reserved bootstrap policy status action mismatch")
                }
                self.model_bootstrap_policy(request, true)
            }
            "python_policy" => self.python_policy(serde_json::from_value(params)?, true),
            "python_policy_status" => {
                let request: crate::python_policy::Request = serde_json::from_value(params)?;
                if !matches!(request.command, crate::python_policy::Command::Show) {
                    bail!("Reserved Python policy status action mismatch")
                }
                self.python_policy(request, true)
            }
            "relay_command" => self.relay_command(serde_json::from_value(params)?, true),
            "relay_status" => {
                let request: crate::relay_setup::RelayRequest = serde_json::from_value(params)?;
                if !matches!(request.command, crate::relay_setup::RelayCommand::Show) {
                    bail!("Reserved relay status action mismatch")
                }
                self.relay_command(request, true)
            }
            "permission_command" => self.permission_command(serde_json::from_value(params)?, true),
            "permission_status" => {
                let request: crate::permission_commands::PermissionRequest =
                    serde_json::from_value(params)?;
                if !matches!(
                    request.command,
                    crate::permission_commands::PermissionCommand::Show {}
                ) {
                    bail!("Reserved permission status action mismatch")
                }
                self.permission_command(request, true)
            }
            "model_lifecycle_policy" => {
                self.model_lifecycle_policy_command(serde_json::from_value(params)?, true, false)
            }
            "model_lifecycle_policy_status" => {
                let request: crate::model_lifecycle_policy::PolicyRequest =
                    serde_json::from_value(params)?;
                if !matches!(
                    request.command,
                    crate::model_lifecycle_policy::PolicyCommand::Show {}
                ) {
                    bail!("reserved model policy action mismatch")
                }
                self.model_lifecycle_policy_command(request, true, false)
            }
            "model_command" => {
                self.dispatch_model_command(serde_json::from_value(params)?, true)
                    .await
            }
            "model_lifecycle" => {
                self.model_lifecycle_command(serde_json::from_value(params)?, true)
                    .await
            }
            "model_lifecycle_status" | "model_lifecycle_cancel" => {
                let request: crate::model_lifecycle::LifecycleRequest =
                    serde_json::from_value(params)?;
                if !(action == "model_lifecycle_status"
                    && matches!(
                        request.command,
                        crate::model_lifecycle::LifecycleCommand::Show { .. }
                    )
                    || action == "model_lifecycle_cancel"
                        && matches!(
                            request.command,
                            crate::model_lifecycle::LifecycleCommand::Cancel { .. }
                        ))
                {
                    bail!("reserved model control action mismatch")
                }
                self.model_lifecycle_command(request, true).await
            }
            "diagnostics" => Ok(self.diagnostics()),
            "get" => {
                config["psk"] = json!(SECRET_PLACEHOLDER);
                Ok(json!({"config":config,"remote_allowed":allowed}))
            }
            "status" => {
                let live = self.live.lock().unwrap().clone();
                Ok(
                    json!({"control_schema_version":1,"remote_allowed":allowed,"runtime_id":live["runtime_id"],"connection_state":live["connection"]["state"],
                    "name":config["name"],"node_id":self.node_id,"version":crate::VERSION,"connection":live["connection"],"hardware":live["hardware"],"permissions":live["permissions"],"models":live["models"],"pending_restart":live["pending_restart"],"download":live["download"],"metadata":live["metadata"],"last_request_context":live["last_request_context"],"last_resource_id":live["last_resource_id"]}),
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

/// Structural/policy validation; commits separately validate introduced artifacts.
fn validate_config_shape(value: &Value) -> Result<()> {
    crate::model_lifecycle_policy::ModelLifecyclePolicy::from_config(value)?;
    if let Some(policy) = value.get("input_workflow") {
        serde_json::from_value::<crate::request_input::InputPolicy>(policy.clone())?;
    }
    if let Some(policy) = value.get("grants") {
        serde_json::from_value::<crate::grants::GrantPolicy>(policy.clone())?.validate()?;
    }
    if let Some(id) = value.get("node_id") {
        crate::network::validate_remote_node_id(id.as_str().unwrap_or(""))?;
    }
    let auth_mode: crate::authentication::AuthMode = serde_json::from_value(
        value
            .get("auth_mode")
            .cloned()
            .unwrap_or_else(|| json!("legacy")),
    )
    .map_err(|_| anyhow::anyhow!("invalid node authentication mode"))?;
    if auth_mode == crate::authentication::AuthMode::NodeCredential {
        crate::authentication::validate_credential(value["psk"].as_str().unwrap_or(""))?;
        crate::tls::client_config(value["coordinator_certificate"].as_str().unwrap_or(""))?;
    }
    let limits: crate::request_limits::RequestLimits = serde_json::from_value(
        value
            .get("request_limits")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )?;
    limits.validate()?;
    let network: crate::network::NetworkAdvertisement =
        serde_json::from_value(value.get("network").cloned().unwrap_or_else(|| json!({})))?;
    network.validate()?;
    let metadata: crate::context::NodeMetadata =
        serde_json::from_value(value.get("metadata").cloned().unwrap_or_else(|| json!({})))?;
    metadata.validate()?;
    if let Some(address) = serde_json::from_value::<Option<std::net::SocketAddr>>(
        value
            .get("peer_destination_address")
            .cloned()
            .unwrap_or(Value::Null),
    )? {
        crate::peer::plan::validate_address(address)?;
    }
    serde_json::from_value::<crate::python::BridgeLimits>(
        value
            .get("python_bridge")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )?
    .validate()?;
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

/// Validate introduced/rebound artifacts against startup's configuration base.
/// Keeping an existing identity and model/projector references is not a health claim.
fn validate_model_references(
    value: &Value,
    previous: &Value,
    config: &std::path::Path,
) -> Result<()> {
    let Some(raw) = value.pointer("/models/llamacpp/slots") else {
        return Ok(());
    };
    let slots: Vec<crate::ModelSlot> = serde_json::from_value(raw.clone())?;
    let prior = previous
        .pointer("/models/llamacpp/slots")
        .and_then(Value::as_array);
    let resolved = |path: &std::path::Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            config
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(path)
        }
    };
    for slot in slots {
        let matching: Vec<_> = prior
            .into_iter()
            .flatten()
            .filter(|old| old["model_id"] == slot.model_id)
            .collect();
        let retained = matching.len() == 1
            && serde_json::from_value::<crate::ModelSlot>(matching[0].clone()).is_ok_and(|old| {
                old.model_path == slot.model_path && old.mmproj_path == slot.mmproj_path
            });
        if retained {
            continue;
        }
        if !resolved(&slot.model_path).is_file() {
            bail!("Choose an existing model file before selecting new model references")
        }
        if slot
            .mmproj_path
            .as_ref()
            .is_some_and(|path| !resolved(path).is_file())
        {
            bail!("Choose an existing projector file before selecting new model references")
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "model_reference_tests.rs"]
mod model_reference_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn installed_selection_commit_rechecks_snapshots_index_and_current_remote_approval() {
        use crate::model_selection::{ModelCommand, ModelCommandRequest};
        for case in ["models", "catalog", "index", "revoked", "unrelated"] {
            let root = tempfile::tempdir().unwrap();
            let service = crate::installed_selection::tests::fixture(root.path());
            let mut config = service.read_config().unwrap();
            config["management"]["allow_remote"] = json!(true);
            service.save_config(config.clone()).unwrap();
            let mut prepared = crate::installed_selection::prepare(
                service.model_directory(),
                config,
                "fixture".into(),
                true,
            )
            .await
            .unwrap();
            prepared.activate_backend = true;
            #[cfg(windows)]
            {
                assert!(
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(&prepared.slot.model_path)
                        .is_err(),
                    "verified read lease must exclude writers"
                );
            }
            let mut latest = service.read_config().unwrap();
            match case {
                "models" => latest["models"]["provider"] = json!("llamacpp"),
                "catalog" => latest["model_catalog"][0]["name"] = json!("Changed"),
                "index" => {
                    let path = service.model_directory().join("installed.json");
                    let mut index: Value =
                        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    index[0]["name"] = json!("Changed");
                    crate::setup::write_json(&path, &index).unwrap();
                }
                "revoked" => latest["management"]["allow_remote"] = json!(false),
                "unrelated" => latest["name"] = json!("Owner's latest name"),
                _ => unreachable!(),
            }
            service.save_config(latest).unwrap();
            let before = std::fs::read(root.path().join("node.json")).unwrap();
            let result = service.model_command_checked(
                ModelCommandRequest {
                    schema_version: 1,
                    expected_node_id: "cache-node".into(),
                    command: ModelCommand::SelectGguf {
                        slot: prepared.slot.clone(),
                    },
                },
                true,
                Some(&prepared),
            );
            if case == "unrelated" {
                assert_eq!(result.unwrap()["success"], true);
                assert_eq!(
                    service.read_config().unwrap()["models"]["provider"],
                    "llamacpp"
                );
                assert_eq!(
                    service.read_config().unwrap()["name"],
                    "Owner's latest name"
                );
            } else {
                if case == "revoked" {
                    assert_eq!(result.unwrap()["error_code"], "approval_required");
                } else {
                    assert!(result.is_err(), "{case}");
                }
                assert_eq!(
                    std::fs::read(root.path().join("node.json")).unwrap(),
                    before,
                    "{case}"
                );
            }
        }
    }
    #[tokio::test]
    async fn remote_status_exposes_restart_preflight_without_raw_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("node.json");
        std::fs::write(&config_path, br#"{"management":{"allow_remote":false},"psk":"PRIVATE-PSK","unrelated":"PRIVATE-CONFIG"}"#).unwrap();
        let service = NodeService::new(config_path, "stable".into());
        let runtime_id = uuid::Uuid::new_v4().to_string();
        *service.live.lock().unwrap() =
            json!({"runtime_id":runtime_id,"connection":{"state":"connected"}});
        let status = service.remote("status", json!({})).await.unwrap();
        assert_eq!(status["control_schema_version"], 1);
        assert_eq!(status["node_id"], "stable");
        assert_eq!(status["remote_allowed"], false);
        assert_eq!(status["runtime_id"], runtime_id);
        assert_eq!(status["connection_state"], "connected");
        assert!(!status.to_string().contains("PRIVATE"));
        assert!(service.remote("restart", json!({})).await.is_err());
    }
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
        core.live.lock().unwrap()["request_limits"] =
            json!({"work":16,"control":2,"secret":"PRIVATE"});
        assert!(core.diagnostics()["request_limits"].is_null());
        assert!(!core.diagnostics().to_string().contains("PRIVATE"));
    }

    #[test]
    fn invalid_request_limits_do_not_overwrite_saved_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let config = json!({"node_id":"stable","psk":"private-fixture","request_limits":{"work":2,"control":1},"future":{"preserved":true}});
        crate::setup::write_json(&path, &config).unwrap();
        let service = NodeService::new(path.clone(), "stable".into());
        let before = std::fs::read(&path).unwrap();
        for limits in [
            json!({"work":0}),
            json!({"control":9}),
            json!({"work":"2"}),
            json!({"unknown":true}),
            json!(null),
        ] {
            let mut invalid = config.clone();
            invalid["request_limits"] = limits;
            assert!(service.save_configuration(invalid).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        assert!(
            validate_config_shape(&json!({})).is_ok(),
            "older configurations retain bounded defaults"
        );
    }

    #[test]
    fn request_limit_commands_preserve_config_distinguish_applied_and_never_restart() {
        use crate::connection_commands::{ConnectionCommand, ConnectionRequest};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let config = json!({"node_id":"stable","psk":"PRIVATE","request_limits":{"work":2,"control":1},"future":{"preserved":true}});
        crate::setup::write_json(&path, &config).unwrap();
        let service = NodeService::new(path.clone(), "stable".into());
        service.live.lock().unwrap()["request_limits"] = json!({"work":2,"control":1});
        let command = |work, control| ConnectionRequest {
            schema_version: 1,
            expected_node_id: "stable".into(),
            command: ConnectionCommand::Limits { work, control },
        };
        let before = std::fs::read(&path).unwrap();
        let shown = service.connection_command(command(None, None)).unwrap();
        assert_eq!(shown["changed"], false);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            service
                .connection_command(command(Some(0), Some(2)))
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut wrong = command(Some(4), None);
        wrong.expected_node_id = "other".into();
        assert!(service.connection_command(wrong).is_err());
        let changed = service.connection_command(command(Some(4), None)).unwrap();
        assert_eq!(
            changed["saved_request_limits"],
            json!({"work":4,"control":1})
        );
        assert_eq!(
            changed["applied_request_limits"],
            json!({"work":2,"control":1})
        );
        assert_eq!(changed["restart_required"], true);
        assert_eq!(changed["restart_requested"], false);
        assert!(!changed.to_string().contains("PRIVATE"));
        let saved = service.read_config().unwrap();
        assert_eq!(saved["future"], config["future"]);
        assert_eq!(saved["psk"], "PRIVATE");
        assert_eq!(saved["node_id"], "stable");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            service.connection_command(command(None, Some(1))).unwrap()["changed"],
            false
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
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
        let progress = json!({"state":"downloading","id":uuid::Uuid::new_v4().to_string(),"model_id":"fixture","backend":"ollama","definition_sha256":"a".repeat(64)});
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
        assert!(
            validate_config_shape(&json!({"dashboard":{"enabled":false,"ui_enabled":true}}))
                .is_ok()
        );
        assert!(validate_config_shape(&json!({"dashboard":{"ui_enabled":"false"}})).is_err());
        assert!(validate_config_shape(&json!({"models": {"provider": "magic"}})).is_err());
        assert!(validate_config_shape(&json!({"models": {"provider": "auto"}})).is_ok());
        assert!(validate_config_shape(&json!({"models":{"ollama":{"selected":[{"model_id":"fixture","modality":"unknown"}]}}})).is_err());
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

    #[tokio::test]
    async fn python_bridge_policy_is_validated_approved_and_saved_applied_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":"stable","psk":"private","endpoints":[]}),
        )
        .unwrap();
        let service = NodeService::new(path.clone(), "stable".into());
        let before = std::fs::read(&path).unwrap();
        assert!(
            service
                .remote(
                    "save",
                    json!({"config":{"python_bridge":{"timeout_seconds":0}}})
                )
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let limits = json!({"timeout_seconds":4,"max_request_bytes":2048,"max_stdout_bytes":4096,"max_stderr_bytes":1024});
        let proposed = service
            .remote("save", json!({"config":{"python_bridge":limits}}))
            .await
            .unwrap();
        assert_eq!(proposed["approval_required"], true);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(service.status()["python_bridge"]["enabled"], false);
        service.live.lock().unwrap()["python_bridge"] =
            json!(crate::python::BridgeLimits::default());
        service.save_config(json!({"node_id":"stable","psk":SECRET_PLACEHOLDER,"endpoints":[],"python_bridge":limits})).unwrap();
        let status = service.status()["python_bridge"].clone();
        assert_eq!(status["saved"], limits);
        assert_eq!(status["saved_valid"], true);
        assert_eq!(
            status["applied"],
            json!(crate::python::BridgeLimits::default())
        );
        assert_eq!(status["restart_required"], true);
        assert_eq!(service.read_config().unwrap()["psk"], "private");
    }
}
