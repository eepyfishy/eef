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
            .with_context(|| format!("bind eefn command API {host}:{port}"))?;
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
            .route(
                "/api/commands/setup/check",
                post(setup_check).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/setup/model-verification/prepare",
                post(prepare_model_verification).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/setup/local",
                post(local_setup_plan).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/setup/enroll",
                post(local_enroll).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/connection/pair",
                post(credential_pair).layer(axum::extract::DefaultBodyLimit::max(8192)),
            )
            .route("/api/commands/models", post(model_command))
            .route(
                "/api/commands/model-bootstrap-policy",
                post(model_bootstrap_policy).layer(axum::extract::DefaultBodyLimit::max(8192)),
            )
            .route(
                "/api/commands/repair-setup-policy",
                post(package_policy).layer(axum::extract::DefaultBodyLimit::max(16384)),
            )
            .route(
                "/api/commands/python",
                post(python_policy).layer(axum::extract::DefaultBodyLimit::max(8192)),
            )
            .route(
                "/api/commands/listening",
                post(listening_command).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/permissions",
                post(permission_command).layer(axum::extract::DefaultBodyLimit::max(4096)),
            )
            .route(
                "/api/commands/relay",
                post(relay_command).layer(axum::extract::DefaultBodyLimit::max(8192)),
            )
            .route("/api/commands/model-lifecycle", post(model_lifecycle))
            .route(
                "/api/commands/model-lifecycle-policy",
                post(model_lifecycle_policy),
            )
            .route(
                "/api/commands/model-downloads",
                post(model_download_command),
            )
            .route("/api/commands/jobs", post(job_command))
            .route("/api/commands/grants/receipt", post(grant_receipt))
            .route("/api/commands/grants/result", post(grant_result))
            .route("/api/commands/requests/preview", post(request_preview))
            .route("/api/commands/requests/review", post(request_review))
            .route("/api/commands/requests/open", post(request_open))
            .route("/api/commands/requests/input", post(request_input))
            .route("/api/commands/requests/respond", post(request_response))
            .route(
                "/api/commands/requests/workflow",
                post(request_input_policy),
            )
            .route(
                "/api/commands/requests/open-proposal",
                post(request_open_proposal),
            )
            .route("/api/commands/requests/drafts", post(request_draft_command))
            .route("/api/commands/requests/resolve", post(request_resolution))
            .route("/api/commands/requests/prepare", post(request_preparation))
            .route("/api/commands/requests/submit", post(request_submission))
            .route(
                "/api/commands/requests/audio",
                post(audio_submission).layer(axum::extract::DefaultBodyLimit::max(65536)),
            )
            .route("/api/commands/requests/planning", post(request_planning))
            .route("/api/commands/automations", post(automation_command))
            .route(
                "/api/commands/requests/abandon",
                post(request_abandon_command),
            )
            .route(
                "/api/commands/requests/receipt",
                post(request_receipt_command),
            )
            .route("/api/commands/requests/result", post(request_result))
            .route(
                "/api/commands/requests/stop-receipt",
                post(request_stop_receipt).layer(axum::extract::DefaultBodyLimit::max(8192)),
            )
            .route(
                "/api/commands/requests/recover-result",
                post(request_recovery),
            )
            .route(
                "/api/commands/requests/execute",
                post(request_execute_command),
            )
            .route(
                "/api/commands/requests/review-proposal",
                post(request_review_proposal),
            )
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
        info!(host, port = actual_port, "eefn command API listening");
        Ok(ApiServer {
            ui_enabled,
            task: tokio::spawn(async move {
                if let Err(error) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .await
                {
                    warn!(%error, "eefn command API stopped");
                }
            }),
        })
    }
}

async fn status(State(state): State<Arc<NodeService>>) -> Json<Value> {
    Json(state.status())
}

async fn setup_check(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::setup_check::CheckRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.setup_check_async(request).await?))
}
async fn prepare_model_verification(
    State(state): State<Arc<NodeService>>,
    Json(definition): Json<crate::model_verification::Definition>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state.prepare_model_verification(definition, false).await?,
    ))
}
async fn local_setup_plan(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::setup_check::CheckRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.local_setup_plan(request).await?))
}
async fn local_enroll(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::local_enrollment::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.enroll_local(request).await?))
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
    Ok(Json(state.dispatch_model_command(request, false).await?))
}

async fn audio_submission(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::audio_submission::AudioRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.audio_request(request).await?))
}
async fn permission_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::permission_commands::PermissionRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.permission_command(request, false)?))
}
async fn python_policy(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::python_policy::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.python_policy(request, false)?))
}
async fn listening_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::listening::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.listening_command(request, false)?))
}

