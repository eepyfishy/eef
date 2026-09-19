#![cfg_attr(windows, windows_subsystem = "windows")]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use eefn::{
    CoordinatorEndpoint, ModelServer, ModelSlot, NodeClient, NodeClientConfig, NodeEngine,
    NodePolicy, NodeService, PythonRuntime, SelectedModel,
};
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Parser)]
#[command(name = "eefn", version, about = "Native EEF node")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(long, global = true, help = "Print command results as JSON")]
    json: bool,
    #[arg(
        long,
        help = "Run local command/API services without serving the browser UI"
    )]
    no_ui: bool,
    #[arg(long)]
    open_dashboard: bool,
    #[arg(long, global = true, default_value = "config.json")]
    config: PathBuf,
    #[arg(long = "connect")]
    endpoints: Vec<String>,
    #[arg(long = "id")]
    node_id: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, env = "EEF_NODE_PSK")]
    psk: Option<String>,
    #[arg(long)]
    allow_write: bool,
    #[arg(long = "allow-root")]
    allowed_roots: Vec<PathBuf>,
    #[arg(long)]
    python: Option<PathBuf>,
    #[arg(long = "python-plugin")]
    python_plugins: Vec<PathBuf>,
    #[arg(long)]
    check_update: bool,
    #[arg(long)]
    rollback: bool,
    #[arg(
        long,
        help = "Submit one user message through this node, print the result, and exit"
    )]
    ask: Option<String>,
    #[arg(long, help = "List installed Ollama model IDs without selecting them")]
    list_ollama_models: bool,
    #[arg(
        long = "select-model",
        value_name = "MODEL_ID=TEXT|VLM",
        help = "Advertise an installed Ollama model for this run; may be repeated"
    )]
    selected_models: Vec<String>,
    #[arg(long, env = "EEF_NODE_OLLAMA")]
    ollama_url: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Preview an untrusted interpretation locally; never submit or execute work.
    Requests {
        #[command(subcommand)]
        command: eefn::request_preview::RequestAction,
    },
    /// Control the outgoing EEF connection while keeping local commands available.
    Connection {
        #[command(subcommand)]
        command: eefn::connection_commands::ConnectionCommand,
    },
    /// Submit and control this node's durable jobs through its running connection.
    Jobs {
        #[command(subcommand)]
        command: eefn::job_commands::JobCommand,
    },
    /// Restart this node app and optionally verify its new runtime (not Windows).
    Restart {
        #[arg(long, default_value_t = 30)]
        wait_seconds: u64,
    },
    /// Inspect/select models and explicitly manage downloads; no automatic restart.
    Models {
        #[command(subcommand)]
        command: eefn::model_downloads::NodeModelsAction,
    },
    /// Inspect or configure node identity and advertised network metadata.
    Network {
        #[command(subcommand)]
        command: NetworkAction,
    },
}

async fn execute_model_command(
    args: &Args,
    action: &eefn::model_cli::ModelsAction,
) -> Result<serde_json::Value> {
    use eefn::model_selection::ModelCommandRequest;
    let command = action.to_command(true)?;
    let lock = eefn::setup::instance_lock(&args.config)?;
    let file = load_file(&args.config)?;
    if file.node_id.is_empty() {
        bail!("No saved node identity; configure the node with network set first")
    }
    let request = ModelCommandRequest {
        schema_version: 1,
        expected_node_id: file.node_id.clone(),
        command,
    };
    if lock.is_some() {
        let service = NodeService::new(args.config.clone(), file.node_id);
        let mut result = service.model_command(request)?;
        result["running"] = serde_json::json!(false);
        result["restart_required"] = serde_json::json!(false);
        result["note"] = serde_json::json!(
            "Saved for next node start. No backend download or model load was performed."
        );
        return Ok(result);
    }
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &request.expected_node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!("http://{}/api/commands/models", address))
        .timeout(Duration::from_secs(15))
        .json(&request)
        .send()
        .await?;
    let ok = response.status().is_success();
    let mut result = eefn::model_manager::bounded_json(response).await?;
    if !ok {
        bail!(
            "{}",
            result["error"]
                .as_str()
                .unwrap_or("local model command failed")
        )
    }
    result["running"] = serde_json::json!(true);
    Ok(result)
}

async fn execute_request_preview(args: &Args, text: &str) -> Result<serde_json::Value> {
    use serde_json::json;
    eefn::interpretation::messages(text, &[])?;
    if eefn::setup::instance_lock(&args.config)?.is_some() {
        return Ok(
            json!({"schema_version":1,"report_type":"request_preview","success":false,
            "error_code":"node_not_running","execution_authorized":false,"dispatched":false,
            "error":"Start the node first. Preview does not start a second runtime."}),
        );
    }
    let file = load_file(&args.config)?;
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &file.node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let request = eefn::request_preview::PreviewRequest {
        schema_version: 1,
        expected_node_id: file.node_id.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        text: text.into(),
    };
    let exchange = async {
        let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build()?
            .post(format!("http://{address}/api/commands/requests/preview"))
            .timeout(Duration::from_secs(25)).json(&request).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(json!({"schema_version":1,"report_type":"request_preview","success":false,
                "error_code":"unsupported_command","execution_authorized":false,"dispatched":false,
                "error":"This running node does not support preview. No chat/job fallback was attempted."}));
        }
        let status = response.status();
        let report = eefn::model_manager::bounded_json(response).await?;
        if !status.is_success() || report["schema_version"] != 1 || report["report_type"] != "request_preview"
            || report["node_id"] != file.node_id || report["request_id"] != request.request_id
            || !report["success"].is_boolean() || report["execution_authorized"] != false || report["dispatched"] != false {
            bail!("Unrecognized request-preview response");
        }
        Ok::<serde_json::Value, anyhow::Error>(report)
    }.await;
    Ok(exchange.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"request_preview","success":false,
        "request_id":request.request_id,"error_code":"preview_unconfirmed","execution_authorized":false,
        "dispatched":false,"automatically_retried":false,
        "error":"No confirmed preview reply. Inference may have run; no job or action was requested. No automatic retry."})))
}

