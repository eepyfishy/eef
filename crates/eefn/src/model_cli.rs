//! Shared CLI adapter. Remote paths are resolved only by the target node.
use crate::model_selection::{Backend, ModelCommand, ModelHints};
use crate::{ModelSlot, SelectedModel};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    Show,
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
        #[arg(long = "capability")]
        capabilities: Vec<String>,
        #[arg(long = "role")]
        roles: Vec<String>,
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
    fn remote_paths_are_not_resolved_on_the_coordinator() {
        let path = PathBuf::from("/nonexistent-node-volume/fixture.gguf");
        let action = ModelsAction::SelectGguf {
            model: "fixture".into(),
            file: path.clone(),
            projector: Some(path.clone()),
            port: None,
            gpu_layers: 0,
            context: 4096,
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
