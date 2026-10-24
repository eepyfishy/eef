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
    /// Inspect this package; explicitly probe bundled runtimes without a service.
    Doctor {
        #[command(flatten)]
        options: eefn::package_health::Args,
    },
    /// Restore this exact application package; preserve settings and restart separately.
    Repair {
        #[command(flatten)]
        artifact: eefn::package_repair::RepairArgs,
    },
    /// Opt into bounded missing-bundled-file maintenance; never models or restarts.
    RepairSetup {
        #[command(subcommand)]
        command: eefn::package_setup::Action,
    },
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
    /// Explicitly test one selected interpreter or inspect historical test evidence.
    VerifyModel {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eef::model_verification::Action,
    },
    /// Manage a node's shared installation maintenance policy, not repair approval.
    RepairSetup {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::package_setup::policy::Action,
    },
    /// Explain a connected node's setup and next steps; never run setup or work.
    Setup {
        #[arg(long)]
        node: String,
    },
    /// Inspect/save bootstrap preferences with current local node-owner consent.
    ModelBootstrapPolicy {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::model_bootstrap::Action,
    },
    /// Inspect/save a node's Python budgets; writes need current local consent.
    Python {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::python_policy::Action,
    },
    /// Configure finite relay resources with current local node-owner approval.
    Relay {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::relay_setup::RelayAction,
    },
    /// Inspect or allow/deny whole node features with current local node-owner approval.
    Permissions {
        #[arg(long)]
        node: String,
        #[command(subcommand)]
        command: eefn::permission_commands::PermissionAction,
    },
    /// Read the installed update selector without downloading, applying or restarting.
    UpdateStatus {
        #[arg(long)]
        node: String,
        /// Inspect one retained repair operation without replay or recovery.
        #[arg(long)]
        operation_id: Option<String>,
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
        /// Restore this exact running build in a separate bundle, not a newer version.
        #[arg(long)]
        repair: bool,
        /// Stable identity for this repair only; an admitted ID is never replayed.
        #[arg(long, requires = "repair")]
        operation_id: Option<String>,
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
    /// Inspect/save the node's lifecycle policy; applying it requires explicit restart.
    ModelLifecyclePolicy {
        #[arg(long)]
        node: String,
        #[command(flatten)]
        command: eefn::model_lifecycle_policy::PolicyArgs,
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
    fn model_check_cli_requires_explicit_consent_and_separates_inspection() {
        assert!(
            Args::try_parse_from(["eef", "node", "verify-model", "--node", "fixture", "start"])
                .is_err()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "verify-model",
                "--node",
                "fixture",
                "start",
                "--allow-inference"
            ])
            .is_ok()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "verify-model",
                "--node",
                "fixture",
                "inspect"
            ])
            .is_err()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "verify-model",
                "--node",
                "fixture",
                "inspect",
                "--operation-id",
                "fixture"
            ])
            .is_ok()
        );
        for flag in [
            "--allow-inference",
            "--restart",
            "--prompt",
            "--model",
            "--approve",
        ] {
            assert!(
                Args::try_parse_from([
                    "eef",
                    "node",
                    "verify-model",
                    "--node",
                    "fixture",
                    "inspect",
                    "--operation-id",
                    "fixture",
                    flag
                ])
                .is_err()
            );
        }
    }
    #[test]
    fn node_setup_cli_requires_only_a_node_and_exposes_no_mutation_flags() {
        let args = Args::try_parse_from(["eef", "node", "setup", "--node", "fixture"]).unwrap();
        assert!(
            matches!(args.command,Some(Command::Node{command:NodeCommand::Setup{node}}) if node=="fixture")
        );
        assert!(Args::try_parse_from(["eef", "node", "setup"]).is_err());
        for flag in ["--approve", "--restart", "--install", "--enable-automation"] {
            assert!(
                Args::try_parse_from(["eef", "node", "setup", "--node", "fixture", flag]).is_err()
            );
        }
    }
    #[test]
    fn bootstrap_policy_cli_requires_node_and_exposes_no_download_or_restart() {
        Args::try_parse_from([
            "eef",
            "node",
            "model-bootstrap-policy",
            "--node",
            "owner",
            "configure",
            "--mode",
            "managed",
        ])
        .unwrap();
        assert!(Args::try_parse_from(["eef", "node", "model-bootstrap-policy", "show"]).is_err());
        for flag in [
            "--download",
            "--load",
            "--restart",
            "--execute",
            "--allow-remote",
        ] {
            assert!(
                Args::try_parse_from([
                    "eef",
                    "node",
                    "model-bootstrap-policy",
                    "--node",
                    "owner",
                    "show",
                    flag
                ])
                .is_err()
            );
        }
    }

    #[test]
    fn stop_inspection_cli_requires_exact_ids_and_has_no_mutation_options() {
        let base = vec![
            "eef",
            "requests",
            "stop-receipt",
            "job",
            "--grant-id",
            "grant",
        ];
        Args::try_parse_from(&base).unwrap();
        for flag in ["--execute", "--retry", "--restart", "--force", "--stop"] {
            let mut bad = base.clone();
            bad.push(flag);
            assert!(Args::try_parse_from(bad).is_err(), "{flag}");
        }
        assert!(Args::try_parse_from(["eef", "requests", "stop-receipt", "job"]).is_err());
    }
    #[test]
    fn remote_python_policy_cli_preserves_explicit_budgets_without_execution_flags() {
        let args = Args::try_parse_from([
            "eef",
            "node",
            "python",
            "--node",
            "node",
            "configure",
            "--memory-mib",
            "128",
        ])
        .unwrap();
        let Some(Command::Node {
            command:
                NodeCommand::Python {
                    command: eefn::python_policy::Action::Configure(settings),
                    ..
                },
        }) = args.command
        else {
            panic!("Wrong command")
        };
        assert_eq!(
            settings
                .merge(Default::default())
                .unwrap()
                .max_job_memory_mib,
            Some(128)
        );
        for extra in [
            vec!["--memory-mib", "63"],
            vec!["--restart"],
            vec!["--execute"],
            vec!["--allow-remote"],
            vec!["--retry"],
            vec!["--force"],
        ] {
            assert!(
                Args::try_parse_from(
                    ["eef", "node", "python", "--node", "node", "configure"]
                        .into_iter()
                        .chain(extra)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn relay_setup_cli_never_includes_workload_or_permission_authority() {
        let args = Args::try_parse_from([
            "eef",
            "node",
            "relay",
            "--node",
            "node",
            "configure",
            "--microphone",
            "--receive",
            "--address",
            "127.0.0.1",
        ])
        .unwrap();
        let Some(Command::Node {
            command:
                NodeCommand::Relay {
                    command: eefn::relay_setup::RelayAction::Configure(settings),
                    ..
                },
        }) = args.command
        else {
            panic!("wrong relay command")
        };
        assert!(settings.microphone && settings.receive);
        assert!(settings.port.is_none());
        settings.validate().unwrap();
        for flag in [
            "--execute",
            "--permissions",
            "--restart",
            "--force",
            "--retry",
        ] {
            assert!(
                Args::try_parse_from([
                    "eef",
                    "node",
                    "relay",
                    "--node",
                    "node",
                    "configure",
                    "--microphone",
                    flag
                ])
                .is_err()
            );
        }
    }
    #[test]
    fn remote_ollama_residency_cli_preserves_exact_policy_choices() {
        for extra in [
            vec!["--ollama-keep-alive-seconds", "-1"],
            vec!["--ollama-keep-alive-default"],
        ] {
            let digest = "a".repeat(64);
            let mut argv = vec![
                "eef",
                "node",
                "model-lifecycle-policy",
                "--node",
                "owner",
                "--runtime",
                "11111111-1111-4111-8111-111111111111",
                "set",
                "--expected-digest",
                &digest,
            ];
            argv.extend(extra);
            let args = Args::try_parse_from(argv).unwrap();
            let Some(Command::Node {
                command: NodeCommand::ModelLifecyclePolicy { node, command },
            }) = args.command
            else {
                panic!("wrong adapter")
            };
            eefn::model_lifecycle_policy::PolicyRequest {
                schema_version: 1,
                expected_node_id: node,
                expected_runtime_id: command.runtime,
                command: command.command,
            }
            .validate()
            .unwrap();
        }
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "model-lifecycle-policy",
                "--node",
                "owner",
                "set",
                "--expected-digest",
                &"a".repeat(64),
                "--ollama-keep-alive-seconds",
                "0",
                "--ollama-keep-alive-default"
            ])
            .is_err()
        );
    }
    #[test]
    fn remote_lifecycle_policy_cli_requires_target_and_exact_saved_digest() {
        let args = Args::try_parse_from([
            "eef",
            "node",
            "model-lifecycle-policy",
            "--node",
            "owner",
            "--runtime",
            "11111111-1111-4111-8111-111111111111",
            "set",
            "--expected-digest",
            &"a".repeat(64),
            "--startup",
            "manual",
            "--idle-unload-seconds",
            "2",
        ])
        .unwrap();
        let Some(Command::Node {
            command: NodeCommand::ModelLifecyclePolicy { node, command },
        }) = args.command
        else {
            panic!("wrong adapter")
        };
        let request = eefn::model_lifecycle_policy::PolicyRequest {
            schema_version: 1,
            expected_node_id: node,
            expected_runtime_id: command.runtime,
            command: command.command,
        };
        assert!(request.validate().is_ok());
        assert!(Args::try_parse_from(["eef", "node", "model-lifecycle-policy", "show"]).is_err());
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "model-lifecycle-policy",
                "--node",
                "owner",
                "set",
                "--force"
            ])
            .is_err()
        );
    }
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
    fn node_maintenance_cli_cannot_run_or_approve_remote_repairs() {
        assert!(
            Args::try_parse_from(["eef", "node", "repair-setup", "--node", "node", "show"]).is_ok()
        );
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "repair-setup",
                "--node",
                "node",
                "configure",
                "--mode",
                "prompt"
            ])
            .is_ok()
        );
        assert!(Args::try_parse_from(["eef", "node", "repair-setup", "show"]).is_err());
        for operation in ["run", "approve", "restart", "retry"] {
            assert!(
                Args::try_parse_from(["eef", "node", "repair-setup", "--node", "node", operation])
                    .is_err()
            );
        }
        assert!(
            Args::try_parse_from([
                "eef",
                "node",
                "repair-setup",
                "--node",
                "node",
                "configure",
                "--mode",
                "auto",
                "--allow-prerelease"
            ])
            .is_err()
        );
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
    #[test]
    fn credential_export_requires_exact_node_and_endpoint_without_rotation_flags() {
        let args = [
            "eef",
            "credentials",
            "export",
            "--node",
            "stable",
            "--address",
            "127.0.0.1:51335",
        ];
        assert!(Args::try_parse_from(args).is_ok());
        for flag in [
            "--rotate",
            "--allow",
            "--permissions",
            "--execute",
            "--restart",
        ] {
            let mut invalid = args.to_vec();
            invalid.push(flag);
            assert!(Args::try_parse_from(invalid).is_err());
        }
        for index in [3, 5] {
            let mut missing = args.to_vec();
            missing.drain(index..index + 2);
            assert!(Args::try_parse_from(missing).is_err());
        }
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

async fn remote_relay_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eefn::relay_setup::RelayAction,
) -> Result<serde_json::Value> {
    use eef::node_relay::Request;
    use eefn::relay_setup::{RelayAction, RelayCommand};
    use serde_json::json;
    eefn::network::validate_remote_node_id(node)?;
    if let RelayAction::Configure(settings) = action {
        settings.validate()?;
    }
    let mutation_requested = matches!(action, RelayAction::Configure(_));
    let mut sending_mutation = false;
    let exchange = async {
        let url = format!("{base}/api/commands/node/relay");
        let mut request = Request {
            schema_version: 1,
            node_id: node.into(),
            expected_runtime_id: None,
            command: RelayCommand::Show,
        };
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Relay inspection unavailable")
        }
        let inspected = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_relay::confirmed(&inspected, &request) {
            bail!("Unconfirmed relay inspection")
        }
        let RelayAction::Configure(settings) = action else {
            return Ok::<_, anyhow::Error>(inspected);
        };
        request.expected_runtime_id = Some(
            inspected["result"]["runtime_id"]
                .as_str()
                .context("Missing node runtime")?
                .into(),
        );
        request.command = RelayCommand::Configure {
            settings: settings.clone(),
            expected_digest: inspected["result"]["saved_digest"]
                .as_str()
                .context("Missing relay configuration digest")?
                .into(),
        };
        request.validate()?;
        sending_mutation = true;
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Relay save unconfirmed")
        }
        let saved = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_relay::confirmed(&saved, &request) {
            bail!("Unconfirmed relay save")
        }
        Ok(saved)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"remote_relay_setup","node_id":node,
        "success":false,"mutation_requested":mutation_requested,"sending_attempted":sending_mutation,"outcome_unknown":sending_mutation,"result":null,
        "execution_authorized":false,"restart_requested":false,"automatically_retried":false,
        "error_code":"relay_setup_unconfirmed","note":"Inspect this exact node with relay show. No automatic retry, permissions, restart or workload was sent."})))
}