async fn execute_download_command(
    args: &Args,
    command: eefn::model_downloads::DownloadCommand,
) -> Result<serde_json::Value> {
    use serde_json::json;
    command.validate()?;
    let operation_id = command.operation_id().map(str::to_owned);
    let mutating = command.mutating();
    // A short-lived CLI must never own a transfer or launch a second node.
    if eefn::setup::instance_lock(&args.config)?.is_some() {
        return Ok(
            json!({"schema_version":1,"report_type":"model_download","success":false,
            "operation_id":operation_id,"error_code":"node_not_running","request_sent":false,
            "error":"Start the node first. Download commands do not launch another node."}),
        );
    }
    let file = load_file(&args.config)?;
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &file.node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let request = eefn::model_downloads::DownloadRequest {
        schema_version: 1,
        expected_node_id: file.node_id.clone(),
        command,
    };
    let exchange = async {
        let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build()?
            .post(format!("http://{address}/api/commands/model-downloads"))
            .timeout(Duration::from_secs(15)).json(&request).send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(json!({"schema_version":1,"report_type":"model_download","success":false,
                "operation_id":operation_id,"error_code":"unsupported_command","error":"This running node does not support download commands. Upgrade it explicitly; no legacy action was retried."}));
        }
        let report = eefn::model_manager::bounded_json(response).await?;
        if !status.is_success() || report["schema_version"] != 1 || report["report_type"] != "model_download"
            || report["node_id"] != file.node_id || report["operation_id"] != json!(operation_id)
            || !report["success"].is_boolean() {
            bail!("Unrecognized download-command response");
        }
        Ok::<serde_json::Value, anyhow::Error>(report)
    }.await;
    Ok(match exchange {
        Ok(report) => report,
        Err(_) => {
            json!({"schema_version":1,"report_type":"model_download","success":false,"node_id":file.node_id,
            "operation_id":operation_id,"error_code":if mutating {"outcome_unknown"} else {"inspection_failed"},
            "outcome_unknown":mutating,"automatically_retried":false,
            "error":"No confirmed command reply. Inspect download-status with this receipt before deciding whether to retry. Only the latest in-memory transfer is retained."})
        }
    })
}

async fn execute_connection_command(
    args: &Args,
    command: &eefn::connection_commands::ConnectionCommand,
) -> Result<serde_json::Value> {
    if eefn::setup::instance_lock(&args.config)?.is_some() {
        bail!("node is not running; connection controls do not launch another node")
    }
    let file = load_file(&args.config)?;
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &file.node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!("http://{address}/api/commands/connection"))
        .timeout(Duration::from_secs(15))
        .json(&eefn::connection_commands::ConnectionRequest {
            schema_version: 1,
            expected_node_id: file.node_id,
            command: command.clone(),
        })
        .send()
        .await
        .context("Connection command reply unavailable; inspect status before retrying")?;
    let ok = response.status().is_success();
    let value = eefn::model_manager::bounded_json(response).await?;
    if !ok {
        bail!(
            "{}",
            value["error"]
                .as_str()
                .unwrap_or("connection command unavailable; update the node")
        )
    }
    Ok(value)
}

async fn execute_job_command(
    args: &Args,
    command: &eefn::job_commands::JobCommand,
) -> Result<serde_json::Value> {
    use serde_json::json;
    command.submission()?;
    if eefn::setup::instance_lock(&args.config)?.is_some() {
        bail!("node is not running; job commands do not launch a node or a second connection")
    }
    let file = load_file(&args.config)?;
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &file.node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let request = eefn::job_commands::JobRequest {
        schema_version: 1,
        expected_node_id: file.node_id.clone(),
        operation_id: Some(uuid::Uuid::new_v4().to_string()),
        command: command.clone(),
    };
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!("http://{address}/api/commands/jobs"))
        .timeout(Duration::from_secs(125))
        .json(&request)
        .send()
        .await;
    if let Ok(response) = response {
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("running node does not support typed job commands; update the node first")
        }
        let ok = response.status().is_success();
        if let Ok(value) = eefn::model_manager::bounded_json(response).await {
            if ok
                && value["schema_version"] == 1
                && value["report_type"] == "node_job_command"
                && value["node_id"] == file.node_id
                && value["operation_id"].as_str() == request.operation_id.as_deref()
            {
                return Ok(value);
            }
        }
    }
    Ok(
        json!({"schema_version":1,"success":false,"node_id":file.node_id,
        "report_type":"node_job_command","operation_id":request.operation_id,
        "error_code":"local_job_reply_unconfirmed","acknowledged":false,"outcome_unknown":command.mutates(),
        "note":"No valid local job reply. For creation, use jobs find --operation-id with this receipt, then inspect job status. No automatic resubmission was sent; no match is not proof that work was never accepted."}),
    )
}