async fn model_bootstrap_policy(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::model_bootstrap::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.model_bootstrap_policy(request, false)?))
}
async fn package_policy(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::package_setup::policy::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.repair_setup_policy(request, false).await?))
}
async fn relay_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::relay_setup::RelayRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.relay_command(request, false)?))
}
async fn model_lifecycle(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::model_lifecycle::LifecycleRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.model_lifecycle_command(request, false).await?))
}
async fn model_lifecycle_policy(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::model_lifecycle_policy::PolicyRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state.model_lifecycle_policy_command(request, false, false)?,
    ))
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

async fn credential_pair(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::credential_pairing::PairRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.pair_credential_command(request)?))
}

async fn diagnostics(State(state): State<Arc<NodeService>>) -> Json<Value> {
    Json(state.diagnostics())
}
async fn grant_receipt(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::grants::ReceiptRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.grant_receipt(request)?))
}
async fn grant_result(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::grants::ReceiptRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.grant_result(request)?))
}
async fn request_stop_receipt(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_stop_receipt::Request>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.request_stop_receipt(request).await?))
}
async fn request_result(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_result::ResultRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.request_result(request).await?))
}
async fn request_recovery(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_recovery::RecoverRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.recover_request_command(request).await?))
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

async fn request_preview(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::PreviewRequest>,
) -> Json<Value> {
    Json(state.preview_request(request).await)
}

async fn automation_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::automation::AutomationRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.automation_request(request).await?))
}

async fn request_review(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::PreviewRequest>,
) -> Json<Value> {
    Json(state.review_request(request).await)
}

async fn request_review_proposal(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::ProposalReviewRequest>,
) -> Json<Value> {
    Json(state.review_supplied_proposal(request).await)
}
async fn request_open(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::PreviewRequest>,
) -> Json<Value> {
    Json(state.open_request(request).await)
}
async fn request_input(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::PreviewRequest>,
) -> Json<Value> {
    Json(state.input_request(request).await)
}
async fn request_response(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_response::RespondRequest>,
) -> Json<Value> {
    Json(state.respond_request(request).await)
}
async fn request_input_policy(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_input::PolicyRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.input_policy_command(request)?))
}
async fn request_open_proposal(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preview::ProposalReviewRequest>,
) -> Json<Value> {
    Json(state.open_supplied_proposal(request).await)
}
async fn request_draft_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_drafts::DraftRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.draft_command(request).await?))
}
async fn request_resolution(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_resolution::ResolveRequest>,
) -> Json<Value> {
    Json(state.resolve_request(request).await)
}
async fn request_preparation(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_preparation::PrepareRequest>,
) -> Json<Value> {
    Json(state.prepare_request(request).await)
}
async fn request_submission(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_submission::ConcreteRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.concrete_request(request).await?))
}
async fn request_planning(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_planning::PlanningRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.planning_request(request).await?))
}
async fn request_abandon_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_abandon::AbandonRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.abandon_request_command(request).await?))
}
async fn request_receipt_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_receipt::ReceiptRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.request_receipt(request).await?))
}
async fn request_execute_command(
    State(state): State<Arc<NodeService>>,
    Json(request): Json<crate::request_execution::ExecuteRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.execute_command(request).await?))
}

async fn chat(
    State(state): State<Arc<NodeService>>,
    Json(value): Json<ChatInput>,
) -> Result<Json<Value>, ApiError> {
    // Compatibility adapter only: same runtime-owned input service as CLI.
    // Never send raw messages or silently fall back to keyword execution.
    let receipt = value
        .request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let is_answer = value.response_to.is_some();
    let mut report = if let Some(target) = value.response_to {
        state
            .respond_request(crate::request_response::RespondRequest {
                schema_version: 1,
                expected_node_id: state.node_id.clone(),
                inference_timeout_seconds: None,
                command: crate::request_response::RespondCommand {
                    id: target.id,
                    expected_revision: target.expected_revision,
                    text: value.message,
                    operation_id: receipt,
                },
            })
            .await
    } else {
        state
            .input_request(crate::request_preview::PreviewRequest {
                schema_version: 1,
                expected_node_id: state.node_id.clone(),
                request_id: receipt,
                text: value.message,
                inference_timeout_seconds: None,
            })
            .await
    };
    // Human-readable compatibility output, not fabricated model output or a
    // browser-owned execution decision. The complete receipt remains present.
    report["reply"] = json!(if is_answer {
        crate::request_response::reply(&report)
    } else {
        crate::request_input::input_reply(&report)
    });
    Ok(Json(report))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatInput {
    message: String,
    request_id: Option<String>,
    response_to: Option<ResponseTarget>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseTarget {
    id: String,
    expected_revision: u64,
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
    Ok(Json(json!(crate::startup_status("eef Node")?)))
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