async fn remote_setup_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
) -> Result<serde_json::Value> {
    use serde_json::json;
    let request = eef::node_setup::Request {
        schema_version: 1,
        node_id: node.into(),
        expected_runtime_id: None,
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    request.validate()?;
    let result = async {
        let response = client
            .post(format!("{base}/api/commands/node/setup"))
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("Node setup inspection unavailable");
        }
        let value = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_setup::confirmed(&value, &request) {
            bail!("Node setup reply unconfirmed");
        }
        Ok::<_, anyhow::Error>(value)
    }
    .await;
    Ok(result.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"remote_node_setup","node_id":node,"request_id":request.request_id,"success":false,"result":null,
        "changed":false,"model_inference_requested":false,"execution_authorized":false,"restart_requested":false,"automatically_retried":false,
        "error_code":"node_setup_unconfirmed","note":"Node setup observations are unavailable or unconfirmed. No client retry, legacy fallback, model setup, restart or workload was requested."})))
}

async fn model_verification_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eef::model_verification::Action,
) -> Result<serde_json::Value> {
    use eef::model_verification::{Action, Command, Request};
    use serde_json::json;
    let operation = match action {
        Action::Start { operation_id, .. } => operation_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        Action::Inspect { operation_id } => operation_id.clone(),
    };
    let source = eefn::model_verification::receipt_id(node, &operation, "source", None)?;
    if matches!(
        action,
        Action::Start {
            allow_inference: false,
            ..
        }
    ) {
        bail!("Explicit --allow-inference consent required")
    }
    let mut sending = false;
    let exchange = async {
        let response = client.get(format!("{base}/api/diagnostics")).send().await?;
        if !response.status().is_success() {
            bail!("Coordinator unavailable")
        }
        let diagnostic = eefn::model_manager::bounded_json(response).await?;
        if diagnostic["schema_version"] != 1
            || diagnostic["success"] != true
            || diagnostic["report_type"] != "coordinator_diagnostics"
        {
            bail!("Coordinator observation unconfirmed")
        }
        let command = match action {
            Action::Inspect { .. } => Command::Inspect,
            Action::Start { .. } => {
                let inspect = eef::node_setup::Request {
                    schema_version: 1,
                    node_id: node.into(),
                    expected_runtime_id: None,
                    request_id: uuid::Uuid::new_v4().to_string(),
                };
                let response = client
                    .post(format!("{base}/api/commands/node/setup"))
                    .json(&inspect)
                    .send()
                    .await?;
                if !response.status().is_success() {
                    bail!("Node setup unavailable")
                }
                let observed = eefn::model_manager::bounded_json(response).await?;
                Command::Start {
                    definition: eef::model_verification::selected_definition(
                        &observed, &inspect, &operation,
                    )?,
                    approve_inference: true,
                }
            }
        };
        let request = Request {
            schema_version: 1,
            expected_runtime_id: diagnostic["runtime_id"]
                .as_str()
                .context("Missing coordinator runtime")?
                .into(),
            node_id: node.into(),
            operation_id: operation.clone(),
            command,
        };
        request.validate()?;
        sending = matches!(action, Action::Start { .. });
        let response = client
            .post(format!("{base}/api/commands/node/model-verification"))
            .timeout(std::time::Duration::from_secs(45))
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("Model check unconfirmed")
        }
        let value = eefn::model_manager::bounded_json(response).await?;
        if !eef::model_verification::confirmed(&value, &request) {
            bail!("Model-check reply unconfirmed")
        }
        Ok::<_, anyhow::Error>(value)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"model_verification_client_unconfirmed",
        "node_id":node,"operation_id":operation,"source_operation_id":source,"success":false,
        "sending_attempted":sending,"outcome_unknown":sending,"automatically_retried":false,
        "note":"Inspect this exact operation ID. No client retry or automatic approval renewal was requested; an unconfirmed start may have issued work."})))
}

