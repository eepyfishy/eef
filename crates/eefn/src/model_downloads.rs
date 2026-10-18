//! Local owner model-transfer commands, backed by the existing installer.
//! Durable installation receipts; byte progress remains live-only, never resumed.
use crate::NodeService;
use anyhow::{Result, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Debug, Subcommand)]
pub enum NodeModelsAction {
    #[command(flatten)]
    Selection(crate::model_cli::ModelsAction),
    /// Inspect installed models, the configured catalog and backend storage limits.
    Installed,
    /// Inspect an installed Ollama model's reported capabilities, without loading it.
    Inspect {
        #[arg(long)]
        model: String,
    },
    /// Read-only backend, artifact and disk preflight; does not download or load.
    InstallPlan {
        #[arg(long)]
        model: String,
    },
    /// Request installation once; does not select/load the model or restart the node.
    Install {
        #[arg(long)]
        model: String,
    },
    /// Inspect a retained installation outcome and current live progress when available.
    DownloadStatus {
        #[arg(long)]
        id: Option<String>,
    },
    /// Request cancellation of this exact download, not whichever transfer is current.
    CancelDownload {
        #[arg(long)]
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DownloadCommand {
    Installed {},
    Inspect { model: String },
    Plan { model: String },
    Install { model: String, operation_id: String },
    Status { operation_id: Option<String> },
    Cancel { operation_id: String },
}

impl DownloadCommand {
    pub fn operation_id(&self) -> Option<&str> {
        match self {
            Self::Install { operation_id, .. } | Self::Cancel { operation_id } => {
                Some(operation_id)
            }
            Self::Status { operation_id } => operation_id.as_deref(),
            _ => None,
        }
    }
    pub fn mutating(&self) -> bool {
        matches!(self, Self::Install { .. } | Self::Cancel { .. })
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(id) = self.operation_id() {
            if uuid::Uuid::parse_str(id).is_err() || uuid::Uuid::parse_str(id)?.to_string() != id {
                bail!("download ID must be a canonical UUID");
            }
        }
        if let Self::Install { model, .. } | Self::Inspect { model } | Self::Plan { model } = self {
            crate::model_selection::validate_id(model)?;
        }
        Ok(())
    }
}

impl NodeModelsAction {
    pub fn download_command(&self) -> Option<DownloadCommand> {
        Some(match self {
            Self::Selection(_) => return None,
            Self::Installed => DownloadCommand::Installed {},
            Self::Inspect { model } => DownloadCommand::Inspect {
                model: model.clone(),
            },
            Self::InstallPlan { model } => DownloadCommand::Plan {
                model: model.clone(),
            },
            Self::Install { model } => DownloadCommand::Install {
                model: model.clone(),
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
            Self::DownloadStatus { id } => DownloadCommand::Status {
                operation_id: id.clone(),
            },
            Self::CancelDownload { id } => DownloadCommand::Cancel {
                operation_id: id.clone(),
            },
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: DownloadCommand,
}

impl NodeService {
    pub async fn download_command(self: &Arc<Self>, request: DownloadRequest) -> Value {
        let id = request
            .command
            .operation_id()
            .filter(|id| id.len() == 36)
            .map(str::to_owned);
        let result = async {
            if request.schema_version != 1 || request.expected_node_id != self.node_id {
                bail!("download command does not match this node or schema");
            }
            request.command.validate()?;
            match request.command {
                DownloadCommand::Installed {} => Ok(json!({"inventory":crate::model_manager::list(self).await?})),
                DownloadCommand::Inspect { model } => Ok(json!({"model":crate::model_manager::inspect(self, json!({"id":model})).await?})),
                DownloadCommand::Plan { model } => Ok(json!({"plan":crate::model_manager::install_plan(self, &model).await?})),
                DownloadCommand::Install { model, operation_id } => {
                    crate::model_manager::install_identified(self.clone(), model, operation_id).await?;
                    Ok(json!({"accepted":true,"completed":false,"selection_changed":false,"restart_requested":false,
                        "note":"Download admitted. Inspect its receipt for completion; installation does not select or load the model."}))
                }
                DownloadCommand::Status { operation_id } => {
                    let mut progress = self.live.lock().unwrap()["download"].clone();
                    let query=operation_id.as_deref().or_else(||progress["id"].as_str());
                    let receipt=if let Some(id)=query { self.download_receipts.lock().map_err(|_|anyhow::anyhow!("Model download receipt owner unavailable"))?.get(id)? } else { None };
                    if query.is_some() && receipt.is_none() {
                        return Ok(json!({"success":false,"error_code":"operation_not_found","known":false,
                            "note":"No retained receipt matches this ID. Missing evidence is not proof an earlier request was never accepted."}));
                    }
                    if let Some(receipt)=receipt {
                        if progress["id"]!=receipt.id || progress["state"]=="interrupted" { progress=receipt.progress(); }
                        else {progress["durable_receipt"]=json!(receipt);}
                    }
                    Ok(json!({"download":progress,"known":true}))
                }
                DownloadCommand::Cancel { operation_id } => {
                    if self.live.lock().unwrap()["download"]["id"].as_str() != Some(&operation_id) {
                        bail!("download ID is not the retained transfer; nothing was cancelled");
                    }
                    let requested = crate::model_manager::cancel(self, Some(&operation_id))?;
                    Ok(json!({"cancel_requested":requested,"completed":false,
                        "note":"Inspect download status for confirmation. Shared Ollama transfers/layers can outlive this node's cancellation."}))
                }
            }
        }.await;
        let mut report = match result {
            Ok(value) => value,
            Err(error) => {
                json!({"success":false,"error_code":"download_command_rejected","error":error.to_string()})
            }
        };
        if report.get("success").is_none() {
            report["success"] = json!(true);
        }
        report["schema_version"] = json!(1);
        report["report_type"] = json!("model_download");
        report["node_id"] = json!(self.node_id);
        report["operation_id"] = json!(id);
        report["retention"] = json!("durable_receipts");
        report["progress_retention"] = json!("live_only");
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn exact_receipts_scope_cancel_and_unknown_does_not_mean_not_sent() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("node.json"), "node".into());
        let active = uuid::Uuid::new_v4().to_string();
        service.begin_model_download(&service.read_config().unwrap(),false,
            json!({"id":active,"model_id":"fixture","backend":"ollama","definition_sha256":"a".repeat(64),"state":"downloading","cancel_requested":false})).unwrap();
        let request = |command| DownloadRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            command,
        };
        let wrong = uuid::Uuid::new_v4().to_string();
        let status = service
            .download_command(request(DownloadCommand::Status {
                operation_id: Some(wrong.clone()),
            }))
            .await;
        assert_eq!(status["error_code"], "operation_not_found");
        assert_eq!(status["known"], false);
        let cancel = service
            .download_command(request(DownloadCommand::Cancel {
                operation_id: wrong,
            }))
            .await;
        assert_eq!(cancel["success"], false);
        assert_eq!(
            service.live.lock().unwrap()["download"]["cancel_requested"],
            false
        );
        let mut wrong_node = request(DownloadCommand::Cancel {
            operation_id: active.clone(),
        });
        wrong_node.expected_node_id = "other".into();
        assert_eq!(service.download_command(wrong_node).await["success"], false);
        let cancel = service
            .download_command(request(DownloadCommand::Cancel {
                operation_id: active.clone(),
            }))
            .await;
        assert_eq!(cancel["cancel_requested"], true);
        assert_eq!(cancel["completed"], false);
        service
            .download_receipts
            .lock()
            .unwrap()
            .finish(&active, crate::download_receipts::State::Cancelled)
            .unwrap();
        service.live.lock().unwrap()["download"]["state"] = json!("cancelled");
        let cancel = service
            .download_command(request(DownloadCommand::Cancel {
                operation_id: active,
            }))
            .await;
        assert_eq!(cancel["cancel_requested"], false);
    }

    #[test]
    fn typed_download_inputs_reject_unbounded_ids_unknown_fields_and_bad_schema_values() {
        for id in [
            "x".to_string(),
            "x".repeat(8192),
            uuid::Uuid::new_v4().simple().to_string(),
        ] {
            assert!(
                DownloadCommand::Cancel { operation_id: id }
                    .validate()
                    .is_err()
            );
        }
        assert!(serde_json::from_value::<DownloadCommand>(json!({"operation":"install","model":"m","operation_id":uuid::Uuid::new_v4().to_string(),"url":"https://invalid.example"})).is_err());
    }

    #[tokio::test]
    async fn installation_receipt_reuses_verified_cache_without_selecting_or_replaying() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("node.json");
        let bytes = b"verification fixture, not model weights";
        let hash = hex::encode(Sha256::digest(bytes));
        let config = json!({"node_id":"node","models":{"provider":"llamacpp","ollama":{"base_url":"http://127.0.0.1:1"}},
            "model_catalog":[{"id":"fixture","name":"Fixture","url":"https://example.invalid/never-requested","sha256":hash,"bytes":bytes.len()}]});
        crate::setup::write_json(&config_path, &config).unwrap();
        let service = NodeService::new(config_path.clone(), "node".into());
        std::fs::create_dir_all(service.model_directory()).unwrap();
        std::fs::write(
            service.model_directory().join(format!("{hash}.gguf")),
            bytes,
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let install = || DownloadRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            command: DownloadCommand::Install {
                model: "fixture".into(),
                operation_id: id.clone(),
            },
        };
        let accepted = service.download_command(install()).await;
        assert_eq!(accepted["accepted"], true);
        assert_eq!(accepted["operation_id"], id);
        assert_eq!(accepted["selection_changed"], false);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if service.live.lock().unwrap()["download"]["state"] != "downloading" {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            service.live.lock().unwrap()["download"]["state"],
            "installed"
        );
        assert_eq!(service.download_command(install()).await["success"], false);
        assert_eq!(
            service.live.lock().unwrap()["download"]["state"],
            "installed"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(config_path).unwrap()).unwrap(),
            config
        );
    }

    #[tokio::test]
    async fn verified_file_with_failed_final_receipt_is_unknown_not_installed_success() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let bytes = b"synthetic verified cache";
        let hash = hex::encode(Sha256::digest(bytes));
        let config = json!({"node_id":"node","models":{"provider":"llamacpp"},"model_catalog":[{"id":"fixture","url":"https://example.invalid/not-fetched","sha256":hash,"bytes":bytes.len()}]});
        crate::setup::write_json(&path, &config).unwrap();
        let service = NodeService::new(path.clone(), "node".into());
        std::fs::create_dir_all(service.model_directory()).unwrap();
        let file = service.model_directory().join(format!("{hash}.gguf"));
        std::fs::write(&file, bytes).unwrap();
        let seed = uuid::Uuid::new_v4().to_string();
        service.begin_model_download(&config,false,json!({"id":seed,"model_id":"fixture","backend":"llamacpp","total":bytes.len(),"definition_sha256":"a".repeat(64),"state":"downloading"})).unwrap();
        service
            .download_receipts
            .lock()
            .unwrap()
            .finish(&seed, crate::download_receipts::State::Error)
            .unwrap();
        service.live.lock().unwrap()["download"]["state"] = json!("error");
        let mut journal = path.as_os_str().to_os_string();
        journal.push(".model-downloads.sqlite3");
        let db = rusqlite::Connection::open(std::path::PathBuf::from(journal)).unwrap();
        db.execute_batch("CREATE TRIGGER final_fault BEFORE UPDATE ON model_download_receipts WHEN json_extract(NEW.payload,'$.state')='installed' BEGIN SELECT RAISE(ABORT,'fixture final receipt failure'); END;").unwrap();
        drop(db);
        let id = uuid::Uuid::new_v4().to_string();
        let request = |command| DownloadRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            command,
        };
        assert_eq!(
            service
                .download_command(request(DownloadCommand::Install {
                    model: "fixture".into(),
                    operation_id: id.clone()
                }))
                .await["accepted"],
            true
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while service.live.lock().unwrap()["download"]["state"] == "downloading"
                || Arc::strong_count(&service) != 1
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let status = service
            .download_command(request(DownloadCommand::Status {
                operation_id: Some(id.clone()),
            }))
            .await;
        assert_eq!(status["download"]["state"], "unknown");
        assert_eq!(status["download"]["receipt_checkpoint_failed"], true);
        assert_eq!(
            status["download"]["durable_receipt"]["state"],
            "downloading"
        );
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert_eq!(
            crate::model_manager::installed_index(&service.model_directory())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(service.read_config().unwrap(), config);
        drop(service);
        let recovered = NodeService::new(path, "node".into());
        let status = recovered
            .download_command(request(DownloadCommand::Status {
                operation_id: Some(id),
            }))
            .await;
        assert_eq!(status["download"]["state"], "interrupted");
        assert!(status["download"]["completed"].is_null());
        assert_eq!(status["download"]["termination_confirmed"], false);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
    }
    #[test]
    fn flattened_cli_preserves_selection_commands_and_requires_exact_cancel_id() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(subcommand)]
            action: NodeModelsAction,
        }
        assert!(matches!(
            Cli::try_parse_from(["models", "show"]).unwrap().action,
            NodeModelsAction::Selection(_)
        ));
        assert!(matches!(
            Cli::try_parse_from(["models", "install", "--model", "fixture"])
                .unwrap()
                .action,
            NodeModelsAction::Install { .. }
        ));
        assert!(Cli::try_parse_from(["models", "cancel-download"]).is_err());
        assert!(Cli::try_parse_from(["models", "download-status"]).is_ok());
    }
}
