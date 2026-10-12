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
    /// Run command/API services without serving the browser UI.
    #[arg(long)]
    no_ui: bool,
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
    /// Enable/inspect/stop bounded automatic fixed-input work as the local owner.
    AutomationSchedules {
        #[command(subcommand)]
        command: eef::automation_schedule::ScheduleCommand,
    },
    /// Save/inspect/revoke bounded reusable approval as the local owner; never dispatch.
    AutomationPolicies {
        #[command(subcommand)]
        command: eefn::automation_policy::PolicyCommand,
    },
    /// Reserve bounded authority for an independently compiled job, without starting work.
    AutomationOccurrences {
        #[command(subcommand)]
        command: eefn::automation_occurrence::OccurrenceCommand,
    },
    /// Author/inspect/remove disabled automation definitions as the local owner; never run them.
    Automations {
        #[command(subcommand)]
        command: eefn::automation::AutomationCommand,
    },
    /// Inspect/compile/approve reviewed requests as the local owner, without dispatch.
    Requests {
        #[command(subcommand)]
        command: eef::request_planning::PlanningAction,
    },
    /// Inspect/save non-model scheduler limits as the local coordinator owner.
    Scheduling {
        #[command(subcommand)]
        command: eef::scheduling::SchedulingCommand,
    },
    /// Local-owner node credential provisioning and migration (not execution grants).
    Credentials {
        #[command(subcommand)]
        command: eef::credentials::CredentialCommand,
    },
    /// Inspect models advertised by connected nodes without loading or downloading.
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
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
    /// Read the installed update selector without downloading, applying or restarting.
    UpdateStatus {
        #[arg(long)]
        node: String,
    },
    /// Install one exact owner-approved release; never changes automatic-update policy.
    Update {
        #[arg(long)]
        node: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        size_bytes: u64,
        /// Explicit consent for this alpha/beta/RC artifact only.
        #[arg(long)]
        allow_prerelease: bool,
    },
    /// Inspect or edit a connected node's model selections with its owner's approval.
    Models {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::model_cli::ModelsAction,
    },
    /// Control owned selected GGUF residency with current node owner approval.
    ModelLifecycle {
        #[arg(long)]
        node: String,
        #[command(flatten)]
        command: eefn::model_lifecycle::LifecycleArgs,
    },
    /// Restart the node app, not Windows. Requires local node management approval.
    Restart {
        #[arg(long)]
        node: String,
        /// Wait for a new runtime ID (0 acknowledges only; maximum 60).
        #[arg(long, default_value_t = 30)]
        wait_seconds: u64,
    },
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    /// Preview current model candidates; does not authorize or execute inference.
    Route {
        #[arg(long,value_parser=["llm.infer","vlm.analyze"])]
        capability: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    List {
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        capability: Option<String>,
        #[arg(long)]
        after: Option<String>,
        /// Maximum nodes per page, not number of models (1-32).
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
}

#[cfg(test)]
mod credential_cli_tests {
    use super::*;
    #[test]
    fn remote_model_lifecycle_cli_preserves_exact_target_runtime_generation_and_receipt() {
        let args = Args::try_parse_from([
            "eef",
            "node",
            "model-lifecycle",
            "--node",
            "target",
            "--runtime",
            "11111111-1111-4111-8111-111111111111",
            "unload",
            "--model",
            "owner-selected",
            "--generation",
            "2",
            "--operation-id",
            "22222222-2222-4222-8222-222222222222",
        ])
        .unwrap();
        let Some(Command::Node {
            command: NodeCommand::ModelLifecycle { node, command },
        }) = args.command
        else {
            panic!("wrong adapter")
        };
        let req = eefn::model_lifecycle::LifecycleRequest {
            schema_version: 1,
            expected_node_id: node,
            expected_runtime_id: command.runtime,
            command: command.command,
        };
        assert!(req.validate().is_ok());
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "model-lifecycle",
                "--node",
                "target",
                "show",
                "--force"
            ])
            .is_err()
        );
    }
    #[test]
    fn refusal_resolution_cli_requires_proof_and_rejects_force_or_unknown_ack() {
        let args = [
            "eef",
            "automation-schedules",
            "resolve-refusal",
            "id",
            "--revision",
            "3",
            "--trigger-id",
            "trigger",
            "--refusal-sha256",
            "hash",
            "--operation-id",
            "receipt",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in (4..args.len()).step_by(2) {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in ["--force", "--retry", "--renew", "--acknowledge-unknown"] {
            let mut extra = args.to_vec();
            extra.push(flag);
            assert!(Args::try_parse_from(extra).is_err());
        }
    }
    #[test]
    fn automatic_schedule_cli_requires_exact_policy_receipt_and_stop_revision() {
        let args = [
            "eef",
            "automation-schedules",
            "enable",
            "--policy",
            "policy",
            "--policy-sha256",
            "hash",
            "--operation-id",
            "receipt",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in (3..args.len()).step_by(2) {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in ["--force", "--renew", "--retry", "--permissions"] {
            let mut extra = args.to_vec();
            extra.push(flag);
            assert!(Args::try_parse_from(extra).is_err());
        }
        assert!(
            Args::try_parse_from([
                "eef",
                "automation-schedules",
                "stop",
                "id",
                "--revision",
                "2",
                "--operation-id",
                "receipt"
            ])
            .is_ok()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "automation-schedules",
                "stop",
                "id",
                "--operation-id",
                "receipt"
            ])
            .is_err()
        );
    }
    #[test]
    fn archive_cli_needs_exact_terminal_revision_receipt_and_cannot_refund_or_execute() {
        let args = [
            "eef",
            "automation-occurrences",
            "archive",
            "--reservation",
            "receipt",
            "--reservation-sha256",
            "hash",
            "--job",
            "job",
            "--revision",
            "4",
            "--operation-id",
            "archive-receipt",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in (3..args.len()).step_by(2) {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in [
            "--execute",
            "--refund",
            "--replace",
            "--renew",
            "--retry",
            "--force",
        ] {
            let mut extra = args.to_vec();
            extra.push(flag);
            assert!(Args::try_parse_from(extra).is_err());
        }
        assert!(
            Args::try_parse_from(["eef", "automation-occurrences", "archive-show", "receipt"])
                .is_ok()
        );
    }
    #[test]
    fn reusable_approval_cli_requires_all_bounds_and_cannot_start_work() {
        let args = [
            "eef",
            "automation-policies",
            "approve",
            "--definition",
            "id",
            "--revision",
            "1",
            "--definition-sha256",
            "hash",
            "--generation",
            "0",
            "--operation-id",
            "receipt",
            "--valid-for-ms",
            "10000",
            "--max-runs",
            "3",
            "--max-execution-ms-per-task",
            "1000",
            "--max-result-bytes-per-task",
            "1024",
            "--max-reserved-execution-ms",
            "3000",
            "--max-reserved-result-bytes",
            "3072",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in (3..args.len()).step_by(2) {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in ["--execute", "--run", "--enable", "--retry", "--permissions"] {
            let mut extra = args.to_vec();
            extra.push(flag);
            assert!(Args::try_parse_from(extra).is_err());
        }
        assert!(
            Args::try_parse_from([
                "eef",
                "automation-policies",
                "revoke",
                "id",
                "--revision",
                "1",
                "--operation-id",
                "receipt"
            ])
            .is_ok()
        );
    }
    #[test]
    fn automation_authoring_requires_explicit_source_and_bounded_declarations() {
        let args = [
            "eef",
            "automations",
            "define",
            "--operation-id",
            "receipt",
            "--from-job",
            "source",
            "--revision",
            "2",
            "--plan-sha256",
            "digest",
            "--name",
            "Observation",
            "--every-ms",
            "1000",
            "--max-runs",
            "3",
            "--valid-for-ms",
            "10000",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in (3..args.len()).step_by(2) {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err(), "{}", args[index]);
        }
        for flag in [
            "--execute",
            "--enable",
            "--approve",
            "--retry",
            "--node",
            "--auto-approve",
        ] {
            let mut extra = args.to_vec();
            extra.push(flag);
            assert!(Args::try_parse_from(extra).is_err(), "{flag}");
        }
    }
    #[test]
    fn automation_inventory_and_removal_cannot_start_or_authorize_work() {
        assert!(Args::try_parse_from(["eef", "automations", "list", "--limit", "1"]).is_ok());
        assert!(Args::try_parse_from(["eef", "automations", "show", "receipt"]).is_ok());
        assert!(
            Args::try_parse_from([
                "eef",
                "automations",
                "remove",
                "receipt",
                "--revision",
                "1",
                "--operation-id",
                "removal"
            ])
            .is_ok()
        );
        assert!(
            Args::try_parse_from(["eef", "automations", "remove", "receipt", "--revision", "1"])
                .is_err()
        );
        for action in ["run", "enable", "approve", "resume", "renew"] {
            assert!(
                Args::try_parse_from(["eef", "automations", action, "receipt"]).is_err(),
                "{action}"
            );
        }
    }
    #[test]
    fn recovery_requires_saved_revision_grant_and_operation_without_retry_or_evidence_flags() {
        let args = [
            "eef",
            "requests",
            "recover-result",
            "job-id",
            "--revision",
            "8",
            "--grant-id",
            "grant",
            "--operation-id",
            "receipt",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in [4, 6, 8] {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in [
            "--retry",
            "--renew",
            "--execute",
            "--report",
            "--resume",
            "--auto-approve",
        ] {
            let mut bad = args.to_vec();
            bad.push(flag);
            assert!(Args::try_parse_from(bad).is_err());
        }
    }
    #[test]
    fn planning_requires_an_explicit_revision_and_input_file() {
        assert!(Args::try_parse_from(["eef", "requests", "inspect", "draft-id"]).is_ok());
        assert!(Args::try_parse_from(["eef", "requests", "compile", "draft-id"]).is_err());
        assert!(
            Args::try_parse_from(["eef", "requests", "compile", "draft-id", "--revision", "1"])
                .is_err()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "requests",
                "compile",
                "draft-id",
                "--revision",
                "1",
                "--file",
                "input.json"
            ])
            .is_ok()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "requests",
                "compile",
                "draft-id",
                "--revision",
                "1",
                "--file",
                "input.json",
                "--approve"
            ])
            .is_err()
        );
    }
    #[test]
    fn scheduling_commands_require_explicit_node_limits_and_preserve_zero() {
        assert!(Args::try_parse_from(["eef", "scheduling", "show"]).is_ok());
        assert!(Args::try_parse_from(["eef", "scheduling", "node", "--node", "a"]).is_err());
        assert!(Args::try_parse_from(["eef", "scheduling", "remove-node"]).is_err());
        let args =
            Args::try_parse_from(["eef", "scheduling", "limits", "--max-in-flight", "0"]).unwrap();
        assert!(matches!(
            args.command,
            Some(Command::Scheduling {
                command: eef::scheduling::SchedulingCommand::Limits {
                    max_in_flight: Some(0),
                    ..
                }
            })
        ));
    }
    #[test]
    fn occurrence_reservation_requires_exact_receipt_scope_and_cannot_enable_execution() {
        let args = [
            "eef",
            "automation-occurrences",
            "reserve",
            "--policy",
            "policy",
            "--policy-sha256",
            "hash",
            "--job",
            "job",
            "--revision",
            "2",
            "--plan-sha256",
            "digest",
            "--operation-id",
            "receipt",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for index in [3, 5, 7, 9, 11, 13] {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in [
            "--execute",
            "--retry",
            "--refund",
            "--renew",
            "--enable",
            "--permissions",
        ] {
            let mut bad = args.to_vec();
            bad.push(flag);
            assert!(Args::try_parse_from(bad).is_err());
        }
        assert!(Args::try_parse_from(["eef", "automation-occurrences", "list"]).is_ok());
        assert!(Args::try_parse_from(["eef", "automation-occurrences", "show", "receipt"]).is_ok());
    }
    #[test]
    fn approval_requires_digest_revision_and_all_bounds_and_has_no_execute_flag() {
        let args = [
            "eef",
            "requests",
            "approve",
            "draft-id",
            "--revision",
            "2",
            "--digest",
            "hash",
            "--valid-for-ms",
            "60000",
            "--max-execution-ms",
            "5000",
            "--max-result-bytes",
            "65536",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for i in [4, 6, 8, 10, 12] {
            let mut missing = args.to_vec();
            missing.drain(i..i + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
        for flag in ["--execute", "--renew", "--retry", "--auto-approve"] {
            let mut bad = args.to_vec();
            bad.push(flag);
            assert!(Args::try_parse_from(bad).is_err());
        }
        let mut explicit = args.to_vec();
        explicit.extend(["--operation-id", "receipt"]);
        assert!(Args::try_parse_from(explicit).is_ok());
    }
    #[test]
    fn legacy_policy_requires_an_explicit_value_and_rotation_is_opt_in() {
        assert!(Args::try_parse_from(["eef", "credentials", "legacy"]).is_err());
        assert!(Args::try_parse_from(["eef", "credentials", "legacy", "--allow", "false"]).is_ok());
        let args = Args::try_parse_from([
            "eef",
            "credentials",
            "provision",
            "--node",
            "a",
            "--address",
            "127.0.0.1:51335",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            Some(Command::Credentials {
                command: eef::credentials::CredentialCommand::Provision { rotate: false, .. }
            })
        ));
    }
}

async fn owner_automation_exchange(
    client: &reqwest::Client,
    base: &str,
    command: serde_json::Value,
    mutation: bool,
    path: &str,
    report_type: &str,
    validate: impl Fn(&serde_json::Value, &str) -> bool,
) -> serde_json::Value {
    use serde_json::json;
    let mut sending_attempted = false;
    let mut runtime_id: Option<String> = None;
    let exchange: std::result::Result<serde_json::Value, &str> = async {
        let response = client
            .get(format!("{base}/api/diagnostics"))
            .send()
            .await
            .map_err(|_| "coordinator_unavailable")?;
        if !response.status().is_success() {
            return Err("coordinator_unavailable");
        }
        let diagnostics = eefn::model_manager::bounded_json(response)
            .await
            .map_err(|_| "coordinator_unconfirmed")?;
        let runtime = diagnostics["runtime_id"]
            .as_str()
            .ok_or("coordinator_unconfirmed")?;
        if diagnostics["schema_version"] != 1
            || diagnostics["success"] != true
            || diagnostics["report_type"] != "coordinator_diagnostics"
            || eefn::job_commands::validate_operation_id(runtime).is_err()
        {
            return Err("coordinator_unconfirmed");
        }
        runtime_id = Some(runtime.to_owned());
        sending_attempted = true;
        let response = client
            .post(format!("{base}/api/commands/{path}"))
            .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command}))
            .send()
            .await
            .map_err(|_| "automation_reply_unconfirmed")?;
        if !response.status().is_success() {
            return Err("automation_rejected");
        }
        let report = eefn::model_manager::bounded_json(response)
            .await
            .map_err(|_| "automation_reply_unconfirmed")?;
        if !validate(&report, runtime) {
            return Err("automation_reply_unconfirmed");
        }
        Ok(report)
    }
    .await;
    let (result, error_code) = match exchange {
        Ok(report) => (Some(report), None),
        Err(code) => (None, Some(code)),
    };
    let success = result.is_some();
    json!({"schema_version":1,"report_type":report_type,"actor":"coordinator_owner","command":command,"runtime_id":runtime_id,
        "success":success,"coordinator_acknowledged":success,"sending_attempted":sending_attempted,
        "outcome_unknown":!success && sending_attempted && mutation,"error_code":error_code,"result":result,
        "timers_started":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false,
        "note":"Private automation checkpoints only, never timers or execution. Inspect the exact receipt after uncertainty. No new receipt, fallback or automatic retry."})
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
        Command::AutomationSchedules { command } => {
            command.validate()?;
            let mut value = owner_automation_exchange(
                &client,
                &base,
                json!(command),
                command.receipt().is_some(),
                "automation-schedules",
                "owner_automation_schedule_command",
                |v, runtime| eef::automation_schedule::confirmed(v, command, runtime),
            )
            .await;
            // Enable may authorize later automatic work even if its reply was
            // lost. Do not carry the record-only adapter's no-timer claims.
            for field in ["timers_started", "execution_authorized", "dispatched"] {
                value.as_object_mut().unwrap().remove(field);
            }
            value["timer_active"] = value["result"]["timer_active"].clone();
            value["note"] = json!(
                "Private bounded automatic scheduling. Enable permits future fixed-scope work; uncertain replies require exact receipt inspection, never replacement or automatic retry. Stop is not physical-stop proof."
            );
            return Ok(value);
        }
        Command::AutomationOccurrences { command } => {
            command.validate()?;
            return Ok(owner_automation_exchange(
                &client,
                &base,
                json!(command),
                command.receipt().is_some(),
                "automation-occurrences",
                "owner_automation_occurrence_command",
                |v, runtime| eefn::automation_occurrence::confirmed(v, command, runtime),
            )
            .await);
        }
        Command::Automations { command } => {
            command.validate()?;
            return Ok(owner_automation_exchange(
                &client,
                &base,
                json!(command),
                command.receipt().is_some(),
                "automations",
                "owner_automation_command",
                |value, _| eefn::automation::confirmed(value, command, None),
            )
            .await);
        }
        Command::AutomationPolicies { command } => {
            command.validate()?;
            return Ok(owner_automation_exchange(
                &client,
                &base,
                json!(command),
                command.receipt().is_some(),
                "automation-policies",
                "owner_automation_policy_command",
                |value, runtime| eefn::automation_policy::confirmed(value, command, runtime),
            )
            .await);
        }
        Command::Requests { command } => {
            if let eef::request_planning::PlanningAction::Abandon {
                id,
                revision,
                grant_id,
                acknowledge_effects_may_continue,
                operation_id,
            } = command
            {
                let command = eefn::request_abandon::AbandonCommand {
                    id: id.clone(),
                    expected_revision: *revision,
                    grant_id: grant_id.clone(),
                    acknowledge_effects_may_continue: *acknowledge_effects_may_continue,
                    operation_id: operation_id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                };
                command.validate()?;
                let state =
                    command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                        .await?;
                let runtime = state["runtime_id"].as_str().context("Missing runtime ID")?;
                let exchange = async {
                    let value = command_response(client.post(format!("{base}/api/commands/requests/abandon"))
                        .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command})).send().await?).await?;
                    if !eefn::request_abandon::confirmed(&value, &command, eefn::request_abandon::AbandonActor::CoordinatorOwner, None, Some(runtime)) { bail!("Unconfirmed abandonment receipt") }
                    Ok::<_,anyhow::Error>(value)
                }.await;
                return Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"request_abandonment","success":false,
                    "id":command.id,"operation_id":command.operation_id,"outcome_unknown":true,"execution_requested":false,
                    "automatically_retried":false,"remote_stop_confirmed":false,"remote_effects_unknown":true,
                    "error_code":"abandonment_unconfirmed","note":"Inspect the request and reuse only the exact receipt; no fallback, stop or retry was authorized."})));
            }
            if let eef::request_planning::PlanningAction::RecoverResult {
                id,
                revision,
                grant_id,
                operation_id,
            } = command
            {
                let command = eefn::request_recovery::RecoverCommand {
                    id: id.clone(),
                    expected_revision: *revision,
                    grant_id: grant_id.clone(),
                    operation_id: operation_id.clone(),
                };
                command.validate()?;
                let state =
                    command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                        .await?;
                let runtime = state["runtime_id"].as_str().context("Missing runtime ID")?;
                let exchange = async {
                    let value = command_response(client.post(format!("{base}/api/commands/requests/recover-result"))
                        .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command})).send().await?).await?;
                    if !eefn::request_recovery::confirmed(&value, &command, eefn::request_recovery::RecoveryActor::CoordinatorOwner, None, Some(runtime)) {
                        bail!("Unconfirmed recovery receipt")
                    }
                    Ok::<_,anyhow::Error>(value)
                }.await;
                return Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"request_recovery","success":false,
                    "command":command,"outcome_unknown":true,"execution_requested":false,"automatically_retried":false,
                    "remote_stop_confirmed":false,"error_code":"recovery_unconfirmed",
                    "note":"Inspect the request and reuse only this exact receipt. No replay, continuation, renewal or fallback."})));
            }
            if let eef::request_planning::PlanningAction::Receipt { id, grant_id }
            | eef::request_planning::PlanningAction::Result { id, grant_id } = command
            {
                let result_query = matches!(
                    command,
                    eef::request_planning::PlanningAction::Result { .. }
                );
                let command = eefn::request_receipt::ReceiptCommand {
                    id: id.clone(),
                    grant_id: grant_id.clone(),
                };
                command.validate()?;
                let state =
                    command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                        .await?;
                let runtime = state["runtime_id"].as_str().context("Missing runtime ID")?;
                let path = if result_query { "result" } else { "receipt" };
                let result = command_response(client.post(format!("{base}/api/commands/requests/{path}"))
                    .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command})).send().await?).await?;
                if !(if result_query {
                    eefn::request_result::confirmed(&result, &command, None, Some(runtime))
                } else {
                    eefn::request_receipt::confirmed(&result, &command, None, Some(runtime))
                }) {
                    bail!("Unconfirmed receipt inspection; no fallback or recovery")
                }
                return Ok(result);
            }
            if let eef::request_planning::PlanningAction::Execute {
                id,
                revision,
                approval_digest,
                operation_id,
            } = command
            {
                let command = eefn::request_execution::ExecuteCommand {
                    id: id.clone(),
                    expected_revision: *revision,
                    approval_sha256: approval_digest.clone(),
                    operation_id: operation_id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                };
                command.validate()?;
                let state =
                    command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                        .await?;
                let runtime = state["runtime_id"].as_str().context("Missing runtime ID")?;
                let exchange=async {
                    let result=command_response(client.post(format!("{base}/api/commands/requests/execute"))
                        .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command})).send().await?).await?;
                    if !eefn::request_execution::confirmed(&result,&command,None,Some(runtime)) {bail!("Unconfirmed execution receipt")}
                    Ok::<_,anyhow::Error>(result)
                }.await;
                return Ok(exchange.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"request_execution","success":false,
                    "id":command.id,"operation_id":command.operation_id,"outcome_unknown":true,"automatically_retried":false,
                    "error_code":"execution_unconfirmed","note":"Inspect the request/job and receipts before retrying; no fallback, replay or automatic renewal."})));
            }
            let command = match command {
                eef::request_planning::PlanningAction::RecoverResult { .. } => unreachable!(),
                eef::request_planning::PlanningAction::Abandon { .. } => unreachable!(),
                eef::request_planning::PlanningAction::Receipt { .. } => unreachable!(),
                eef::request_planning::PlanningAction::Result { .. } => unreachable!(),
                eef::request_planning::PlanningAction::Execute { .. } => unreachable!(),
                eef::request_planning::PlanningAction::Inspect { id } => {
                    eef::request_planning::PlanningCommand::Inspect { id: id.clone() }
                }
                eef::request_planning::PlanningAction::Compile { id, revision, file } => {
                    eef::request_planning::PlanningCommand::Compile {
                        id: id.clone(),
                        expected_revision: *revision,
                        operation_id: uuid::Uuid::new_v4().to_string(),
                        input: eef::request_planning::read_input(file)?,
                    }
                }
                eef::request_planning::PlanningAction::Approve {
                    id,
                    revision,
                    digest,
                    valid_for_ms,
                    max_execution_ms,
                    max_result_bytes,
                    operation_id,
                } => eef::request_planning::PlanningCommand::Approve {
                    id: id.clone(),
                    expected_revision: *revision,
                    operation_id: operation_id
                        .clone()
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    plan_sha256: digest.clone(),
                    valid_for_ms: *valid_for_ms,
                    limits: eefn::grants::GrantLimits {
                        max_execution_ms: *max_execution_ms,
                        max_result_bytes: *max_result_bytes,
                    },
                },
            };
            command.validate()?;
            let state =
                command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                    .await?;
            let runtime_id = state["runtime_id"]
                .as_str()
                .context("Missing coordinator runtime ID")?;
            let operation_id = match &command {
                eef::request_planning::PlanningCommand::Inspect { .. } => None,
                eef::request_planning::PlanningCommand::Compile { operation_id, .. }
                | eef::request_planning::PlanningCommand::Approve { operation_id, .. } => {
                    Some(operation_id.clone())
                }
            };
            let exchange=async {
                let value=command_response(client.post(format!("{base}/api/commands/requests/planning"))
                    .json(&json!({"schema_version":1,"expected_runtime_id":runtime_id,"command":command})).send().await?).await?;
                if !eef::request_planning::confirmed_report(&value,runtime_id,&command) {bail!("Unconfirmed planning report")}
                Ok::<_,anyhow::Error>(value)
            }.await;
            return Ok(match exchange {
                Ok(v) => v,
                Err(e) => {
                    json!({"schema_version":1,"report_type":"request_planning","success":false,
                "id":command.id(),"operation_id":operation_id,"execution_authorized":false,"dispatched":false,"automatically_retried":false,
                "outcome_unknown":operation_id.is_some(),"error_code":"planning_unconfirmed","error":e.to_string(),
                "note":"Use requests inspect with this ID before retrying; a plan/approval may have been retained. Keep the operation receipt. No fallback, replay or execution was requested."})
                }
            });
        }
        Command::Scheduling { command } => {
            let response = client
                .get(format!("{base}/api/commands/scheduling"))
                .send()
                .await?;
            if !response.status().is_success() {
                bail!(
                    "coordinator scheduling commands unavailable; no fallback or retry was attempted"
                )
            }
            let state = eefn::model_manager::bounded_json(response).await?;
            if !eef::scheduling::confirmed_report(
                &state,
                None,
                &eef::scheduling::SchedulingCommand::Show {},
            ) {
                bail!("coordinator returned an invalid scheduling report")
            }
            if matches!(command, eef::scheduling::SchedulingCommand::Show {}) {
                return Ok(state);
            }
            let runtime_id = state["runtime_id"]
                .as_str()
                .context("coordinator returned no runtime ID")?;
            let response = client
                .post(format!("{base}/api/commands/scheduling"))
                .json(
                    &json!({"schema_version":1,"expected_runtime_id":runtime_id,"command":command}),
                )
                .send()
                .await?;
            let result = command_response(response).await?;
            if !eef::scheduling::confirmed_report(&result, Some(runtime_id), command)
                || result["applied_policy"] != state["applied_policy"]
            {
                bail!(
                    "scheduling outcome unconfirmed; inspect scheduling show before retrying. No fallback or replay was attempted"
                )
            }
            return Ok(result);
        }
        Command::Credentials { command } => {
            let response = client
                .get(format!("{base}/api/commands/credentials"))
                .send()
                .await?;
            if !response.status().is_success() {
                bail!("coordinator credential commands unavailable")
            }
            let state = eefn::model_manager::bounded_json(response).await?;
            if matches!(command, eef::credentials::CredentialCommand::Show) {
                return Ok(state);
            }
            let runtime_id = state["runtime_id"]
                .as_str()
                .context("coordinator returned no runtime ID")?;
            client
                .post(format!("{base}/api/commands/credentials"))
                .json(
                    &json!({"schema_version":1,"expected_runtime_id":runtime_id,"command":command}),
                )
                .send()
                .await?
        }
        Command::Node {
            command: NodeCommand::UpdateStatus { node },
        } => {
            eefn::network::validate_node_id(node)?;
            client
                .post(format!("{base}/api/node/{node}/invoke"))
                .json(
                    &json!({"capability":"node.update","action":"status","params":{},"timeout":10}),
                )
                .send()
                .await?
        }
        Command::Node {
            command:
                NodeCommand::Update {
                    node,
                    version,
                    url,
                    sha256,
                    size_bytes,
                    allow_prerelease,
                },
        } => {
            let request = eefn::updater::ExplicitUpdate {
                schema_version: 1,
                expected_node_id: node.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                version: version.clone(),
                url: url.clone(),
                sha256: sha256.clone(),
                size_bytes: *size_bytes,
                allow_prerelease: *allow_prerelease,
            };
            request.validate()?;
            let mut report = json!({"schema_version":1,"report_type":"remote_explicit_update",
                "node_id":node,"request_id":request.request_id,"success":false,
                "outcome_unknown":true,"automatic_policy_changed":false,
                "note":"No automatic retry. Inspect node update status before retrying; restart is a separate command."});
            // Distinct action: older nodes reject it. Never fall back to feed changes or shell execution.
            let exchange = async {
                let response = client.post(format!("{base}/api/node/{node}/invoke"))
                    .timeout(std::time::Duration::from_secs(140))
                    .json(&json!({"capability":"node.update","action":"apply_explicit","params":request,"timeout":130}))
                    .send().await?;
                eefn::model_manager::bounded_json(response).await
            }.await;
            if let Ok(value) = exchange {
                if value["success"] == true
                    && value["data"]["report_type"] == "explicit_update"
                    && value["data"]["schema_version"] == 1
                    && value["data"]["success"] == true
                    && value["data"]["node_id"] == *node
                    && value["data"]["request_id"] == request.request_id
                    && value["data"]["applied"]["new_version"] == *version
                {
                    report["success"] = json!(true);
                    report["outcome_unknown"] = json!(false);
                    report["result"] = value["data"].clone();
                } else {
                    report["error_code"] = json!("explicit_update_unconfirmed");
                    report["error"] = json!(
                        "Node did not confirm the exact update. It may be unsupported, denied, failed or still running; inspect before retrying."
                    );
                }
            } else {
                report["error_code"] = json!("explicit_update_unconfirmed");
            }
            return Ok(report);
        }
        Command::Models {
            command:
                ModelsCommand::Route {
                    capability,
                    role,
                    node,
                    backend,
                    model,
                    limit,
                },
        } => {
            let mut query = vec![
                ("capability", capability.clone()),
                ("limit", limit.to_string()),
            ];
            for (key, value) in [
                ("role", role),
                ("node_id", node),
                ("backend", backend),
                ("model_id", model),
            ] {
                if let Some(value) = value {
                    query.push((key, value.clone()));
                }
            }
            client
                .get(format!("{base}/api/commands/models/route"))
                .query(&query)
                .send()
                .await?
        }
        Command::Models {
            command:
                ModelsCommand::List {
                    node,
                    capability,
                    after,
                    limit,
                },
        } => {
            let mut query = vec![("limit", limit.to_string())];
            for (key, value) in [
                ("node_id", node),
                ("capability", capability),
                ("after", after),
            ] {
                if let Some(value) = value {
                    query.push((key, value.clone()));
                }
            }
            client
                .get(format!("{base}/api/commands/models"))
                .query(&query)
                .send()
                .await?
        }
        Command::Node {
            command: NodeCommand::Models { node, command },
        } => {
            eefn::network::validate_node_id(node)?;
            client.post(format!("{base}/api/commands/node/models"))
                .json(&json!({"schema_version":1,"node_id":node,"command":command.to_command(false)?}))
                .send().await?
        }
        Command::Node {
            command: NodeCommand::ModelLifecycle { node, command },
        } => {
            let request = eefn::model_lifecycle::LifecycleRequest {
                schema_version: 1,
                expected_node_id: node.clone(),
                expected_runtime_id: command.runtime.clone(),
                command: command.command.clone(),
            };
            request.validate()?;
            client
                .post(format!("{base}/api/commands/node/model-lifecycle"))
                .json(&request)
                .send()
                .await?
        }
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
    command_response(response).await
}

