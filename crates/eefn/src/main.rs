use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use eefn::{
    CoordinatorEndpoint, ModelServer, ModelSlot, NodeClient, NodeClientConfig, NodeDashboard,
    NodeEngine, PythonRuntime, SelectedModel,
};
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Parser)]
#[command(name = "eefn", version, about = "Native EEF node")]
struct Args {
    #[arg(long, default_value = "config.json")]
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

#[derive(Clone, Debug, Default, Deserialize)]
struct FileConfig {
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
    #[serde(default = "dashboard_host")]
    host: String,
    #[serde(default = "dashboard_port")]
    port: u16,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
    let args = Args::parse();
    let mut file = load_file(&args.config)?;
    merge_args(&mut file, &args);

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

    let dashboard_config = file.dashboard.clone().unwrap_or_default();
    let dashboard = NodeDashboard::new(args.config.clone(), file.node_id.clone());
    let dashboard_task = if dashboard_config.enabled && args.ask.is_none() {
        Some(
            dashboard
                .start(&dashboard_config.host, dashboard_config.port)
                .await?,
        )
    } else {
        None
    };

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
    if let Some(server) = &model_server {
        server.start(Duration::from_secs(300)).await?;
    }

    let active_ollama_models = if use_ollama {
        ollama_models
    } else {
        Vec::new()
    };
    let mut engine = NodeEngine::new(
        file.allow_write,
        file.allowed_roots.clone(),
        install_dir.clone(),
    )?
    .with_update_manifest(manifest)
    .with_ollama(
        ollama_url.clone(),
        active_ollama_models
            .iter()
            .map(|model| (model.model_id.clone(), model.modality.clone())),
    );
    if let Some(server) = &model_server {
        engine = engine.with_model_server(server.clone())
    }
    if !file.python_plugins.is_empty() {
        let runtime = Arc::new(
            PythonRuntime::discover(file.python.clone()).context("Python runtime unavailable")?,
        );
        info!(python = %runtime.executable().display(), "Python plugin runtime enabled");
        for plugin in &file.python_plugins {
            let path = resolve_relative(&args.config, plugin);
            if let Err(error) = engine
                .add_python_plugin(runtime.clone(), path.clone())
                .await
            {
                warn!(plugin = %path.display(), %error, "Python plugin not loaded");
            }
        }
    }
    let engine = Arc::new(engine);
    let client = if file.endpoints.is_empty() {
        None
    } else {
        Some(NodeClient::new(
            NodeClientConfig {
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
            engine,
        )?)
    };
    if let Some(message) = &args.ask {
        let result = client
            .as_ref()
            .context("--ask requires at least one configured EEF coordinator")?
            .submit_message(message, Duration::from_secs(120))
            .await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        if let Some(server) = &model_server {
            server.stop().await
        }
        if let Some(task) = dashboard_task {
            task.abort();
        }
        return Ok(());
    }
    let update_task = start_update_monitor(file.update.as_ref(), install_dir.clone())?;
    if let Some(client) = &client {
        tokio::select! {
            result = client.run() => result?,
            _ = tokio::signal::ctrl_c() => info!("shutdown requested"),
        }
    } else {
        tokio::signal::ctrl_c().await?;
        info!("shutdown requested");
    }
    if let Some(server) = &model_server {
        server.stop().await
    }
    if let Some(task) = update_task {
        task.abort();
    }
    if let Some(task) = dashboard_task {
        task.abort();
    }
    Ok(())
}

fn start_update_monitor(
    config: Option<&UpdateConfig>,
    install_dir: PathBuf,
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
                                warn!(version = %applied.new_version, "EEF node update installed; restart required")
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
    if let Some(value) = &args.psk {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_model_selection_parses_without_model_assumptions() {
        let selected = parse_selected_model("owner/model:tag=VLM").unwrap();
        assert_eq!(selected.model_id, "owner/model:tag");
        assert_eq!(selected.modality, "vlm");
        assert!(parse_selected_model("unnamed").is_err());
    }
}
