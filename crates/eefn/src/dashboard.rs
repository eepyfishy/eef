use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{info, warn};

const SECRET_PLACEHOLDER: &str = "__KEEP_EXISTING_SECRET__";

#[derive(Clone)]
pub struct NodeDashboard {
    config_path: PathBuf,
    node_id: String,
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
        })
    }

    pub async fn start(self: &Arc<Self>, host: &str, port: u16) -> Result<JoinHandle<()>> {
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .with_context(|| format!("bind EEFN dashboard {host}:{port}"))?;
        let actual_port = listener.local_addr()?.port();
        let app = Router::new()
            .route("/", get(index))
            .route("/api/status", get(status))
            .route("/api/config", get(config_get).put(config_save))
            .route("/api/startup", get(startup_get).put(startup_set))
            .with_state(self.clone());
        info!(host, port = actual_port, "EEFN dashboard listening");
        Ok(tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                warn!(%error, "EEFN dashboard stopped");
            }
        }))
    }

    fn read_config(&self) -> Result<Value> {
        if !self.config_path.is_file() {
            return Ok(json!({
                "node_id": self.node_id,
                "endpoints": [],
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

    fn save_config(&self, mut value: Value) -> Result<()> {
        if !value.is_object() {
            bail!("node configuration must be a JSON object")
        }
        let old = self.read_config().unwrap_or_else(|_| json!({}));
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
}

fn validate_config(value: &Value) -> Result<()> {
    if value
        .get("endpoints")
        .is_some_and(|endpoints| !endpoints.is_array())
    {
        bail!("endpoints must be an array")
    }
    if let Some(provider) = value.pointer("/models/provider").and_then(Value::as_str) {
        if !matches!(provider, "auto" | "ollama" | "llamacpp") {
            bail!("models.provider must be auto, ollama, or llamacpp")
        }
    }
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/node.html"))
}

async fn status(State(state): State<Arc<NodeDashboard>>) -> Json<Value> {
    Json(json!({
        "node_id": state.node_id,
        "runtime": "rust",
        "version": crate::VERSION,
        "node_alive": true,
        "coordinator_required_for_node": false,
        "coordinator_required_for_orchestration": true,
    }))
}

async fn config_get(
    State(state): State<Arc<NodeDashboard>>,
) -> Result<Json<Value>, DashboardError> {
    let mut config = state.read_config()?;
    if config.get("psk").and_then(Value::as_str).is_some() {
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
    Ok(Json(json!({"saved": true, "restart_required": true})))
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

    #[test]
    fn rejects_unknown_model_provider() {
        assert!(validate_config(&json!({"models": {"provider": "magic"}})).is_err());
        assert!(validate_config(&json!({"models": {"provider": "auto"}})).is_ok());
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