async fn execute_restart(args: &Args, wait_seconds: u64) -> Result<serde_json::Value> {
    use serde_json::json;
    if wait_seconds > 60 {
        bail!("wait_seconds must be 0-60")
    }
    if eefn::setup::instance_lock(&args.config)?.is_some() {
        bail!("node is not running; restart does not launch a stopped node")
    }
    let file = load_file(&args.config)?;
    let node_id = file.node_id;
    let local = file.dashboard.unwrap_or_default();
    let address = || {
        eefn::local_commands::endpoint(
            &args.config,
            &node_id,
            &local.host,
            local.port,
            local.enabled,
        )
    };
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(3))
        .build()?;
    let diagnostics = async || -> Result<serde_json::Value> {
        let response = http
            .get(format!("http://{}/api/diagnostics", address()?))
            .send()
            .await?
            .error_for_status()?;
        let value = eefn::model_manager::bounded_json(response).await?;
        if value["success"] != true || value["node_id"] != node_id {
            bail!("local diagnostics do not match this node")
        }
        Ok(value)
    };
    let before = diagnostics().await?;
    let previous = before["runtime_id"].as_str().map(str::to_owned);
    let mut result = json!({"schema_version":1,"success":false,"report_type":"local_node_restart","node_id":node_id,
        "operation_id":uuid::Uuid::new_v4().to_string(),"previous_runtime_id":previous,"restart_requested":true,"acknowledged":false,"completed":false,"outcome_unknown":false});
    let reply = http
        .post(format!("http://{}/api/commands/restart", address()?))
        .json(&eefn::local_commands::RestartRequest {
            schema_version: 1,
            expected_node_id: node_id.clone(),
            expected_runtime_id: previous.clone(),
        })
        .send()
        .await;
    match reply {
        Ok(response) if response.status().is_client_error() => {
            result["error_code"] = json!("restart_rejected");
            result["note"] =
                json!("Node rejected restart or does not support this command. No retry was sent.");
            return Ok(result);
        }
        Ok(response) if response.status().is_success() => {
            if let Ok(value) = eefn::model_manager::bounded_json(response).await {
                if value["success"] == true
                    && value["node_id"] == node_id
                    && value["restart_requested"] == true
                    && value["previous_runtime_id"] == json!(previous)
                {
                    result["acknowledged"] = json!(true);
                }
            }
        }
        _ => {}
    }
    if wait_seconds == 0 && result["acknowledged"] == true {
        result["success"] = json!(true);
        result["note"] = json!("Restart acknowledged; completion was not requested or verified.");
        return Ok(result);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait_seconds);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(current)) = tokio::time::timeout_at(deadline, diagnostics()).await {
            if let Some(runtime) = current["runtime_id"]
                .as_str()
                .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            {
                if previous.as_deref() != Some(runtime) {
                    result["success"] = json!(true);
                    result["completed"] = json!(true);
                    result["runtime_id"] = json!(runtime);
                    result["pending_restart"] = current["pending_restart"].clone();
                    result["note"] = json!(
                        "Same stable node ID initialized a new runtime. Coordinator reconnection, jobs and model readiness are not verified."
                    );
                    return Ok(result);
                }
            }
        }
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(250)).min(deadline),
        )
        .await;
    }
    result["error_code"] = json!("restart_unconfirmed");
    result["outcome_unknown"] = json!(true);
    result["note"] = json!(
        "Restart was not confirmed; it may still be running or may have disabled its local API. Inspect diagnostics before retrying. No automatic retry was sent."
    );
    Ok(result)
}

#[derive(Debug, Subcommand)]
enum NetworkAction {
    Show,
    /// Export a minimal connection report; does not upload anything.
    Diagnose,
    /// Inspect only peers disclosed by EEF's owner; does not authorize access.
    Peers {
        #[arg(long, conflicts_with = "after")]
        node: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 32)]
        limit: usize,
    },
    Set {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, conflicts_with = "clear_address")]
        advertise_address: Option<String>,
        #[arg(long)]
        clear_address: bool,
        #[arg(
            long,
            requires = "coordinator_address",
            conflicts_with = "clear_coordinator"
        )]
        coordinator_id: Option<String>,
        #[arg(
            long,
            requires = "coordinator_id",
            conflicts_with = "clear_coordinator"
        )]
        coordinator_address: Option<String>,
        #[arg(long, value_parser = ["active", "standby", "unknown"], requires = "coordinator_id")]
        coordinator_state: Option<String>,
        #[arg(long)]
        clear_coordinator: bool,
    },
}

