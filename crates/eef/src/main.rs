#![cfg_attr(windows, windows_subsystem = "windows")]
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "eef", version, about = "EEF native coordinator")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long)]
    open_dashboard: bool,
    #[arg(long, global = true, default_value = "config/default_identity.yaml")]
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

#[derive(Debug, Subcommand)]
enum Command {
    /// Owner-approved control of a connected node.
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Request coordinator restart. Use diagnostics to verify completion.
    Restart,
    /// Manage exact, directional peer-discovery grants as the local EEF owner.
    Discovery {
        #[command(subcommand)]
        command: eef::discovery::DiscoveryCommand,
    },
    /// Export minimal diagnostics or explicitly ping a connected node.
    Diagnostics {
        #[arg(long)]
        node: Option<String>,
        #[arg(long, default_value_t = 3)]
        samples: usize,
    },
    /// Create a PRIVATE pairing code using a reachable coordinator endpoint.
    Invite {
        #[arg(long)]
        address: String,
    },
}

#[derive(Debug, Subcommand)]
enum NodeCommand {
    /// Restart the node app, not Windows. Requires local node management approval.
    Restart {
        #[arg(long)]
        node: String,
        /// Wait for a new runtime ID (0 acknowledges only; maximum 60).
        #[arg(long, default_value_t = 30)]
        wait_seconds: u64,
    },
}

async fn run_command(args: &Args, command: &Command) -> Result<serde_json::Value> {
    use serde_json::json;
    let config = eef::config::Config::load(Some(&args.config))?;
    let host: std::net::IpAddr = config
        .string("web.host", "127.0.0.1")
        .parse()
        .context("command API requires a loopback IP")?;
    if !host.is_loopback() {
        bail!("command API must use a loopback address")
    }
    let port = u16::try_from(config.u64("web.port", 51334)).context("invalid local API port")?;
    let base = format!("http://{}", std::net::SocketAddr::new(host, port));
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(25))
        .build()?;
    let response = match command {
        Command::Node {
            command: NodeCommand::Restart { node, wait_seconds },
        } => {
            eefn::network::validate_node_id(node)?;
            if *wait_seconds > 60 {
                bail!("wait_seconds must be 0-60")
            }
            client
                .post(format!("{base}/api/commands/node/restart"))
                .timeout(std::time::Duration::from_secs(wait_seconds + 15))
                .json(&json!({"node_id":node,"wait_seconds":wait_seconds}))
                .send()
                .await?
        }
        Command::Restart => {
            client
                .post(format!("{base}/api/restart"))
                .json(&json!({}))
                .send()
                .await?
        }
        Command::Discovery { command } => {
            let response = client
                .get(format!("{base}/api/commands/discovery"))
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("coordinator discovery commands unavailable")
            }
            let state = eefn::model_manager::bounded_json(response).await?;
            if matches!(command, eef::discovery::DiscoveryCommand::Show) {
                return Ok(state);
            }
            let runtime_id = state["runtime_id"]
                .as_str()
                .context("coordinator returned no runtime ID")?;
            client
                .post(format!("{base}/api/commands/discovery"))
                .json(
                    &json!({"schema_version":1,"expected_runtime_id":runtime_id,"command":command}),
                )
                .send()
                .await?
        }
        Command::Diagnostics {
            node: Some(node),
            samples,
        } => {
            if !(1..=10).contains(samples) {
                bail!("samples must be 1-10")
            }
            client
                .post(format!("{base}/api/diagnostics/probe"))
                .json(&json!({"node_id":node,"samples":samples}))
                .send()
                .await?
        }
        Command::Diagnostics { node: None, .. } => {
            client.get(format!("{base}/api/diagnostics")).send().await?
        }
        Command::Invite { address } => {
            eefn::network::validate_address(address, true)?;
            client
                .post(format!("{base}/api/network/invite"))
                .json(&json!({"address":address}))
                .send()
                .await?
        }
    };
    let ok = response.status().is_success();
    let value = eefn::model_manager::bounded_json(response).await?;
    if !ok {
        bail!(
            "{}",
            value["error"]
                .as_str()
                .unwrap_or("coordinator command failed")
        )
    }
    Ok(value)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
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
    if let Some(command) = &args.command {
        match run_command(&args, command).await {
            Ok(value) => {
                if matches!(command, Command::Invite { .. }) {
                    eprintln!(
                        "Private connection code: share only with the node owner, not in reports or issues."
                    );
                }
                println!(
                    "{}",
                    if args.json {
                        serde_json::to_string(&value)?
                    } else {
                        serde_json::to_string_pretty(&value)?
                    }
                );
                if value["success"].as_bool() == Some(false) {
                    bail!(
                        "command did not complete successfully; inspect the result before retrying"
                    )
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