async fn command_response(response: reqwest::Response) -> Result<serde_json::Value> {
    let status = response.status();
    let ok = status.is_success();
    if status == reqwest::StatusCode::NOT_FOUND {
        bail!(
            "coordinator returned HTTP 404; this command may require a newer coordinator. No fallback or retry was attempted; inspect node state before retrying"
        )
    }
    let value = eefn::model_manager::bounded_json(response)
        .await
        .with_context(|| {
            format!(
                "invalid coordinator response (HTTP {}); outcome unconfirmed. No fallback or retry was attempted; inspect node state before retrying",
                status.as_u16()
            )
        })?;
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

#[cfg(test)]
mod command_response_tests {
    use super::*;

    fn response(status: u16, body: &'static str) -> reqwest::Response {
        axum::http::Response::builder()
            .status(status)
            .body(body)
            .unwrap()
            .into()
    }

    #[tokio::test]
    async fn old_coordinator_and_invalid_replies_have_actionable_errors() {
        let old = command_response(response(404, ""))
            .await
            .unwrap_err()
            .to_string();
        assert!(old.contains("HTTP 404") && old.contains("newer coordinator"));
        assert!(old.contains("No fallback or retry"));
        for status in [200, 500] {
            let invalid = command_response(response(status, "not JSON"))
                .await
                .unwrap_err()
                .to_string();
            assert!(invalid.contains(&format!("HTTP {status}")));
            assert!(invalid.contains("outcome unconfirmed"));
        }
        let denied = command_response(response(403, r#"{"error":"approval required"}"#))
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(denied, "approval required");
        assert_eq!(
            command_response(response(200, r#"{"success":true}"#))
                .await
                .unwrap()["success"],
            true
        );
    }
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
                if matches!(command, Command::Invite { .. }) || value["private"] == true {
                    eprintln!(
                        "Private connection code: share only with the node owner, not in reports or issues."
                    );
                } else if matches!(
                    command,
                    Command::Requests { .. }
                        | Command::Automations { .. }
                        | Command::AutomationPolicies { .. }
                        | Command::AutomationOccurrences { .. }
                        | Command::AutomationSchedules { .. }
                ) && value["success"] == true
                {
                    eprintln!(
                        "Private request/plan report: may contain user text, task inputs and target bindings; do not publish it in reports or issues."
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
            if args.open_dashboard
                && !args.no_ui
                && config.bool("web.ui_enabled", true)
                && cfg!(feature = "dashboard")
            {
                eefn::setup::open_dashboard(
                    &config.string("web.host", "127.0.0.1"),
                    config.u64("web.port", 51334) as u16,
                );
            }
            return Ok(());
        }
    };
    let mut open_dashboard = args.open_dashboard && !args.no_ui && cfg!(feature = "dashboard");
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
        if open_dashboard && runtime.config.bool("web.ui_enabled", true) {
            eefn::setup::open_dashboard(&host, port);
            open_dashboard = false;
        }
        // Clear legacy local pairing advertisements while that transport is disabled;
        // independent node credentials are never published as a shared fallback.
        let local_secret = if runtime.node_server.authentication_policy().allow_legacy {
            std::env::var("EEF_NODE_PSK")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| runtime.config.string("node.psk", ""))
        } else {
            String::new()
        };
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
        let result = axum::serve(listener, eef::api::router_with_ui(runtime.clone(), !args.no_ui && runtime.config.bool("web.ui_enabled", true)))
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
