//! Optional browser adapter. Core services do not import this module.
pub use crate::service::NodeService as NodeDashboard;

#[cfg(feature = "dashboard")]
pub(crate) mod web {
    use crate::{NodeService, api::ApiError};
    use axum::response::{Html, IntoResponse};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    pub(crate) fn router(enabled: Arc<AtomicBool>) -> Router<Arc<NodeService>> {
        Router::new()
            .route("/", get(index))
            .route("/app.js", get(script))
            .route("/app.css", get(style))
            .route("/advanced/legacy", get(legacy))
            .route("/api/pick", post(pick))
            .route_layer(axum::middleware::from_fn_with_state(enabled, ui_guard))
    }
    async fn ui_guard(
        axum::extract::State(enabled): axum::extract::State<Arc<AtomicBool>>,
        request: axum::extract::Request,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        if enabled.load(Ordering::Relaxed) {
            next.run(request).await
        } else {
            axum::http::StatusCode::NOT_FOUND.into_response()
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

    async fn pick(Json(request): Json<Value>) -> Result<Json<Value>, ApiError> {
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
            return Err(anyhow::anyhow!(
                "The file picker could not open. Enter the path manually."
            )
            .into());
        }
        Ok(Json(
            json!({"path":String::from_utf8_lossy(&output.stdout).trim()}),
        ))
    }
}