async fn execute_command(args: &Args, command: &Commands) -> Result<serde_json::Value> {
    if let Commands::Connection { command } = command {
        return execute_connection_command(args, command).await;
    }
    if let Commands::Jobs { command } = command {
        return execute_job_command(args, command).await;
    }
    if let Commands::Restart { wait_seconds } = command {
        return execute_restart(args, *wait_seconds).await;
    }
    use eefn::network::{
        CommandRequest, CoordinatorAdvertisement, CoordinatorState, NetworkChanges, NetworkCommand,
    };
    if let Commands::Models { command } = command {
        return match command {
            eefn::model_downloads::NodeModelsAction::Selection(action) => {
                execute_model_command(args, action).await
            }
            action => {
                execute_download_command(args, action.download_command().expect("download action"))
                    .await
            }
        };
    }
    if let Commands::Requests {
        command: eefn::request_preview::RequestAction::Preview { text },
    } = command
    {
        return execute_request_preview(args, text).await;
    }
    let Commands::Network { command } = command else {
        unreachable!()
    };
    let command = match command {
        NetworkAction::Show => NetworkCommand::Show,
        NetworkAction::Diagnose => NetworkCommand::Diagnose,
        NetworkAction::Peers { node, after, limit } => {
            let query = eefn::network::PeerQuery {
                node_id: node.clone(),
                after: after.clone(),
                limit: *limit,
            };
            query.validate()?;
            NetworkCommand::Peers { query }
        }
        NetworkAction::Set {
            name,
            advertise_address,
            clear_address,
            coordinator_id,
            coordinator_address,
            coordinator_state,
            clear_coordinator,
        } => {
            let coordinator = match (coordinator_id, coordinator_address) {
                (Some(id), Some(address)) => Some(CoordinatorAdvertisement {
                    coordinator_id: id.clone(),
                    address: address.clone(),
                    state: match coordinator_state.as_deref() {
                        Some("active") => CoordinatorState::Active,
                        Some("standby") => CoordinatorState::Standby,
                        _ => CoordinatorState::Unknown,
                    },
                }),
                (None, None) => None,
                _ => bail!("coordinator ID and address are required together"),
            };
            let changes = NetworkChanges {
                name: name.clone(),
                advertised_address: advertise_address.clone(),
                clear_advertised_address: *clear_address,
                coordinator,
                clear_coordinator: *clear_coordinator,
            };
            changes.apply(&mut serde_json::json!({}))?;
            NetworkCommand::Set { changes }
        }
    };
    let lock = eefn::setup::instance_lock(&args.config)?;
    if lock.is_some() {
        // Offline commands never start discovery, inference, plugins or a listener.
        if matches!(command, NetworkCommand::Set { .. }) {
            eefn::setup::initialize_identity(&args.config)?;
        }
        let file = load_file(&args.config)?;
        if file.node_id.is_empty() {
            bail!("No saved node identity; configure the node with network set first")
        }
        let service = eefn::NodeService::new(args.config.clone(), file.node_id.clone());
        let mut result = service.network_command(CommandRequest {
            schema_version: 1,
            expected_node_id: file.node_id,
            command,
        })?;
        result["running"] = serde_json::json!(false);
        result["restart_required"] = serde_json::json!(false);
        if result["report_type"] == "node_diagnostics" {
            result["connection_state"] = serde_json::json!("stopped");
            result["metrics_available"] = serde_json::json!(false);
            for key in [
                "service_uptime_ms",
                "connection_checks",
                "successful_connections",
                "failed_connection_attempts",
                "model_count",
                "capability_count",
            ] {
                result[key] = serde_json::Value::Null;
            }
        }
        return Ok(result);
    }
    // Ask the existing instance to mutate its config under its own lock.
    let file = load_file(&args.config)?;
    let local = file.dashboard.unwrap_or_default();
    let address = eefn::local_commands::endpoint(
        &args.config,
        &file.node_id,
        &local.host,
        local.port,
        local.enabled,
    )?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .post(format!("http://{}/api/commands/network", address))
        .timeout(Duration::from_secs(15))
        .json(&CommandRequest {
            schema_version: 1,
            expected_node_id: file.node_id,
            command,
        })
        .send()
        .await?;
    let ok = response.status().is_success();
    let mut result: serde_json::Value = eefn::model_manager::bounded_json(response).await?;
    if !ok {
        bail!(
            "{}",
            result["error"].as_str().unwrap_or("local command failed")
        )
    }
    result["running"] = serde_json::json!(true);
    Ok(result)
}

#[derive(Clone, Debug, Deserialize)]
struct FileConfig {
    #[serde(default)]
    network: eefn::network::NetworkAdvertisement,
    #[serde(default)]
    metadata: eefn::context::NodeMetadata,
    #[serde(default)]
    endpoints: Vec<CoordinatorEndpoint>,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    psk: String,
    #[serde(default)]
    heartbeat_seconds: Option<f64>,
    #[serde(default)]
    allow_write: bool,
    #[serde(default)]
    allowed_roots: Vec<PathBuf>,
    #[serde(default)]
    python: Option<PathBuf>,
    #[serde(default)]
    python_plugins: Vec<PathBuf>,
    #[serde(default)]
    update: Option<UpdateConfig>,
    #[serde(default)]
    models: Option<ModelsConfig>,
    #[serde(default)]
    dashboard: Option<DashboardConfig>,
    #[serde(default)]
    permissions: NodePolicy,
    #[serde(default = "enabled")]
    connection_enabled: bool,
}

impl Default for FileConfig {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({})).expect("default node configuration")
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
struct UpdateConfig {
    manifest_url: Option<String>,
    #[serde(default = "prompt_policy")]
    policy: String,
    #[serde(default = "update_interval")]
    check_interval_seconds: u64,
}

fn prompt_policy() -> String {
    "prompt".into()
}
const fn update_interval() -> u64 {
    21_600
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ModelsConfig {
    #[serde(default = "auto_provider")]
    provider: String,
    ollama: Option<OllamaConfig>,
    llamacpp: Option<LlamaConfig>,
}

fn auto_provider() -> String {
    "auto".into()
}

#[derive(Clone, Debug, Deserialize)]
struct DashboardConfig {
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default = "enabled")]
    ui_enabled: bool,
    #[serde(default = "dashboard_host")]
    host: String,
    #[serde(default = "dashboard_port")]
    port: u16,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ui_enabled: true,
            host: dashboard_host(),
            port: dashboard_port(),
        }
    }
}

const fn enabled() -> bool {
    true
}
fn dashboard_host() -> String {
    "127.0.0.1".into()
}
const fn dashboard_port() -> u16 {
    51336
}

#[derive(Clone, Debug, Default, Deserialize)]
struct OllamaConfig {
    base_url: Option<String>,
    #[serde(default)]
    selected: Vec<SelectedModel>,
}

#[derive(Clone, Debug, Deserialize)]
struct LlamaConfig {
    #[serde(default = "default_llama_binary")]
    binary: PathBuf,
    #[serde(default)]
    slots: Vec<ModelSlot>,
}

