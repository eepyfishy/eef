use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::NodePolicy;

const SECRET_PLACEHOLDER: &str = "__KEEP_EXISTING_SECRET__";

#[derive(Clone)]
pub struct NodeDashboard {
    config_path: PathBuf,
    node_id: String,
    pub live: Arc<std::sync::Mutex<Value>>,
    pub restart: Arc<tokio::sync::Notify>,
    pub submissions: Arc<crate::submission::SubmissionMailbox>,
    config_lock: Arc<std::sync::Mutex<()>>,
}

struct DashboardError(anyhow::Error);

#[derive(Deserialize)]
struct StartupRequest {
    enabled: bool,
}

impl From<anyhow::Error> for DashboardError {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}

impl IntoResponse for DashboardError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": self.0.to_string()})),
        )
            .into_response()
    }
}

impl NodeDashboard {
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
        })
    }

    pub async fn start(self: &Arc<Self>, host: &str, port: u16) -> Result<JoinHandle<()>> {
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .with_context(|| format!("bind EEFN dashboard {host}:{port}"))?;
        let actual_port = listener.local_addr()?.port();
        let app = Router::new()
            .route("/", get(index))
            .route("/app.js", get(script))
            .route("/app.css", get(style))
            .route("/advanced/legacy", get(legacy))
            .route("/api/ui", get(|| async { Json(json!({"role":"device"})) }))
            .route("/api/status", get(status))
            .route("/api/config", get(config_get).put(config_save))
            .route("/api/startup", get(startup_get).put(startup_set))
            .route("/api/restart", post(restart))
            .route("/api/chat", post(chat))
            .route("/api/jobs", get(jobs_list).post(jobs_create))
            .route("/api/jobs/{id}", get(jobs_get).delete(jobs_remove))
            .route("/api/jobs/{id}/{action}", post(jobs_control))
            .route("/api/config/restore", post(restore))
            .route("/api/config/reset", post(reset))
            .route("/api/network/local", post(pair_local))
            .route("/api/network/pause", post(pause_connection))
            .route("/api/models", get(models))
            .route("/api/models/install", post(model_install))
            .route("/api/models/inspect", post(model_inspect))
            .route("/api/models/cancel", post(model_cancel))
            .route("/api/proposal", get(proposal).delete(proposal_discard))
            .route("/api/pick", post(pick))
            .route("/api/update/check", post(update_check))
            .route("/api/update/apply", post(update_apply))
            .layer(axum::middleware::from_fn(crate::setup::local_ui_guard))
            .with_state(self.clone());
        info!(host, port = actual_port, "EEFN dashboard listening");
        Ok(tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                warn!(%error, "EEFN dashboard stopped");
            }
        }))
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

    pub(crate) fn save_config(&self, mut value: Value) -> Result<()> {
        let _guard = self.config_lock.lock().expect("config lock");
        if !value.is_object() {
            bail!("node configuration must be a JSON object")
        }
        let old = self.read_config().unwrap_or_else(|_| json!({}));
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
                for key in ["name", "permissions", "models", "update", "metadata"] {
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

async fn index() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/app.html"))
}
async fn legacy() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/node.html"))
}

async fn script() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        include_str!("../../../dashboard/app.js"),
    )
}
async fn style() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../../dashboard/app.css"),
    )
}

async fn status(State(state): State<Arc<NodeDashboard>>) -> Json<Value> {
    let config = state.read_config().unwrap_or_else(|_| json!({}));
    let live = state.live.lock().unwrap().clone();
    Json(json!({
        "node_id": state.node_id,
        "runtime": "rust",
        "version": crate::VERSION,
        "node_alive": true,
        "coordinator_required_for_node": false,
        "coordinator_required_for_orchestration": true,
        "name":config["name"], "hostname":crate::setup::hostname(), "coordinator_name":config["coordinator_name"], "metadata":live["metadata"],
        "connection":live["connection"], "hardware":live["hardware"], "permissions":live["permissions"],
        "models":live["models"], "capabilities":live["capabilities"], "last_activity":live["last_activity"],
        "last_request_context":live["last_request_context"], "last_resource_id":live["last_resource_id"],
        "pending_restart":live["pending_restart"], "download":live["download"], "update":live["update"],
        "proposal_pending":state.config_path.with_extension("proposal.json").is_file(),
    }))
}

