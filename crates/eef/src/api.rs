use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;

use crate::Runtime;
use crate::assistant::ResponseRule;

#[derive(Debug)]
struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({"success": false, "error": self.0.to_string(), "error_type": "RequestError"}))).into_response()
    }
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Deserialize)]
struct StartupRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct InvokeRequest {
    capability: String,
    #[serde(default = "run")]
    action: String,
    #[serde(default = "empty_object")]
    params: Value,
    #[serde(default = "empty_object")]
    constraints: Value,
    #[serde(default = "timeout_seconds")]
    timeout: f64,
}

fn run() -> String {
    "run".into()
}
fn empty_object() -> Value {
    json!({})
}
const fn timeout_seconds() -> f64 {
    30.0
}

#[derive(Deserialize)]
struct FirmwareGenerateRequest {
    description: String,
    #[serde(default = "esp_id")]
    node_id: String,
    #[serde(default = "esp_id")]
    node_name: String,
    #[serde(default = "firmware_version")]
    version: String,
    config: Option<Value>,
}

fn esp_id() -> String {
    "esp-01".into()
}
fn firmware_version() -> String {
    "1.0.0".into()
}

#[derive(Deserialize)]
struct FirmwarePushRequest {
    node_id: String,
    source: Option<String>,
    #[serde(default = "firmware_version")]
    version: String,
}

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

pub fn router(runtime: Arc<Runtime>) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/api/status", get(status))
        .route("/api/config", get(config_get).put(config_save))
        .route("/api/startup", get(startup_get).put(startup_set))
        .route("/api/world", get(world))
        .route("/api/chat", post(chat))
        .route("/api/firmware/generate", post(firmware_generate))
        .route("/api/firmware/push", post(firmware_push))
        .route("/api/firmware/list", get(firmware_list))
        .route("/api/memory", get(memory))
        .route("/api/memory/reset", post(memory_reset))
        .route("/api/tasks", get(tasks))
        .route("/api/logs", get(logs))
        .route("/api/capabilities", get(capabilities))
        .route("/api/assistant/status", get(assistant_status))
        .route("/api/assistant/execute", post(assistant_execute))
        .route("/api/rules", get(rules_list).post(rules_create))
        .route("/api/rules/{rule_id}", delete(rules_delete))
        .route("/api/rules/{rule_id}/toggle", post(rules_toggle))
        .route("/api/node/status", get(node_status))
        .route("/api/node/{node_id}/invoke", post(node_invoke))
        .route("/api/update/status", get(update_status))
        .route("/api/update/check", post(update_check))
        .route("/api/update/apply", post(update_apply))
        .layer(TraceLayer::new_for_http())
        .with_state(runtime)
}

async fn root() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/index.html"))
}
async fn status(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(runtime.summary().await)
}

async fn config_get(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!({
        "config": runtime.config.as_value(),
        "path": runtime.config.source().map(|path| path.display().to_string()),
        "note": "Changes are validated and applied on restart"
    }))
}

async fn config_save(
    State(runtime): State<Arc<Runtime>>,
    Json(value): Json<Value>,
) -> ApiResult<Value> {
    let config = value.get("config").unwrap_or(&value);
    runtime.config.save_for_restart(config)?;
    runtime
        .bus
        .publish("config.saved", json!({"restart_required": true}))
        .await;
    Ok(Json(json!({"saved": true, "restart_required": true})))
}

async fn startup_get() -> ApiResult<Value> {
    Ok(Json(json!(eefn::startup_status("EEF Coordinator")?)))
}

async fn startup_set(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<StartupRequest>,
) -> ApiResult<Value> {
    let current = std::env::current_exe().map_err(anyhow::Error::from)?;
    let root = eefn::updater::installation_root(&current)?;
    let stable = root.join("eef.exe");
    let executable = if stable.is_file() { stable } else { current };
    let config = runtime
        .config
        .source()
        .context("EEF was not started with a configuration path")?;
    Ok(Json(json!(eefn::set_startup(
        "EEF Coordinator",
        &executable,
        &["--config".into(), config.display().to_string()],
        request.enabled,
    )?)))
}
async fn world(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(runtime.world.summary().await)
}

async fn chat(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<ChatRequest>,
) -> ApiResult<Value> {
    Ok(Json(
        json!({"reply": runtime.handle_message(&request.message).await?}),
    ))
}