fn default_llama_binary() -> PathBuf {
    PathBuf::from("tools/llama-server.exe")
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "eefn=info".into()),
        )
        .init();
    let executable = std::env::current_exe()?;
    let install_dir = eefn::updater::installation_root(&executable)?;
    if eefn::updater::handoff_if_selected(&install_dir, eefn::VERSION, "eefn")? {
        return Ok(());
    }
    let mut args = Args::parse();
    if args.config == PathBuf::from("config.json") && install_dir.join("config.json").is_file() {
        args.config = install_dir.join("config.json");
    }
    if let Some(command) = &args.command {
        match execute_command(&args, command).await {
            Ok(value) => {
                if args.json {
                    println!("{}", serde_json::to_string(&value)?);
                } else if value.get("nodes").is_some() || value.get("report_type").is_some() {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                } else {
                    println!(
                        "Node: {} ({})",
                        value["display_name"].as_str().unwrap_or(""),
                        value["node_id"].as_str().unwrap_or("")
                    );
                    println!(
                        "Advertised address: {}",
                        value
                            .pointer("/network/advertised_address")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("not configured")
                    );
                    println!("{}", serde_json::to_string_pretty(&value)?);
                }
                if value["success"].as_bool() == Some(false) {
                    bail!("command did not complete; inspect the result before retrying")
                }
                return Ok(());
            }
            Err(error) => {
                if args.json {
                    println!(
                        "{}",
                        serde_json::json!({"schema_version":1,"success":false,"error_code":"command_failed","error":error.to_string()})
                    );
                }
                return Err(error);
            }
        }
    }
    // Submit through the already running node instead of registering its identity
    // a second time. Hold the instance lock during standalone --ask as well.
    let _ask_instance = if let Some(message) = &args.ask {
        match eefn::setup::instance_lock(&args.config)? {
            Some(lock) => Some(lock),
            None => {
                let file = load_file(&args.config)?;
                let dashboard = file.dashboard.unwrap_or_default();
                let address = eefn::local_commands::endpoint(
                    &args.config,
                    &file.node_id,
                    &dashboard.host,
                    dashboard.port,
                    dashboard.enabled,
                )?;
                let url = format!("http://{}/api/chat", address);
                let response = reqwest::Client::new()
                    .post(url)
                    .timeout(Duration::from_secs(125))
                    .json(&serde_json::json!({"message":message}))
                    .send()
                    .await?;
                let success = response.status().is_success();
                let value: serde_json::Value = response.json().await?;
                if !success {
                    bail!(
                        "{}",
                        value["error"]
                            .as_str()
                            .unwrap_or("running node rejected the message")
                    )
                }
                println!("{}", serde_json::to_string_pretty(&value)?);
                return Ok(());
            }
        }
    } else {
        None
    };
    let _instance =
        if args.ask.is_none() && !args.check_update && !args.rollback && !args.list_ollama_models {
            match eefn::setup::instance_lock(&args.config)? {
                Some(lock) => Some(lock),
                None => {
                    let config = load_file(&args.config)?.dashboard.unwrap_or_default();
                    if args.open_dashboard
                        && !args.no_ui
                        && config.ui_enabled
                        && cfg!(feature = "dashboard")
                    {
                        eefn::setup::open_dashboard(&config.host, config.port);
                    }
                    return Ok(());
                }
            }
        } else {
            None
        };
    let initial = eefn::setup::initialize_identity(&args.config)?;
    let _ = eefn::setup::pair_local(&args.config)?;
    let file = load_file(&args.config)?;
    let mut dashboard_config = file.dashboard.unwrap_or_default();
    let dashboard = NodeService::new(
        args.config.clone(),
        initial["node_id"].as_str().unwrap().into(),
    );
    let interactive =
        args.ask.is_none() && !args.check_update && !args.rollback && !args.list_ollama_models;
    let mut dashboard_task = if dashboard_config.enabled && interactive {
        Some(
            dashboard
                .start_api(
                    &dashboard_config.host,
                    dashboard_config.port,
                    !args.no_ui && dashboard_config.ui_enabled,
                )
                .await?,
        )
    } else {
        None
    };
    if args.open_dashboard
        && !args.no_ui
        && dashboard_config.ui_enabled
        && cfg!(feature = "dashboard")
        && dashboard_task.is_some()
    {
        eefn::setup::open_dashboard(&dashboard_config.host, dashboard_config.port);
    }
    let discovery = if interactive {
        let dashboard = dashboard.clone();
        let path = args.config.clone();
        Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                if let Err(error) = dashboard.discover_local(&path) {
                    warn!(%error,"local discovery");
                }
            }
        }))
    } else {
        None
    };
    let mut handoff = false;
    loop {
        if let Ok(file) = load_file(&args.config) {
            let next = file.dashboard.unwrap_or_default();
            if let Some(api) = &dashboard_task {
                api.set_ui_enabled(!args.no_ui && next.ui_enabled);
            }
            dashboard_config.ui_enabled = next.ui_enabled;
            if interactive
                && (next.host != dashboard_config.host
                    || next.port != dashboard_config.port
                    || next.enabled != dashboard_config.enabled)
            {
                let replacement = if next.enabled {
                    Some(
                        dashboard
                            .start_api(&next.host, next.port, !args.no_ui && next.ui_enabled)
                            .await?,
                    )
                } else {
                    None
                };
                if let Some(task) = dashboard_task.take() {
                    task.abort();
                }
                dashboard_task = replacement;
                dashboard_config = next;
            }
        }
        {
            let mut status = dashboard.live.lock().unwrap();
            status["connection"] = serde_json::json!({"state":"starting"});
            status["startup_issues"] = serde_json::json!([]);
        }
        tokio::select! {
            result = run_node(&args, install_dir.clone(), dashboard.clone()) => {
                if !interactive { return result; }
                if let Err(error)=result {
                    warn!(%error,"device needs attention");
                    dashboard.live.lock().unwrap()["connection"]=serde_json::json!({"state":"error","last_error":format!("{error:#}")});
                }
                tokio::select! { _=dashboard.restart.notified()=>{}, _=tokio::signal::ctrl_c()=>break }
            }
            _ = dashboard.restart.notified() => {},
            _ = tokio::signal::ctrl_c() => break,
        }
        if std::fs::read_to_string(install_dir.join("current.txt"))
            .is_ok_and(|v| !v.trim().is_empty() && v.trim() != eefn::VERSION)
        {
            handoff = true;
            break;
        }
    }
    if let Some(task) = dashboard_task {
        task.abort();
    }
    if let Some(task) = discovery {
        task.abort();
    }
    drop(_instance);
    if handoff {
        eefn::updater::handoff_if_selected(&install_dir, eefn::VERSION, "eefn")?;
    }
    Ok(())
}

