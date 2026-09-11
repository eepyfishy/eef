//! Runtime-owned node state and operations; no HTTP or browser dependency.
use crate::NodePolicy;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
pub(crate) const SECRET_PLACEHOLDER: &str = "__KEEP_EXISTING_SECRET__";

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

    pub(crate) fn model_directory(&self) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("models")
    }

    pub async fn remote(self: &Arc<Self>, action: &str, params: Value) -> Result<Value> {
        let mut config = self.read_config()?;
        let allowed = config
            .pointer("/management/allow_remote")
            .and_then(Value::as_bool)
            == Some(true);
        match action {
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
            "save" => {
                let changes = params.get("config").unwrap_or(&params);
                // Remote management cannot grant itself permission, replace identity,
                // replace the network secret, or inject arbitrary Python plugins.
                for key in [
                    "name",
                    "permissions",
                    "models",
                    "update",
                    "metadata",
                    "network",
                ] {
                    if let Some(v) = changes.get(key) {
                        config[key] = v.clone();
                    }
                }
                validate_config(&config)?;
                if !allowed {
                    config["psk"] = json!(SECRET_PLACEHOLDER);
                    crate::setup::write_json(
                        &self.config_path.with_extension("proposal.json"),
                        &config,
                    )?;
                    return Ok(
                        json!({"approval_required":true,"message":"Review and approve these changes in the device app."}),
                    );
                }
                self.save_config(config)?;
                self.live.lock().unwrap()["pending_restart"] = json!(true);
                Ok(json!({"saved":true,"restart_required":true}))
            }
            "restart" if allowed => {
                let state = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    state.restart.notify_one();
                });
                Ok(json!({"restarting":true}))
            }
            "install" if allowed => {
                crate::model_manager::install(self.clone(), params).await?;
                Ok(json!({"started":true}))
            }
            "cancel" if allowed => {
                Ok(json!({"requested":crate::model_manager::cancel(self, params["id"].as_str())?}))
            }
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
        for slot in slots {
            if slot.model_id.trim().is_empty() || !slot.model_path.is_file() {
                bail!("Choose an installed model file before saving")
            }
            if slot.port == 0 || !ports.insert(slot.port) {
                bail!("Each local model needs a different nonzero port")
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(validate_config(&json!({"models": {"provider": "magic"}})).is_err());
        assert!(validate_config(&json!({"models": {"provider": "auto"}})).is_ok());
        assert!(validate_config(&json!({"models":{"ollama":{"selected":[{"model_id":"fixture","modality":"unknown"}]}}})).is_err());
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