async fn remote_package_policy_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eefn::package_setup::policy::Action,
) -> Result<serde_json::Value> {
    use eef::node_package_policy::Request;
    use eefn::package_setup::policy::{Action, Command, Version};
    use serde_json::json;
    eefn::network::validate_remote_node_id(node)?;
    // Validate all owner-supplied options before contacting either service.
    action.command(Version {
        revision: 0,
        digest: "0".repeat(64),
    })?;
    let mutation = !matches!(action, Action::Show);
    let mut sending = false;
    let mut requested = serde_json::Value::Null;
    let exchange = async {
        let url = format!("{base}/api/commands/node/repair-setup-policy");
        let mut request = Request {
            schema_version: 1,
            node_id: node.into(),
            expected_runtime_id: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            command: Command::Show,
        };
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Package policy inspection unavailable")
        }
        let inspected = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_package_policy::confirmed(&inspected, &request) {
            bail!("Package policy inspection unconfirmed")
        }
        if !mutation {
            return Ok::<_, anyhow::Error>(inspected);
        }
        request.expected_runtime_id = Some(
            inspected["result"]["runtime_id"]
                .as_str()
                .context("Missing node runtime")?
                .into(),
        );
        request.request_id = uuid::Uuid::new_v4().to_string();
        request.command = action.command(serde_json::from_value(
            inspected["result"]["version"].clone(),
        )?)?;
        request.validate()?;
        requested = json!(request.command);
        sending = true;
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Package policy save unconfirmed")
        }
        let saved = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_package_policy::confirmed(&saved, &request) {
            bail!("Package policy save unconfirmed")
        }
        Ok(saved)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"remote_repair_setup_policy",
        "node_id":node,"command":requested,"success":false,"mutation_requested":mutation,
        "sending_attempted":sending,"outcome_unknown":sending,"result":null,"restart_requested":false,
        "automatically_retried":false,"error_code":"package_policy_unconfirmed",
        "note":"Inspect the exact node's repair-setup policy before any new save. An acknowledged policy may be evaluated by its supervisor; lost replies are not rollback. No client retry, alternate node, repair approval or legacy fallback was sent."})))
}