struct AbortTask(tokio::task::JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn run_node(args: &Args, install_dir: PathBuf, dashboard: Arc<NodeService>) -> Result<()> {
    let mut file = load_file(&args.config)?;
    merge_args(&mut file, &args);
    file.network.validate()?;

    if args.rollback {
        let version = eefn::updater::rollback(&install_dir)?;
        println!("rolled back to {version}");
        return Ok(());
    }
    let manifest = file
        .update
        .as_ref()
        .and_then(|update| update.manifest_url.clone());
    if args.check_update {
        let manifest = manifest.context("no update.manifest_url configured")?;
        let current = std::fs::read_to_string(install_dir.join("current.txt"))
            .unwrap_or_else(|_| eefn::VERSION.into());
        println!(
            "{}",
            serde_json::to_string_pretty(
                &eefn::updater::check(&manifest, current.trim(), Duration::from_secs(30)).await?
            )?
        );
        return Ok(());
    }

    let ollama_url = args
        .ollama_url
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            file.models
                .as_ref()
                .and_then(|models| models.ollama.as_ref())
                .and_then(|ollama| ollama.base_url.clone())
        })
        .unwrap_or_else(|| "http://127.0.0.1:11434".into());
    if args.list_ollama_models {
        let models = eefn::installed_ollama_models(&reqwest::Client::new(), &ollama_url)
            .await
            .with_context(|| format!("could not query Ollama at {ollama_url}"))?;
        println!("{}", serde_json::to_string_pretty(&models)?);
        return Ok(());
    }

    if file.node_id.is_empty() {
        file.node_id = default_node_id()
    }
    if file.name.is_empty() {
        file.name = file.node_id.clone()
    }

    if !file.endpoints.is_empty() {
        validate_secret(&file.psk)?;
    } else if args.ask.is_some() {
        bail!("--ask requires at least one configured EEF coordinator")
    } else {
        warn!("no EEF coordinator configured; node remains available through its local dashboard");
    }

    let ollama_models = if args.selected_models.is_empty() {
        file.models
            .as_ref()
            .and_then(|models| models.ollama.as_ref())
            .map(|ollama| ollama.selected.clone())
            .unwrap_or_default()
    } else {
        args.selected_models
            .iter()
            .map(|value| parse_selected_model(value))
            .collect::<Result<Vec<_>>>()?
    };
    let provider = file
        .models
        .as_ref()
        .map(|models| models.provider.trim().to_ascii_lowercase())
        .filter(|provider| !provider.is_empty())
        .unwrap_or_else(auto_provider);
    if !matches!(provider.as_str(), "auto" | "ollama" | "llamacpp") {
        bail!("models.provider must be auto, ollama, or llamacpp")
    }
    let ollama_available = eefn::installed_ollama_models(&reqwest::Client::new(), &ollama_url)
        .await
        .is_ok();
    let use_ollama = provider == "ollama" || (provider == "auto" && ollama_available);
    let use_llamacpp = provider == "llamacpp" || (provider == "auto" && !ollama_available);
    info!(%provider, ollama_available, use_ollama, use_llamacpp, "model provider selected");

    let model_server = if use_llamacpp {
        file.models
            .as_ref()
            .and_then(|models| models.llamacpp.as_ref())
            .map(|config| {
                ModelServer::new(
                    resolve_binary(&args.config, &install_dir, &config.binary),
                    config
                        .slots
                        .iter()
                        .cloned()
                        .map(|mut slot| {
                            slot.model_path = resolve_relative(&args.config, &slot.model_path);
                            if let Some(path) = &slot.mmproj_path {
                                slot.mmproj_path = Some(resolve_relative(&args.config, path));
                            }
                            slot
                        })
                        .collect(),
                )
            })
    } else {
        None
    };

    let active_ollama_models = if use_ollama {
        ollama_models
    } else {
        Vec::new()
    };
    let mut permissions = file.permissions.clone();
    if !file.allowed_roots.is_empty() {
        permissions.filesystem.roots = file.allowed_roots.clone();
        permissions.filesystem.read = true;
    }
    if file.allow_write {
        permissions.filesystem.write = true;
    }
    let media = permissions.media.clone();
    let mut engine = NodeEngine::new(false, vec![], install_dir.clone())?
        .with_resources(file.node_id.clone(), file.metadata.clone())?
        .with_service(dashboard.clone())
        .with_policy(permissions)?
        .with_update_manifest(manifest)
        .with_ollama(ollama_url.clone(), active_ollama_models.iter().cloned())?;
    if let Some(server) = &model_server {
        engine = engine.with_model_server(server.clone())
    }
    let hardware = eefn::setup::hardware().await;
    let camera_detected = hardware["inventory_available"].as_bool() != Some(true)
        || hardware["devices"].as_array().is_some_and(|list| {
            list.iter()
                .any(|device| matches!(device["PNPClass"].as_str(), Some("Camera" | "Image")))
        });
    let builtin_plugins = [
        (media.microphone, "microphone.py"),
        (media.audio_output, "audio_output.py"),
        (media.tts, "tts.py"),
        (media.camera && camera_detected, "camera.py"),
        (media.screen_capture, "screen_capture.py"),
        (media.input_control, "input_control.py"),
    ];
    if !file.python_plugins.is_empty() || builtin_plugins.iter().any(|(enabled, _)| *enabled) {
        let runtime = match PythonRuntime::discover(file.python.clone()) {
            Some(runtime) if runtime.check_available().await.is_ok() => Some(Arc::new(runtime)),
            _ => None,
        };
        if let Some(runtime) = runtime {
            info!(python = %runtime.executable().display(), "Python plugin runtime enabled");
            for (_, filename) in builtin_plugins.iter().filter(|(enabled, _)| *enabled) {
                let load = async {
                    let path = resolve_builtin_plugin(&install_dir, filename)?;
                    engine.add_python_plugin(runtime.clone(), path).await
                }
                .await;
                if let Err(error) = load {
                    warn!(plugin = %filename, %error, "built-in plugin unavailable; node core will remain connected");
                    dashboard.record_startup_issue(
                        eefn::service::StartupIssue::BuiltinPluginUnavailable,
                    );
                }
            }
            for plugin in &file.python_plugins {
                let path = resolve_relative(&args.config, plugin);
                if let Err(error) = engine
                    .add_python_plugin(runtime.clone(), path.clone())
                    .await
                {
                    warn!(plugin = %path.display(), %error, "Python plugin not loaded");
                    dashboard
                        .record_startup_issue(eefn::service::StartupIssue::CustomPluginUnavailable);
                }
            }
        } else {
            warn!(
                "Python runtime unavailable; node core will remain connected without Python capabilities"
            );
            dashboard.record_startup_issue(eefn::service::StartupIssue::PythonRuntimeUnavailable);
        }
    }
    let engine = Arc::new(engine);
    let runtime_id = uuid::Uuid::new_v4().to_string();
    {
        let mut status = dashboard.live.lock().unwrap();
        status["hardware"] = hardware;
        status["runtime_id"] = serde_json::json!(runtime_id);
        status["metadata"] = serde_json::json!(file.metadata.advertised(&engine.capabilities()));
        status["network"] = serde_json::json!(file.network);
        status["name"] = serde_json::json!(file.name);
        status["permissions"] = serde_json::json!(file.permissions);
        status["pending_restart"] = serde_json::json!(false);
        status["models"] = serde_json::json!([]);
        status["capabilities"] = serde_json::json!(engine.capabilities());
        status["connection"] =
            serde_json::json!({"state":if file.connection_enabled {"waiting"} else {"paused"}});
    }
    let _preview_runtime = dashboard.attach_preview_runtime(&engine, runtime_id);
    let client = if file.endpoints.is_empty() || !file.connection_enabled {
        None
    } else {
        Some(
            NodeClient::new(
                NodeClientConfig {
                    network: file.network,
                    metadata: file.metadata,
                    endpoints: file.endpoints,
                    node_id: file.node_id,
                    name: file.name,
                    psk: file.psk,
                    heartbeat_seconds: file.heartbeat_seconds.unwrap_or(3.0),
                    allow_write: file.allow_write,
                    allowed_roots: file.allowed_roots,
                    python: file.python,
                    python_plugins: file.python_plugins,
                    update_manifest: file
                        .update
                        .as_ref()
                        .and_then(|update| update.manifest_url.clone()),
                    ollama_url,
                    ollama_models: active_ollama_models,
                },
                engine.clone(),
            )?
            .with_status(dashboard.live.clone())
            .with_submissions(dashboard.submissions.clone()),
        )
    };
    // Scoped futures, not detached tasks: runtime restart drops model startup
    // and process monitoring before another runtime begins.
    let model_startup = async {
        if let Some(server) = &model_server {
            let mut snapshot = server.models()?;
            for model in &mut snapshot {
                model["model_metadata"]["lifecycle"] = serde_json::json!("loading");
            }
            dashboard.live.lock().unwrap()["models"] = serde_json::json!(snapshot);
            if let Err(error) = server.start(Duration::from_secs(300)).await {
                warn!(%error, "local models unavailable; node core remains controllable");
                dashboard.record_startup_issue(eefn::service::StartupIssue::LlamacppStartupFailed);
            }
            {
                let mut status = dashboard.live.lock().unwrap();
                // llama.cpp and Ollama are mutually exclusive providers here. Keep
                // this local snapshot inspectable even while EEF is unreachable.
                status["models"] = serde_json::json!(server.models()?);
                status["capabilities"] = serde_json::json!(engine.capabilities());
            }
            if let Err(error) = server.monitor_processes().await {
                warn!(%error, "local model process failed; explicit restart required");
                let mut status = dashboard.live.lock().unwrap();
                status["models"] = serde_json::json!(server.models()?);
                status["capabilities"] = serde_json::json!(engine.capabilities());
            }
        }
        std::future::pending::<Result<()>>().await
    };
    let run = async {
        if let Some(message) = &args.ask {
            let result = client
                .as_ref()
                .context("--ask requires at least one configured EEF coordinator")?
                .submit_message(message, Duration::from_secs(120))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }
        let update_task =
            start_update_monitor(file.update.as_ref(), install_dir.clone(), dashboard.clone())?
                .map(AbortTask);
        if let Some(client) = &client {
            tokio::select! {
                result = client.run() => result?,
                _ = tokio::signal::ctrl_c() => info!("shutdown requested"),
            }
        } else {
            tokio::signal::ctrl_c().await?;
            info!("shutdown requested");
        }
        drop(update_task);
        Ok::<(), anyhow::Error>(())
    };
    let result = tokio::select! {
        result = model_startup => result,
        result = run => result,
    };
    if let Some(server) = &model_server {
        server.stop().await;
    }
    result
}