async fn config_get(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let mut config = state.read_config()?;
    if config
        .get("psk")
        .and_then(Value::as_str)
        .is_some_and(|secret| !secret.is_empty())
    {
        config["psk"] = Value::String(SECRET_PLACEHOLDER.into());
    }
    Ok(Json(json!({
        "config": config,
        "path": state.config_path.display().to_string(),
        "note": "Changes apply after EEFN restarts"
    })))
}

async fn config_save(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    state.save_config(value.get("config").unwrap_or(&value).clone())?;
    state.live.lock().unwrap()["pending_restart"] = json!(true);
    Ok(Json(json!({"saved": true, "restart_required": true})))
}

async fn jobs_list(State(state): State<Arc<NodeDashboard>>) -> Result<Json<Value>, DashboardError> {
    Ok(Json(
        state.submissions.request("jobs.list", json!({})).await?,
    ))
}
async fn jobs_create(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(state.submissions.request("jobs.create", value).await?))
}
async fn jobs_get(
    State(state): State<Arc<NodeDashboard>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(
        state
            .submissions
            .request("jobs.get", json!({"id":id}))
            .await?,
    ))
}
async fn jobs_remove(
    State(state): State<Arc<NodeDashboard>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(
        state
            .submissions
            .request("jobs.remove", json!({"id":id}))
            .await?,
    ))
}
async fn jobs_control(
    State(state): State<Arc<NodeDashboard>>,
    Path((id, action)): Path<(String, String)>,
) -> Result<Json<Value>, DashboardError> {
    if !matches!(action.as_str(), "pause" | "resume" | "stop") {
        return Err(anyhow::anyhow!("Unknown job control").into());
    }
    Ok(Json(
        state
            .submissions
            .request(&format!("jobs.{action}"), json!({"id":id}))
            .await?,
    ))
}

async fn chat(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    let text = value["message"].as_str().context("message is required")?;
    Ok(Json(state.submissions.submit(text).await?))
}

async fn pause_connection(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    let paused = value["paused"]
        .as_bool()
        .context("paused must be true or false")?;
    let mut config = state.read_config()?;
    config["connection_enabled"] = json!(!paused);
    state.save_config(config)?;
    state.restart.notify_one();
    Ok(Json(json!({"paused":paused})))
}

async fn restart(State(state): State<Arc<NodeDashboard>>) -> Json<Value> {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        state.restart.notify_one();
    });
    Json(json!({"restarting":true}))
}
async fn restore(State(state): State<Arc<NodeDashboard>>) -> Result<Json<Value>, DashboardError> {
    let backup: Value = serde_json::from_slice(
        &std::fs::read(state.config_path.with_extension("json.bak"))
            .map_err(anyhow::Error::from)?,
    )
    .map_err(anyhow::Error::from)?;
    state.save_config(backup)?;
    state.live.lock().unwrap()["pending_restart"] = json!(true);
    Ok(Json(json!({"saved":true,"restart_required":true})))
}
async fn reset(State(state): State<Arc<NodeDashboard>>) -> Result<Json<Value>, DashboardError> {
    let mut value = crate::setup::defaults();
    value["name"] = json!(crate::setup::hostname());
    state.save_config(value)?;
    state.live.lock().unwrap()["pending_restart"] = json!(true);
    Ok(Json(json!({"saved":true,"restart_required":true})))
}
async fn pair_local(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let mut config = state.read_config()?;
    config["auto_local"] = json!(true);
    config["local_pairing"] = json!(true);
    config["endpoints"] = json!([]);
    state.save_config(config)?;
    state.discover_local(&state.config_path)?;
    state.restart.notify_one();
    Ok(Json(json!({"saved":true})))
}
async fn models(State(state): State<Arc<NodeDashboard>>) -> Result<Json<Value>, DashboardError> {
    Ok(Json(crate::model_manager::list(&state).await?))
}
async fn model_install(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    crate::model_manager::install(state, value).await?;
    Ok(Json(json!({"started":true})))
}
async fn model_inspect(
    State(state): State<Arc<NodeDashboard>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(crate::model_manager::inspect(&state, value).await?))
}
async fn model_cancel(
    State(state): State<Arc<NodeDashboard>>,
    value: Option<Json<Value>>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(
        json!({"requested":crate::model_manager::cancel(&state, value.as_ref().and_then(|value|value.0["id"].as_str()))?}),
    ))
}
async fn proposal(State(state): State<Arc<NodeDashboard>>) -> Result<Json<Value>, DashboardError> {
    let value: Value = serde_json::from_slice(
        &std::fs::read(state.config_path.with_extension("proposal.json"))
            .map_err(anyhow::Error::from)?,
    )
    .map_err(anyhow::Error::from)?;
    Ok(Json(json!({"config":value})))
}
async fn proposal_discard(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let path = state.config_path.with_extension("proposal.json");
    if path.is_file() {
        std::fs::remove_file(path).map_err(anyhow::Error::from)?;
    }
    Ok(Json(json!({"discarded":true})))
}
async fn pick(Json(request): Json<Value>) -> Result<Json<Value>, DashboardError> {
    if !cfg!(windows) {
        return Err(anyhow::anyhow!("Enter the path manually on this platform").into());
    }
    let kind = request["kind"].as_str().unwrap_or("");
    let script = match kind {
        "folder" => {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; if($d.ShowDialog() -eq 'OK'){[Console]::Write($d.SelectedPath)}"
        }
        "program" => {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='Programs (*.exe)|*.exe'; if($d.ShowDialog() -eq 'OK'){[Console]::Write($d.FileName)}"
        }
        "model" => {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='Models (*.gguf)|*.gguf'; if($d.ShowDialog() -eq 'OK'){[Console]::Write($d.FileName)}"
        }
        _ => return Err(anyhow::anyhow!("Unknown picker type").into()),
    };
    let mut command = tokio::process::Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-STA", "-Command", script])
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = tokio::time::timeout(std::time::Duration::from_secs(180), command.output())
        .await
        .map_err(anyhow::Error::from)?
        .map_err(anyhow::Error::from)?;
    if !output.status.success() {
        return Err(
            anyhow::anyhow!("The file picker could not open. Enter the path manually.").into(),
        );
    }
    Ok(Json(
        json!({"path":String::from_utf8_lossy(&output.stdout).trim()}),
    ))
}

