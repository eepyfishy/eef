#![cfg_attr(windows, windows_subsystem = "windows")]
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "eef", version, about = "EEF native coordinator")]
struct Args {
    #[arg(long)]
    open_dashboard: bool,
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
    let mut args = Args::parse();
    if args.config == PathBuf::from("config/default_identity.yaml")
        && install_root.join(&args.config).is_file()
    {
        args.config = install_root.join(&args.config);
    }
    if args.database == PathBuf::from("data/eef_memory.db") {
        args.database = args
            .config
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(&install_root)
            .join("data/eef_memory.db");
    }
    let _instance = match eefn::setup::instance_lock(&args.config)? {
        Some(lock) => lock,
        None => {
            let config = eef::config::Config::load(Some(&args.config))?;
            if args.open_dashboard {
                eefn::setup::open_dashboard(
                    &config.string("web.host", "127.0.0.1"),
                    config.u64("web.port", 51334) as u16,
                );
            }
            return Ok(());
        }
    };
    let mut open_dashboard = args.open_dashboard;
    if let Some(parent) = args
        .database
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut handoff = false;
    loop {
        let config = eef::config::Config::load(Some(&args.config))?;
        let host = args
            .host
            .clone()
            .unwrap_or_else(|| config.string("web.host", "127.0.0.1"));
        let port = args
            .port
            .unwrap_or_else(|| u16::try_from(config.u64("web.port", 51334)).unwrap_or(51334));
        let runtime = eef::Runtime::initialize(config, &args.database, !args.no_brain).await?;
        let listener = tokio::net::TcpListener::bind((host.as_str(), port))
            .await
            .with_context(|| format!("bind EEF API {host}:{port}"))?;
        info!(%host, %port, node_port = runtime.node_server.port(), "EEF API listening");
        if open_dashboard {
            eefn::setup::open_dashboard(&host, port);
            open_dashboard = false;
        }
        let local_secret = std::env::var("EEF_NODE_PSK")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| runtime.config.string("node.psk", ""));
        eefn::setup::publish_local(
            runtime.node_server.port(),
            port,
            &format!("EEF on {}", eefn::setup::hostname()),
            &local_secret,
        )?;
        let restart_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let requested = restart_requested.clone();
        let restart = runtime.restart.clone();
        let shutdown_runtime = runtime.clone();
        let result = axum::serve(listener, eef::api::router(runtime.clone()))
        .with_graceful_shutdown(async move {
            tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=restart.notified()=>{requested.store(true,std::sync::atomic::Ordering::Relaxed);} }
            // Stop runners before graceful HTTP draining; an in-flight job must
            // not keep a requested restart waiting for its entire timeout.
            shutdown_runtime.shutdown().await;
        })
        .await;
        runtime.shutdown().await;
        result.context("EEF API server failed")?;
        if !restart_requested.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if std::fs::read_to_string(install_root.join("current.txt"))
            .is_ok_and(|v| !v.trim().is_empty() && v.trim() != eef::VERSION)
        {
            handoff = true;
            break;
        }
    }
    drop(_instance);
    if handoff {
        eefn::updater::handoff_if_selected(&install_root, eef::VERSION, "eef")?;
    }
    Ok(())
}