fn start_update_monitor(
    config: Option<&UpdateConfig>,
    install_dir: PathBuf,
    dashboard: Arc<eefn::NodeService>,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let Some(url) = config
        .manifest_url
        .clone()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let policy = config.policy.trim().to_lowercase();
    if policy == "off" || policy == "disabled" {
        return Ok(None);
    }
    if !matches!(policy.as_str(), "prompt" | "notify" | "auto" | "automatic") {
        bail!(
            "unknown update policy '{}'; use off, prompt, or auto",
            config.policy
        )
    }
    let interval = Duration::from_secs(config.check_interval_seconds.max(300));
    Ok(Some(tokio::spawn(async move {
        loop {
            match eefn::updater::check(&url, eefn::VERSION, Duration::from_secs(30)).await {
                Ok(check) if check.update_available => {
                    dashboard.live.lock().unwrap()["update"] = serde_json::json!(check);
                    info!(current = %check.current_version, latest = %check.latest_version, %policy, "EEF node update available");
                    if matches!(policy.as_str(), "auto" | "automatic") {
                        match eefn::updater::apply_for(
                            &url,
                            &install_dir,
                            Duration::from_secs(180),
                            "eefn",
                        )
                        .await
                        {
                            Ok(applied) => {
                                dashboard.live.lock().unwrap()["pending_restart"] =
                                    serde_json::json!(true);
                                warn!(version = %applied.new_version, "EEF node update installed; restart required");
                                return;
                            }
                            Err(error) => warn!(%error, "EEF node automatic update failed"),
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => warn!(%error, "EEF node update check failed"),
            }
            tokio::time::sleep(interval).await;
        }
    })))
}

fn load_file(path: &Path) -> Result<FileConfig> {
    if !path.is_file() {
        return Ok(FileConfig::default());
    }
    serde_json::from_slice(&std::fs::read(path)?)
        .with_context(|| format!("parse {}", path.display()))
}

fn merge_args(file: &mut FileConfig, args: &Args) {
    if !args.endpoints.is_empty() {
        file.endpoints = args
            .endpoints
            .iter()
            .map(|endpoint| CoordinatorEndpoint::new(endpoint, 0))
            .collect()
    }
    if let Some(value) = &args.node_id {
        file.node_id = value.clone()
    }
    if let Some(value) = &args.name {
        file.name = value.clone()
    }
    if let Some(value) = args.psk.as_ref().filter(|value| !value.is_empty()) {
        file.psk = value.clone()
    }
    if args.allow_write {
        file.allow_write = true
    }
    if !args.allowed_roots.is_empty() {
        file.allowed_roots = args.allowed_roots.clone()
    }
    if let Some(value) = &args.python {
        file.python = Some(value.clone())
    }
    if !args.python_plugins.is_empty() {
        file.python_plugins.extend(args.python_plugins.clone())
    }
}

fn validate_secret(psk: &str) -> Result<()> {
    if psk.len() < 12 || psk.eq_ignore_ascii_case("change-me-eef") || psk.contains("CHANGE_ME") {
        bail!("refusing placeholder/short PSK; configure a secret of at least 12 characters")
    }
    Ok(())
}

fn default_node_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "host".into());
    format!("node-{}", host.to_lowercase())
}

fn resolve_relative(config: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        return value.to_path_buf();
    }
    config
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(value)
}