async fn remote_model_bootstrap_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eefn::model_bootstrap::Action,
) -> Result<serde_json::Value> {
    use eef::node_model_bootstrap::Request;
    use eefn::model_bootstrap::{Action, Command};
    use serde_json::json;
    eefn::network::validate_remote_node_id(node)?;
    action.validate()?;
    let mutation = !matches!(action, Action::Show);
    let mut sending_mutation = false;
    let mut requested = serde_json::Value::Null;
    let exchange = async {
        let url = format!("{base}/api/commands/node/model-bootstrap-policy");
        let mut request = Request {
            schema_version: 1,
            node_id: node.into(),
            expected_runtime_id: None,
            command: Command::Show,
        };
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Bootstrap preferences inspection unavailable")
        }
        let inspected = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_model_bootstrap::confirmed(&inspected, &request) {
            bail!("Unconfirmed Bootstrap preferences inspection")
        }
        if matches!(action, Action::Show) {
            return Ok::<_, anyhow::Error>(inspected);
        }
        request.expected_runtime_id = Some(
            inspected["result"]["runtime_id"]
                .as_str()
                .context("Missing node runtime")?
                .into(),
        );
        let previous = serde_json::from_value(inspected["result"]["saved"].clone())?;
        request.command = match action {
            Action::Show => unreachable!(),
            Action::Configure(settings) => settings.command(previous)?,
            Action::Approve => Command::Approve { previous },
            Action::Stop { attempt } => Command::Stop {
                previous,
                attempt_id: attempt.clone(),
            },
        };
        request.validate()?;
        requested = json!(request.command);
        sending_mutation = true;
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Bootstrap preferences save unconfirmed")
        }
        let saved = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_model_bootstrap::confirmed(&saved, &request) {
            bail!("Unconfirmed Bootstrap preferences save")
        }
        Ok(saved)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"remote_model_bootstrap","node_id":node,
        "success":false,"mutation_requested":mutation,"sending_attempted":sending_mutation,"outcome_unknown":sending_mutation,
        "command":requested,"result":null,"execution_authorized":false,"restart_requested":false,"automatically_retried":false,
        "error_code":"model_bootstrap_unconfirmed","note":"Inspect this exact node with model-bootstrap-policy show and its retained attempt. No client replay or fallback was sent; an explicitly enabled backend operation may already have started. Workload permissions are unchanged."})))
}

