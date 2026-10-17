//! Shared CLI adapter. Remote paths are resolved only by the target node.
use crate::model_selection::{Backend, ModelCommand, ModelHints};
use crate::{ModelSlot, SelectedModel};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    Show,
    /// Assign one existing selected model as interpreter; preserves other roles.
    /// Moves that role off all other selections. Restart separately to apply.
    Interpreter {
        #[arg(long,value_parser=["ollama","llamacpp"])]
        backend: String,
        #[arg(long)]
        model: String,
    },
    /// Remove the interpreter role without removing models or other roles.
    ClearInterpreter,
    /// Set local GGUF generation/prompt workers; save only, restart separately.
    CpuThreads {
        #[arg(long)]
        model: String,
        #[arg(long, required_unless_present = "auto", conflicts_with = "auto",
            value_parser = clap::value_parser!(u16).range(1..=256))]
        threads: Option<u16>,
        /// Restore backend defaults, not a measured CPU budget.
        #[arg(long)]
        auto: bool,
    },
    /// Save an owned GGUF Windows job-commit ceiling; restart separately to apply.
    MemoryLimit {
        #[arg(long)]
        model: String,
        #[arg(long,required_unless_present="default",conflicts_with="default",
            value_parser=clap::value_parser!(u64).range(64..=1048576))]
        mib: Option<u64>,
        /// Restore no job-commit ceiling. Does not change a running model.
        #[arg(long)]
        default: bool,
    },
    /// Select an Ollama model ID. Installation/availability is checked on connection.
    SelectOllama {
        #[arg(long)]
        model: String,
        #[arg(long, value_parser=["text","vlm"])]
        modality: String,
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        #[arg(long = "role")]
        roles: Vec<String>,
    },
    /// Select an existing GGUF; internal service port is allocated automatically.
    SelectGguf {
        #[arg(long)]
        model: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        projector: Option<PathBuf>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, default_value_t = 0)]
        gpu_layers: i32,
        #[arg(long, default_value_t = 4096)]
        context: u32,
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=256))]
        cpu_threads: Option<u16>,
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        #[arg(long = "role")]
        roles: Vec<String>,
    },
    /// Select an already-installed text GGUF by catalog ID; no path/port or download.
    SelectInstalledGguf {
        #[arg(long)]
        model: String,
        /// Explicitly move the interpreter role to this model; restart separately.
        #[arg(long)]
        interpreter: bool,
    },
    /// Remove selection only, never model files.
    Remove {
        #[arg(long,value_parser=["ollama","llamacpp"])]
        backend: String,
        #[arg(long)]
        model: String,
    },
    /// Update an existing selection's restrictions or role labels.
    Hints {
        #[arg(long,value_parser=["ollama","llamacpp"])]
        backend: String,
        #[arg(long)]
        model: String,
        #[arg(long = "capability", conflicts_with = "clear_capabilities")]
        capabilities: Vec<String>,
        #[arg(long)]
        clear_capabilities: bool,
        #[arg(long = "role", conflicts_with = "clear_roles")]
        roles: Vec<String>,
        #[arg(long)]
        clear_roles: bool,
    },
    /// Choose which configured backend activates on restart (auto prefers Ollama).
    Provider {
        #[arg(value_parser=["auto","ollama","llamacpp"])]
        provider: String,
    },
}

