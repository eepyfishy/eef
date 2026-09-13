use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::service::NodeService;

pub(crate) struct ApiError(anyhow::Error);

/// HTTP-adapter ownership; browser availability is not runtime/service state.
pub struct ApiServer {
    task: JoinHandle<()>,
    ui_enabled: Arc<AtomicBool>,
}
impl ApiServer {
    pub fn set_ui_enabled(&self, enabled: bool) {
        self.ui_enabled.store(enabled, Ordering::Relaxed);
    }
    pub fn abort(&self) {
        self.task.abort();
    }
}

#[derive(Deserialize)]
struct StartupRequest {
    enabled: bool,
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": self.0.to_string()})),
        )
            .into_response()
    }
}

impl NodeService {
    pub async fn start(self: &Arc<Self>, host: &str, port: u16) -> Result<JoinHandle<()>> {
        self.start_with_ui(host, port, true).await
    }

    pub async fn start_with_ui(
        self: &Arc<Self>,
        host: &str,
        port: u16,
        ui_enabled: bool,
    ) -> Result<JoinHandle<()>> {
        Ok(self.start_api(host, port, ui_enabled).await?.task)
    }

    pub async fn start_api(
        self: &Arc<Self>,
        host: &str,
        port: u16,
        ui_enabled: bool,
    ) -> Result<ApiServer> {
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .with_context(|| format!("bind EEFN command API {host}:{port}"))?;
        let actual_port = listener.local_addr()?.port();
        if let Err(error) =
            crate::local_commands::publish(&self.config_path, &self.node_id, listener.local_addr()?)
        {
            warn!(%error,"could not save local API discovery; commands may require unchanged API settings");
        }
        let ui_enabled = Arc::new(AtomicBool::new(ui_enabled));
        let app = Router::new();
        #[cfg(feature = "dashboard")]
        let app = app.merge(crate::dashboard::web::router(ui_enabled.clone()));
        let app = app
            .route("/api/ui", get(|| async { Json(json!({"role":"device"})) }))
            .route("/api/status", get(status))
            .route("/api/diagnostics", get(diagnostics))
            .route("/api/commands/network", post(network_command))
            .route("/api/commands/connection", post(connection_command))
            .route("/api/commands/models", post(model_command))
            .route(
                "/api/commands/model-downloads",
                post(model_download_command),
            )
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
            .route("/api/update/check", post(update_check))
            .route("/api/update/apply", post(update_apply))
            .layer(axum::middleware::from_fn(crate::setup::local_ui_guard))
            .with_state(self.clone());
        info!(host, port = actual_port, "EEFN command API listening");
        Ok(ApiServer {
            ui_enabled,
            task: tokio::spawn(async move {
                if let Err(error) = axum::serve(listener, app).await {
                    warn!(%error, "EEFN command API stopped");
                }
            }),
        })
    }
}

async fn status(State(state): State<Arc<NodeService>>) -> Json<Value> {
    Json(state.status())
}

async fn restart_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::local_commands::RestartRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.request_restart(request)?))
}

async fn model_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::model_selection::ModelCommandRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.model_command(request)?))
}

async fn model_download_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::model_downloads::DownloadRequest>,
) -> Json<Value> {
    Json(state.download_command(request).await)
}

async fn job_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::job_commands::JobRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.job_command(request).await?))
}

async fn network_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::network::CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.execute_network_command(request).await?))
}

async fn connection_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::connection_commands::ConnectionRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.connection_command(request)?))
}

async fn diagnostics(State(state): State<Arc<NodeService>>) -> Json<Value> {
    Json(state.diagnostics())
}

async fn config_get(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.configuration()?))
}

async fn config_save(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.save_configuration(
        value.get("config").unwrap_or(&value).clone(),
    )?))
}

async fn jobs_list(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state.submissions.request("jobs.list", json!({})).await?,
    ))
}
async fn jobs_create(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.submissions.request("jobs.create", value).await?))
}
async fn jobs_get(
    State(state): State<Arc<NodeService>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state
            .submissions
            .request("jobs.get", json!({"id":id}))
            .await?,
    ))
}
async fn jobs_remove(
    State(state): State<Arc<NodeService>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state
            .submissions
            .request("jobs.remove", json!({"id":id}))
            .await?,
    ))
}
async fn jobs_control(
    State(state): State<Arc<NodeService>>,
    Path((id, action)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
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
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let text = value["message"].as_str().context("message is required")?;
    Ok(Json(state.submissions.submit(text).await?))
}

async fn pause_connection(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let paused = value["paused"]
        .as_bool()
        .context("paused must be true or false")?;
    Ok(Json(state.set_connection_paused(paused)?))
}

async fn restart(State(state): State<Arc<NodeService>>) -> Json<Value> {
    Json(state.queue_restart())
}
async fn restore(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.restore_configuration()?))
}
async fn reset(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.reset_configuration()?))
}
async fn pair_local(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.pair_with_local_coordinator()?))
}
async fn models(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(crate::model_manager::list(&state).await?))
}
async fn model_install(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    crate::model_manager::install(state, value).await?;
    Ok(Json(json!({"started":true})))
}
async fn model_inspect(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(crate::model_manager::inspect(&state, value).await?))
}
async fn model_cancel(
    State(state): State<Arc<NodeService>>,
    value: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"requested":crate::model_manager::cancel(&state, value.as_ref().and_then(|value|value.0["id"].as_str()))?}),
    ))
}
async fn proposal(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.configuration_proposal()?))
}
async fn proposal_discard(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.discard_configuration_proposal()?))
}
async fn update_check(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.check_update().await?))
}
async fn update_apply(State(state): State<Arc<NodeService>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.apply_update().await?))
}

async fn startup_get() -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(crate::startup_status("EEF Node")?)))
}

async fn startup_set(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<StartupRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(state.set_startup(request.enabled)?)))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn browser_gate_never_disables_or_rebinds_command_services() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path.clone(), "stable".into());
        let api = service.start_api("127.0.0.1", 0, false).await.unwrap();
        let address =
            crate::local_commands::endpoint(&path, "stable", "127.0.0.1", 0, true).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://{address}");
        for enabled in [false, true, false] {
            api.set_ui_enabled(enabled);
            let expected = if enabled && cfg!(feature = "dashboard") {
                200
            } else {
                404
            };
            assert_eq!(
                client
                    .get(format!("{base}/"))
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .as_u16(),
                expected
            );
            let reply: serde_json::Value = client
                .get(format!("{base}/api/diagnostics"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(reply["node_id"], "stable");
            assert_eq!(
                crate::local_commands::endpoint(&path, "stable", "127.0.0.1", 0, true).unwrap(),
                address
            );
        }
        api.abort();
    }
}