async fn remote_python_policy_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eefn::python_policy::Action,
) -> Result<serde_json::Value> {
    use eef::node_python_policy::Request;
    use eefn::python_policy::{Action, Command};
    use serde_json::json;
    eefn::network::validate_remote_node_id(node)?;
    if let Action::Configure(settings) = action {
        settings.merge(Default::default())?;
    }
    let mutation = matches!(action, Action::Configure(_));
    let mut sending_mutation = false;
    let mut requested = serde_json::Value::Null;
    let exchange = async {
        let url = format!("{base}/api/commands/node/python");
        let mut request = Request {
            schema_version: 1,
            node_id: node.into(),
            expected_runtime_id: None,
            command: Command::Show,
        };
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Python policy inspection unavailable")
        }
        let inspected = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_python_policy::confirmed(&inspected, &request) {
            bail!("Unconfirmed Python policy inspection")
        }
        let Action::Configure(settings) = action else {
            return Ok::<_, anyhow::Error>(inspected);
        };
        request.expected_runtime_id = Some(
            inspected["result"]["runtime_id"]
                .as_str()
                .context("Missing node runtime")?
                .into(),
        );
        request.command = Command::Configure {
            limits: settings.merge(serde_json::from_value(
                inspected["result"]["saved"].clone(),
            )?)?,
            expected_digest: inspected["result"]["saved_digest"]
                .as_str()
                .context("Missing Python policy digest")?
                .into(),
        };
        request.validate()?;
        requested = json!(request.command);
        sending_mutation = true;
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Python policy save unconfirmed")
        }
        let saved = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_python_policy::confirmed(&saved, &request) {
            bail!("Unconfirmed Python policy save")
        }
        Ok(saved)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_| json!({"schema_version":1,"report_type":"remote_python_policy","node_id":node,
        "success":false,"mutation_requested":mutation,"sending_attempted":sending_mutation,"outcome_unknown":sending_mutation,
        "command":requested,"result":null,"execution_authorized":false,"restart_requested":false,"automatically_retried":false,
        "error_code":"python_policy_unconfirmed","note":"Inspect this exact node with python show; no automatic retry, permission change, restart or workload was sent."})))
}