fn resolve_binary(config: &Path, install_dir: &Path, value: &Path) -> PathBuf {
    let relative = resolve_relative(config, value);
    if value.is_absolute() || relative.is_file() {
        relative
    } else {
        install_dir.join(value)
    }
}

fn resolve_builtin_plugin(install_dir: &Path, filename: &str) -> Result<PathBuf> {
    let candidates = [
        install_dir.join("python/plugins").join(filename),
        std::env::current_dir()?
            .join("python/plugins")
            .join(filename),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| format!("enabled built-in plugin '{filename}' is missing"))
}

fn parse_selected_model(value: &str) -> Result<SelectedModel> {
    let (model_id, modality) = value
        .rsplit_once('=')
        .context("--select-model must use MODEL_ID=TEXT or MODEL_ID=VLM")?;
    let modality = modality.to_ascii_lowercase();
    if model_id.trim().is_empty() || !matches!(modality.as_str(), "text" | "vlm") {
        bail!("--select-model must use MODEL_ID=TEXT or MODEL_ID=VLM")
    }
    Ok(SelectedModel {
        model_id: model_id.trim().into(),
        modality,
        hints: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_commands_require_complete_coordinator_metadata() {
        Args::try_parse_from([
            "eefn",
            "network",
            "set",
            "--advertise-address",
            "26.1.2.3",
            "--json",
        ])
        .unwrap();
        Args::try_parse_from(["eefn", "--no-ui"]).unwrap();
        assert!(
            Args::try_parse_from(["eefn", "network", "set", "--coordinator-id", "eef-a"]).is_err()
        );
        assert!(
            Args::try_parse_from([
                "eefn",
                "network",
                "set",
                "--advertise-address",
                "26.1.2.3",
                "--clear-address"
            ])
            .is_err()
        );
    }

    #[test]
    fn explicit_model_selection_parses_without_model_assumptions() {
        let selected = parse_selected_model("owner/model:tag=VLM").unwrap();
        assert_eq!(selected.model_id, "owner/model:tag");
        assert_eq!(selected.modality, "vlm");
        assert!(parse_selected_model("unnamed").is_err());
    }
}