async fn update_check(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let config = state.read_config()?;
    let url = config
        .pointer("/update/manifest_url")
        .and_then(Value::as_str)
        .context("Choose an update feed in Advanced first")?;
    Ok(Json(json!(
        crate::updater::check(url, crate::VERSION, std::time::Duration::from_secs(30)).await?
    )))
}
async fn update_apply(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let config = state.read_config()?;
    let url = config
        .pointer("/update/manifest_url")
        .and_then(Value::as_str)
        .context("Choose an update feed in Advanced first")?;
    let root =
        crate::updater::installation_root(std::env::current_exe().map_err(anyhow::Error::from)?)?;
    if !crate::updater::check(url, crate::VERSION, std::time::Duration::from_secs(30))
        .await?
        .update_available
    {
        return Err(anyhow::anyhow!("You are already up to date").into());
    }
    let result =
        crate::updater::apply_for(url, root, std::time::Duration::from_secs(600), "eefn").await?;
    state.live.lock().unwrap()["pending_restart"] = json!(true);
    Ok(Json(json!(result)))
}

async fn startup_get() -> Result<Json<Value>, DashboardError> {
    Ok(Json(json!(crate::startup_status("EEF Node")?)))
}

async fn startup_set(
    State(state): State<Arc<NodeDashboard>>,
    Json(request): Json<StartupRequest>,
) -> Result<Json<Value>, DashboardError> {
    let current = std::env::current_exe().map_err(anyhow::Error::from)?;
    let root = crate::updater::installation_root(&current)?;
    let stable = root.join("eefn.exe");
    let executable = if stable.is_file() { stable } else { current };
    Ok(Json(json!(crate::set_startup(
        "EEF Node",
        &executable,
        &["--config".into(), state.config_path.display().to_string()],
        request.enabled,
    )?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_management_requires_local_approval_and_cannot_grant_itself_trust() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        crate::setup::write_json(&path,&json!({"node_id":"stable","name":"Original","psk":"private-network-secret","endpoints":[]})).unwrap();
        let dashboard = NodeDashboard::new(path.clone(), "stable".into());
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
        let dashboard = NodeDashboard::new(path.clone(), "node".into());
        dashboard
            .save_config(json!({"psk": SECRET_PLACEHOLDER, "endpoints": []}))
            .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["psk"], "secret-value");
    }
}