async fn remote_permissions_cli(
    client: &reqwest::Client,
    base: &str,
    node: &str,
    action: &eefn::permission_commands::PermissionAction,
) -> Result<serde_json::Value> {
    use eef::node_control::{PermissionCommand, PermissionRequest};
    use eefn::permission_commands::PermissionAction;
    use serde_json::json;
    eefn::network::validate_remote_node_id(node)?;
    let mut sending_mutation = false;
    let mutation_requested = !matches!(action, PermissionAction::Show {});
    let exchange = async {
        let url = format!("{base}/api/commands/node/permissions");
        let mut request = PermissionRequest {
            schema_version: 1,
            node_id: node.into(),
            command: PermissionCommand::Show {},
        };
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Permission inspection unavailable")
        }
        let inspected = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_control::confirmed_permissions(&inspected, &request) {
            bail!("Unconfirmed permission inspection")
        }
        let (feature, enabled) = match action {
            PermissionAction::Show {} => return Ok(inspected),
            PermissionAction::Allow { feature } => (*feature, true),
            PermissionAction::Deny { feature } => (*feature, false),
        };
        request.command = PermissionCommand::Set {
            feature,
            enabled,
            expected_runtime_id: inspected["result"]["runtime_id"]
                .as_str()
                .context("Missing node runtime")?
                .into(),
            expected_digest: inspected["result"]["saved_digest"]
                .as_str()
                .context("Missing permission digest")?
                .into(),
        };
        sending_mutation = true;
        let response = client.post(&url).json(&request).send().await?;
        if !response.status().is_success() {
            bail!("Permission save unconfirmed")
        }
        let saved = eefn::model_manager::bounded_json(response).await?;
        if !eef::node_control::confirmed_permissions(&saved, &request) {
            bail!("Unconfirmed permission save")
        }
        Ok::<_, anyhow::Error>(saved)
    }
    .await;
    Ok(exchange.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"remote_permission_command","success":false,"node_id":node,
        "mutation_requested":mutation_requested,"sending_attempted":sending_mutation,"outcome_unknown":sending_mutation,
        "error_code":"permission_reply_unconfirmed","result":null,"execution_authorized":false,"restart_requested":false,"automatically_retried":false,
        "note":"Inspect the exact node's permissions before deciding what to do. No generic save, retry, restart or workload was sent."})))
}

