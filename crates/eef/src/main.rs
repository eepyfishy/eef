use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "eef", version, about = "EEF native coordinator")]
struct Args {
    #[arg(long, default_value = "config/default_identity.yaml")]
    config: PathBuf,
    #[arg(long, default_value = "data/eef_memory.db")]
    database: PathBuf,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    no_brain: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "eef=info,eefn=info,tower_http=info".into()),
        )
        .init();
    let executable = std::env::current_exe()?;
    let install_root = eefn::updater::installation_root(&executable)?;
    if eefn::updater::handoff_if_selected(&install_root, eef::VERSION, "eef")? {
        return Ok(());
    }
    let args = Args::parse();
    if let Some(parent) = args
        .database
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let config = eef::config::Config::load(Some(&args.config))?;
    let host = args
        .host
        .unwrap_or_else(|| config.string("web.host", "127.0.0.1"));
    let port = args
        .port
        .unwrap_or_else(|| u16::try_from(config.u64("web.port", 51334)).unwrap_or(51334));
    let runtime = eef::Runtime::initialize(config, &args.database, !args.no_brain).await?;
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .with_context(|| format!("bind EEF API {host}:{port}"))?;
    info!(%host, %port, node_port = runtime.node_server.port(), "EEF API listening");
    let result = axum::serve(listener, eef::api::router(runtime.clone()))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    runtime.shutdown().await;
    result.context("EEF API server failed")
}
