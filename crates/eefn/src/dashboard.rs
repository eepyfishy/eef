use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{info, warn};

pub use crate::service::NodeService as NodeDashboard;
use crate::service::SECRET_PLACEHOLDER;

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
    pub async fn start(self: &Arc<Self>, host: &str, port: u16) -> Result<JoinHandle<()>> {
        self.start_with_ui(host, port, true).await
    }

    pub async fn start_with_ui(
        self: &Arc<Self>,
        host: &str,
        port: u16,
        ui_enabled: bool,
    ) -> Result<JoinHandle<()>> {
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .with_context(|| format!("bind EEFN dashboard {host}:{port}"))?;
        let actual_port = listener.local_addr()?.port();
        if let Err(error) =
            crate::local_commands::publish(&self.config_path, &self.node_id, listener.local_addr()?)
        {
            warn!(%error,"could not save local API discovery; commands may require unchanged API settings");
        }
        let app = if ui_enabled {
            Router::new()
                .route("/", get(index))
                .route("/app.js", get(script))
                .route("/app.css", get(style))
                .route("/advanced/legacy", get(legacy))
        } else {
            Router::new()
        };
        let app = app
            .route("/api/ui", get(|| async { Json(json!({"role":"device"})) }))
            .route("/api/status", get(status))
            .route("/api/diagnostics", get(diagnostics))
            .route("/api/commands/network", post(network_command))
            .route("/api/commands/models", post(model_command))
            .route("/api/commands/jobs", post(job_command))
            .route("/api/commands/restart", post(restart_command))
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
        "network":live["network"],
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

async fn restart_command(
    State(state): State<Arc<NodeDashboard>>,
    Json(request): Json<crate::local_commands::RestartRequest>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(state.request_restart(request)?))
}

async fn model_command(
    State(state): State<Arc<NodeDashboard>>,
    Json(request): Json<crate::model_selection::ModelCommandRequest>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(state.model_command(request)?))
}

async fn job_command(
    State(state): State<Arc<NodeDashboard>>,
    Json(request): Json<crate::job_commands::JobRequest>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(state.job_command(request).await?))
}

async fn network_command(
    State(state): State<Arc<NodeDashboard>>,
    Json(request): Json<crate::network::CommandRequest>,
) -> Result<Json<Value>, DashboardError> {
    Ok(Json(state.execute_network_command(request).await?))
}

async fn diagnostics(State(state): State<Arc<NodeDashboard>>) -> Json<Value> {
    Json(state.diagnostics())
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
    Json(state.queue_restart())
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