async fn run_command(args: &Args, command: &Command) -> Result<serde_json::Value> {
    use serde_json::json;
    if let Command::Doctor { options } = command {
        return eefn::package_health::execute(
            &eefn::package_health::Context::current("eef")?,
            options,
        )
        .await;
    }
    if let Command::RepairSetup { command } = command {
        return eefn::package_setup::execute(
            &eefn::package_setup::Context::current("eef")?,
            command,
        )
        .await;
    }
    if let Command::Repair { artifact } = command {
        return eefn::package_repair::execute_local(artifact, "eef").await;
    }
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
        Command::Repair { .. } | Command::RepairSetup { .. } | Command::Doctor { .. } => {
            unreachable!("local package repair returned above")
        }
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
            | eef::request_planning::PlanningAction::Result { id, grant_id }
            | eef::request_planning::PlanningAction::StopReceipt { id, grant_id } = command
            {
                let query = match command {
                    eef::request_planning::PlanningAction::Result { .. } => {
                        eefn::request_receipt::QueryKind::Result
                    }
                    eef::request_planning::PlanningAction::StopReceipt { .. } => {
                        eefn::request_receipt::QueryKind::Stop
                    }
                    _ => eefn::request_receipt::QueryKind::Receipt,
                };
                let command = eefn::request_receipt::ReceiptCommand {
                    id: id.clone(),
                    grant_id: grant_id.clone(),
                };
                command.validate()?;
                let state =
                    command_response(client.get(format!("{base}/api/diagnostics")).send().await?)
                        .await?;
                let runtime = state["runtime_id"].as_str().context("Missing runtime ID")?;
                let path = query.path();
                let result = command_response(client.post(format!("{base}/api/commands/requests/{path}"))
                    .json(&json!({"schema_version":1,"expected_runtime_id":runtime,"command":command})).send().await?).await?;
                if !query.confirmed(&result, &command, None, Some(runtime)) {
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
                eef::request_planning::PlanningAction::StopReceipt { .. } => unreachable!(),
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
            let response = client
                .post(format!("{base}/api/commands/credentials"))
                .json(
                    &json!({"schema_version":1,"expected_runtime_id":runtime_id,"command":command}),
                )
                .send()
                .await?;
            if let eef::credentials::CredentialCommand::Export { node, address } = command {
                if !response.status().is_success() {
                    bail!(
                        "private invitation export rejected; inspect credentials show. No provisioning, rotation or retry was attempted"
                    )
                }
                let result = eefn::model_manager::bounded_json(response).await
                    .map_err(|_| anyhow::anyhow!("private invitation export reply unconfirmed; no code was printed or automatically retried"))?;
                if !eef::credentials::confirmed_export(
                    &result,
                    runtime_id,
                    node,
                    address,
                    &state["saved_policy"],
                    &state["applied_policy"],
                ) {
                    bail!(
                        "private invitation export reply unconfirmed; inspect credentials show. No code was printed or automatically retried"
                    )
                }
                return Ok(result);
            }
            response
        }
        Command::Node {
            command: NodeCommand::UpdateStatus { node, operation_id },
        } => {
            eefn::network::validate_node_id(node)?;
            if let Some(id) = operation_id {
                eefn::package_repair::validate_operation_id(id)?;
            }
            client
                .post(format!("{base}/api/node/{node}/invoke"))
                .json(
                    &json!({"capability":"node.update","action":"status","params":{"operation_id":operation_id},"timeout":10}),
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
                    repair,
                    operation_id,
                },
        } => {
            let request = eefn::updater::ExplicitUpdate {
                schema_version: 1,
                expected_node_id: node.clone(),
                request_id: operation_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                version: version.clone(),
                url: url.clone(),
                sha256: sha256.clone(),
                size_bytes: *size_bytes,
                allow_prerelease: *allow_prerelease,
            };
            request.validate()?;
            if *repair {
                eefn::package_repair::RepairRequest::from(&request).validate()?;
            }
            let action = if *repair {
                "repair_explicit"
            } else {
                "apply_explicit"
            };
            let report_type = if *repair {
                "explicit_repair"
            } else {
                "explicit_update"
            };
            let mut report = json!({"schema_version":1,"report_type":if *repair {"remote_explicit_repair"} else {"remote_explicit_update"},
                "node_id":node,"request_id":request.request_id,"success":false,
                "outcome_unknown":true,"automatic_policy_changed":false,
                "note":"No automatic retry. Inspect node update status before retrying; restart is a separate command."});
            // Distinct action: older nodes reject it. Never fall back to feed changes or shell execution.
            let exchange = async {
                let response = client.post(format!("{base}/api/node/{node}/invoke"))
                    .timeout(std::time::Duration::from_secs(140))
                    .json(&json!({"capability":"node.update","action":action,"params":request,"timeout":130}))
                    .send().await?;
                eefn::model_manager::bounded_json(response).await
            }.await;
            if let Ok(value) = exchange {
                if value["success"] == true
                    && value["data"]["report_type"] == report_type
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
            command: NodeCommand::Setup { node },
        } => return remote_setup_cli(&client, &base, node).await,
        Command::Node {
            command: NodeCommand::VerifyModel { node, command },
        } => return model_verification_cli(&client, &base, node, command).await,
        Command::Node {
            command: NodeCommand::Models { node, command },
        } => {
            eefn::network::validate_node_id(node)?;
            client.post(format!("{base}/api/commands/node/models"))
                .timeout(std::time::Duration::from_secs(if matches!(command, eefn::model_cli::ModelsAction::SelectInstalledGguf {..}) {80} else {25}))
                .json(&json!({"schema_version":1,"node_id":node,"command":command.to_command(false)?}))
                .send().await?
        }
        Command::Node {
            command: NodeCommand::Python { node, command },
        } => return remote_python_policy_cli(&client, &base, node, command).await,
        Command::Node {
            command: NodeCommand::ModelBootstrapPolicy { node, command },
        } => return Box::pin(remote_model_bootstrap_cli(&client, &base, node, command)).await,
        Command::Node {
            command: NodeCommand::RepairSetup { node, command },
        } => return Box::pin(remote_package_policy_cli(&client, &base, node, command)).await,
        Command::Node {
            command: NodeCommand::Relay { node, command },
        } => {
            return remote_relay_cli(&client, &base, node, command).await;
        }
        Command::Node {
            command: NodeCommand::Permissions { node, command },
        } => {
            return remote_permissions_cli(&client, &base, node, command).await;
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
            command: NodeCommand::ModelLifecyclePolicy { node, command },
        } => {
            let request = eefn::model_lifecycle_policy::PolicyRequest {
                schema_version: 1,
                expected_node_id: node.clone(),
                expected_runtime_id: command.runtime.clone(),
                command: command.command.clone(),
            };
            request.validate()?;
            client
                .post(format!("{base}/api/commands/node/model-lifecycle-policy"))
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
    let parsed = Args::try_parse();
    let repair = parsed.as_ref().is_ok_and(|args| {
        matches!(
            args.command,
            Some(Command::Repair { .. } | Command::RepairSetup { .. } | Command::Doctor { .. })
        )
    });
    if !repair && eefn::updater::handoff_if_selected(&install_root, eef::VERSION, "eef")? {
        return Ok(());
    }
    let mut args = parsed.unwrap_or_else(|error| error.exit());
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
                if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::VerifyModel { .. }
                        }
                    )
                {
                    println!(
                        "Node model check: {}",
                        value["state"].as_str().unwrap_or("unconfirmed")
                    );
                    println!(
                        "Node: {}\nOperation: {}\nSource receipt: {}",
                        value["node_id"].as_str().unwrap_or("unknown"),
                        value["operation_id"].as_str().unwrap_or("unknown"),
                        value["source_operation_id"].as_str().unwrap_or("unknown")
                    );
                    println!(
                        "Inspect without replay: eef node verify-model --node NODE_ID inspect --operation-id OPERATION_ID (use the values above)."
                    );
                    println!(
                        "Returned text is historical evidence, not model quality or current setup health. An unconfirmed start may have issued work; do not allocate a new operation to retry it."
                    );
                } else if !args.json && matches!(command, Command::Repair { .. }) {
                    print!("{}", eefn::package_repair::render(&value)?);
                } else if !args.json && matches!(command, Command::Doctor { .. }) {
                    print!("{}", eefn::package_health::render(&value));
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::Setup { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        println!(
                            "Node-local commands below must run on the inspected node, not a different local node."
                        );
                        print!("{}", eefn::setup_check::render(&value["result"])?);
                        println!(
                            "From EEF, inspect the same model setup with: eef node model-bootstrap-policy --node NODE_ID show (replace NODE_ID with the node ID above; remote changes still require local consent)."
                        );
                    } else {
                        eprintln!(
                            "Node setup observations unconfirmed. No retry, model setup, restart or work was requested."
                        );
                    }
                } else if !args.json && matches!(command, Command::RepairSetup { .. }) {
                    print!("{}", eefn::package_setup::render(&value));
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::RepairSetup { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        print!("{}", eefn::package_setup::policy::render(&value["result"])?);
                    } else {
                        eprintln!(
                            "Node maintenance policy unconfirmed. Inspect before a new save; no client retry or fallback was sent."
                        );
                    }
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::ModelBootstrapPolicy { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        print!("{}", eefn::model_bootstrap::render(&value["result"])?);
                    } else {
                        println!(
                            "Bootstrap preferences unconfirmed. Inspect this node with model-bootstrap-policy show; no retry, download or restart was requested."
                        );
                    }
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::Python { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        print!("{}", eefn::python_policy::render(&value["result"])?);
                    } else {
                        println!(
                            "Python policy outcome was not confirmed. Inspect this node with python show; no retry, permission change, restart or workload was sent."
                        );
                    }
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::Relay { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        print!("{}", eefn::relay_setup::render(&value["result"])?);
                    } else {
                        println!(
                            "Relay setup outcome was not confirmed. Inspect this node with relay show. No retry, permission change, restart or workload was sent."
                        );
                    }
                } else if !args.json
                    && matches!(
                        command,
                        Command::Node {
                            command: NodeCommand::Permissions { .. }
                        }
                    )
                {
                    if value["success"] == true {
                        print!("{}", eefn::permission_commands::render(&value["result"])?);
                    } else {
                        println!(
                            "Permission outcome was not confirmed. Inspect this node's permissions before deciding what to do. No retry, fallback, restart or workload was sent."
                        );
                    }
                } else {
                    println!(
                        "{}",
                        if args.json {
                            serde_json::to_string(&value)?
                        } else {
                            serde_json::to_string_pretty(&value)?
                        }
                    );
                }
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
        let (repair_stop, repair_stopped) = tokio::sync::watch::channel(false);
        let _repair_setup = eefn::package_setup::spawn_until("eef", repair_stopped);
        let listener = tokio::net::TcpListener::bind((host.as_str(), port))
            .await
            .with_context(|| format!("bind EEF API {host}:{port}"))?;
        info!(%host, %port, node_port = runtime.node_server.port(), "EEF API listening");
        // Separate secret-free owner discovery from legacy shared-key pairing.
        // A stale advertisement is harmless: clients must confirm its runtime
        // through the owner API before using any observed credential state.
        let local_address = |host: &str, port| -> Option<std::net::SocketAddr> {
            let ip: std::net::IpAddr = host.parse().ok()?;
            let ip = if ip.is_unspecified() {
                if ip.is_ipv4() {
                    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                } else {
                    std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
                }
            } else {
                ip
            };
            (ip.is_loopback() && port != 0).then_some(std::net::SocketAddr::new(ip, port))
        };
        if let (Some(owner_address), Some(node_address)) = (
            local_address(&host, listener.local_addr()?.port()),
            local_address(
                &runtime.config.string("node.host", "0.0.0.0"),
                runtime.node_server.port(),
            ),
        ) {
            if let Err(error) = eefn::local_commands::publish_owner_coordinator(
                &eefn::local_commands::OwnerCoordinator {
                    schema_version: 1,
                    owner_address,
                    node_address,
                    runtime_id: runtime.runtime_id().into(),
                },
            ) {
                tracing::warn!(%error,"local coordinator discovery unavailable; owner API remains available");
            }
        }
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
        let result = axum::serve(listener, eef::api::router_with_ui(runtime.clone(), !args.no_ui && runtime.config.bool("web.ui_enabled", true))
            .into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async move {
            tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=restart.notified()=>{requested.store(true,std::sync::atomic::Ordering::Relaxed);} }
            let _ = repair_stop.send(true);
            // Stop runners before graceful HTTP draining; an in-flight job must
            // not keep a requested restart waiting for its entire timeout.
            shutdown_runtime.shutdown().await;
        })
        .await;
        runtime.shutdown().await;
        result.context("EEF API server failed")?;
        runtime.retire(std::time::Duration::from_secs(5)).await?;
        if !restart_requested.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if eefn::updater::restart_required(&install_root, eef::VERSION, "eef")? {
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