impl ModelsAction {
    pub fn to_command(&self, local_paths: bool) -> Result<ModelCommand> {
        let resolve = |path: &PathBuf| -> Result<PathBuf> {
            if local_paths {
                std::fs::canonicalize(path).context("model/projector file does not exist")
            } else {
                Ok(path.clone())
            }
        };
        let backend = |name: &str| {
            if name == "ollama" {
                Backend::Ollama
            } else {
                Backend::Llamacpp
            }
        };
        let hints = |capabilities: &Vec<String>, roles: &Vec<String>| ModelHints {
            capabilities: (!capabilities.is_empty()).then(|| capabilities.clone()),
            roles: (!roles.is_empty()).then(|| roles.clone()),
        };
        let command = match self {
            ModelsAction::Show => ModelCommand::Show {},
            ModelsAction::SelectInstalledGguf { model, interpreter } => {
                ModelCommand::SelectInstalledGguf {
                    model_id: model.clone(),
                    interpreter: *interpreter,
                }
            }
            ModelsAction::Interpreter {
                backend: name,
                model,
            } => ModelCommand::Interpreter {
                backend: backend(name),
                model_id: model.clone(),
            },
            ModelsAction::ClearInterpreter => ModelCommand::ClearInterpreter {},
            ModelsAction::MemoryLimit { model, mib, .. } => ModelCommand::MemoryLimit {
                model_id: model.clone(),
                mib: *mib,
            },
            ModelsAction::CpuThreads { model, threads, .. } => ModelCommand::CpuThreads {
                model_id: model.clone(),
                threads: *threads,
            },
            ModelsAction::Provider { provider } => ModelCommand::Provider {
                provider: provider.clone(),
            },
            ModelsAction::SelectOllama {
                model,
                modality,
                capabilities,
                roles,
            } => ModelCommand::SelectOllama {
                model: SelectedModel {
                    model_id: model.clone(),
                    modality: modality.clone(),
                    hints: hints(capabilities, roles),
                },
            },
            ModelsAction::SelectGguf {
                model,
                file,
                projector,
                port,
                gpu_layers,
                context,
                cpu_threads,
                capabilities,
                roles,
            } => ModelCommand::SelectGguf {
                slot: ModelSlot {
                    model_id: model.clone(),
                    model_path: resolve(file)?,
                    mmproj_path: projector.as_ref().map(resolve).transpose()?,
                    port: port.unwrap_or(0),
                    gpu_layers: *gpu_layers,
                    context: *context,
                    cpu_threads: *cpu_threads,
                    job_memory_limit_mib: None,
                    gpu_vram_mb: 0,
                    hints: hints(capabilities, roles),
                },
            },
            ModelsAction::Remove {
                backend: name,
                model,
            } => ModelCommand::Remove {
                backend: backend(name),
                model_id: model.clone(),
            },
            ModelsAction::Hints {
                backend: name,
                model,
                capabilities,
                clear_capabilities,
                roles,
                clear_roles,
            } => ModelCommand::Hints {
                backend: backend(name),
                model_id: model.clone(),
                capabilities: (!capabilities.is_empty() || *clear_capabilities)
                    .then(|| capabilities.clone()),
                roles: (!roles.is_empty() || *clear_roles).then(|| roles.clone()),
            },
        };

        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_selection_cli_requires_only_model_id_and_explicit_optional_role() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(subcommand)]
            action: ModelsAction,
        }
        use clap::Parser;
        for args in [
            vec!["models", "select-installed-gguf", "--model", "chosen"],
            vec![
                "models",
                "select-installed-gguf",
                "--model",
                "chosen",
                "--interpreter",
            ],
        ] {
            let wants_role = args.contains(&"--interpreter");
            let command = Cli::try_parse_from(args)
                .unwrap()
                .action
                .to_command(true)
                .unwrap();
            assert_eq!(
                serde_json::to_value(command).unwrap(),
                serde_json::json!({
                "operation":"select_installed_gguf","model_id":"chosen","interpreter":wants_role})
            );
        }
        for args in [
            vec!["models", "select-installed-gguf"],
            vec![
                "models",
                "select-installed-gguf",
                "--model",
                "chosen",
                "--file",
                "caller-path",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn memory_limit_cli_requires_an_explicit_value_or_default_reset() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(subcommand)]
            action: ModelsAction,
        }
        use clap::Parser;
        for (args, wanted) in [
            (
                vec!["eefn", "memory-limit", "--model", "m", "--mib", "128"],
                Some(128),
            ),
            (
                vec!["eefn", "memory-limit", "--model", "m", "--default"],
                None,
            ),
        ] {
            let ModelCommand::MemoryLimit { model_id, mib } = Cli::try_parse_from(args)
                .unwrap()
                .action
                .to_command(false)
                .unwrap()
            else {
                panic!("wrong command")
            };
            assert_eq!(model_id, "m");
            assert_eq!(mib, wanted);
        }
        for args in [
            vec!["eefn", "memory-limit", "--model", "m"],
            vec!["eefn", "memory-limit", "--model", "m", "--mib", "63"],
            vec![
                "eefn",
                "memory-limit",
                "--model",
                "m",
                "--mib",
                "128",
                "--default",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn cpu_thread_cli_requires_exact_model_and_explicit_bounded_or_auto_choice() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(subcommand)]
            action: ModelsAction,
        }
        use clap::Parser;
        for args in [
            vec!["models", "cpu-threads", "--model", "local"],
            vec!["models", "cpu-threads", "--threads", "4"],
            vec![
                "models",
                "cpu-threads",
                "--model",
                "local",
                "--threads",
                "0",
            ],
            vec![
                "models",
                "cpu-threads",
                "--model",
                "local",
                "--threads",
                "257",
            ],
            vec![
                "models",
                "cpu-threads",
                "--model",
                "local",
                "--threads",
                "4",
                "--auto",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        for (choice, expected) in [(vec!["--threads", "4"], Some(4)), (vec!["--auto"], None)] {
            let mut args = vec!["models", "cpu-threads", "--model", "local"];
            args.extend(choice);
            let ModelCommand::CpuThreads { model_id, threads } = Cli::try_parse_from(args)
                .unwrap()
                .action
                .to_command(false)
                .unwrap()
            else {
                panic!("wrong command")
            };
            assert_eq!(model_id, "local");
            assert_eq!(threads, expected);
        }
    }
    #[test]
    fn interpreter_cli_requires_an_explicit_existing_model_identity() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(subcommand)]
            action: ModelsAction,
        }
        for args in [
            vec!["models", "interpreter"],
            vec!["models", "interpreter", "--backend", "ollama"],
            vec![
                "models",
                "interpreter",
                "--backend",
                "automatic",
                "--model",
                "fixture",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        let action = Cli::try_parse_from([
            "models",
            "interpreter",
            "--backend",
            "llamacpp",
            "--model",
            "fixture",
        ])
        .unwrap()
        .action;
        assert!(
            matches!(action.to_command(false).unwrap(), ModelCommand::Interpreter {backend:Backend::Llamacpp,model_id} if model_id == "fixture")
        );
        let clear = Cli::try_parse_from(["models", "clear-interpreter"])
            .unwrap()
            .action;
        assert!(matches!(
            clear.to_command(true).unwrap(),
            ModelCommand::ClearInterpreter {}
        ));
        assert!(serde_json::from_value::<ModelCommand>(serde_json::json!({"operation":"interpreter","backend":"ollama","model_id":"fixture","provider":"auto"})).is_err());
    }
    #[test]
    fn remote_paths_are_not_resolved_on_the_coordinator() {
        let path = PathBuf::from("/nonexistent-node-volume/fixture.gguf");
        let action = ModelsAction::SelectGguf {
            model: "fixture".into(),
            file: path.clone(),
            projector: Some(path.clone()),
            port: None,
            gpu_layers: 0,
            context: 4096,
            cpu_threads: None,
            capabilities: vec![],
            roles: vec![],
        };
        let ModelCommand::SelectGguf { slot } = action.to_command(false).unwrap() else {
            panic!("wrong command")
        };
        assert_eq!(slot.model_path, path);
        assert_eq!(slot.mmproj_path, Some(path));
        assert!(action.to_command(true).is_err());
    }
}