async fn firmware_generate(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<FirmwareGenerateRequest>,
) -> ApiResult<Value> {
    Ok(Json(
        runtime
            .firmware
            .generate(
                &request.description,
                &request.node_id,
                &request.node_name,
                &request.version,
                request.config,
            )
            .await?,
    ))
}

async fn firmware_push(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<FirmwarePushRequest>,
) -> ApiResult<Value> {
    Ok(Json(
        runtime
            .firmware
            .push(&request.node_id, request.source, &request.version)
            .await?,
    ))
}

async fn firmware_list(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!({"firmware": runtime.firmware.list().await}))
}

async fn memory(State(runtime): State<Arc<Runtime>>) -> ApiResult<Value> {
    Ok(Json(json!({
        "identity": runtime.identity.summary(), "facts": runtime.memory.list_facts(100)?,
        "notes": runtime.memory.list_notes(100)?, "conversation": runtime.memory.recent_conversation(20)?,
    })))
}

async fn memory_reset(State(runtime): State<Arc<Runtime>>) -> ApiResult<Value> {
    runtime.memory.reset()?;
    runtime.working.lock().expect("working memory lock").clear();
    Ok(Json(json!({"reset": true, "identity_preserved": true})))
}

async fn tasks(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!({"plans": runtime.engine.list_plans()}))
}

async fn logs(State(runtime): State<Arc<Runtime>>, Query(query): Query<LogsQuery>) -> Json<Value> {
    Json(json!({"logs": runtime.bus.history(query.limit.unwrap_or(100).min(500)).await}))
}

async fn capabilities(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(
        json!({"capabilities": runtime.registry.all_capabilities(), "providers": runtime.registry.all_providers()}),
    )
}

async fn assistant_status(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!({"rules": runtime.assistant.list_rules(), "firings": runtime.assistant.firings()}))
}

async fn assistant_execute(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<InvokeRequest>,
) -> ApiResult<Value> {
    let data = runtime
        .dispatcher
        .run_capability(
            &request.capability,
            &request.action,
            request.params,
            request.constraints,
        )
        .await?;
    Ok(Json(json!({"success": true, "data": data})))
}

async fn rules_list(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!({"rules": runtime.assistant.list_rules()}))
}

async fn rules_create(
    State(runtime): State<Arc<Runtime>>,
    Json(rule): Json<ResponseRule>,
) -> ApiResult<Value> {
    runtime.assistant.add_rule(rule.clone())?;
    Ok(Json(json!({"rule": rule})))
}

async fn rules_delete(
    State(runtime): State<Arc<Runtime>>,
    Path(rule_id): Path<String>,
) -> Json<Value> {
    Json(json!({"removed": runtime.assistant.remove_rule(&rule_id)}))
}

async fn rules_toggle(
    State(runtime): State<Arc<Runtime>>,
    Path(rule_id): Path<String>,
) -> ApiResult<Value> {
    let enabled = runtime
        .assistant
        .toggle_rule(&rule_id)
        .ok_or_else(|| anyhow::anyhow!("rule not found"))?;
    Ok(Json(json!({"rule_id": rule_id, "enabled": enabled})))
}

async fn node_status(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    let connected = runtime.node_server.connected_nodes().await;
    let registered = runtime.node_server.registered_nodes().await;
    Json(json!({"nodes": connected, "registered": registered, "port": runtime.node_server.port()}))
}

async fn node_invoke(
    State(runtime): State<Arc<Runtime>>,
    Path(node_id): Path<String>,
    Json(request): Json<InvokeRequest>,
) -> ApiResult<Value> {
    let response = runtime
        .node_server
        .invoke_remote(
            &node_id,
            &request.capability,
            &request.action,
            request.params,
            Duration::from_secs_f64(request.timeout.clamp(0.1, 300.0)),
        )
        .await?;
    Ok(Json(response))
}

async fn update_status(State(runtime): State<Arc<Runtime>>) -> Json<Value> {
    Json(json!(runtime.update.state().await))
}

async fn update_check(State(runtime): State<Arc<Runtime>>) -> ApiResult<Value> {
    Ok(Json(json!(runtime.update.check().await?)))
}

async fn update_apply(State(runtime): State<Arc<Runtime>>) -> ApiResult<Value> {
    Ok(Json(json!(runtime.update.apply().await?)))
}

#[allow(dead_code)]
fn _assert_result_send(_: Result<()>) {}
