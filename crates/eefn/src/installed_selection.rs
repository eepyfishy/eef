//! Resolve only node-installed text GGUFs; never trust an index-provided path.
use crate::{ModelSlot, model_manifest::GgufArtifact, model_selection::ModelHints};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
#[error("{}", self.message())]
pub(crate) enum CacheIssue {
    InvalidModelId,
    MetadataMissing,
    MetadataAmbiguous,
    MetadataInvalid,
    MetadataMismatch,
    TypeUnsupported,
    CachePathMismatch,
    CacheUnavailable,
    SizeMismatch,
    ChecksumMismatch,
    CustomSelectionConflict,
    VerificationTimeout,
    VerificationBusy,
    Unclassified,
}
impl CacheIssue {
    fn message(self) -> &'static str {
        match self {
            Self::InvalidModelId => "The requested model ID is invalid.",
            Self::MetadataMissing => {
                "The model is missing from the current catalog or installed index."
            }
            Self::MetadataAmbiguous => {
                "More than one catalog, cache or selection record uses this model ID."
            }
            Self::MetadataInvalid => {
                "The model catalog or installed index is invalid or unavailable."
            }
            Self::MetadataMismatch => {
                "Installed-model metadata does not match the current catalog."
            }
            Self::TypeUnsupported => {
                "This operation supports text GGUFs only; configure other types/projectors explicitly."
            }
            Self::CachePathMismatch => {
                "The installed record does not identify a regular file in this node's content-addressed cache."
            }
            Self::CacheUnavailable => {
                "The installed cache file could not be read. It may be missing, locked or inaccessible."
            }
            Self::SizeMismatch => "The installed cache file's size does not match the catalog.",
            Self::ChecksumMismatch => "The installed cache file failed its complete SHA-256 check.",
            Self::CustomSelectionConflict => {
                "This model ID already has a different custom file/projector or invalid selection settings."
            }
            Self::VerificationTimeout => {
                "Installed cache verification exceeded its hash time budget."
            }
            Self::VerificationBusy => "Another installed cache verification is already running.",
            Self::Unclassified => "The node could not prepare this installed cache selection.",
        }
    }
}

const REJECTION_NOTE: &str = "No configuration write was attempted. No model was selected, downloaded, loaded, repaired or deleted by this request. Inspect installed models and saved selections before another explicit attempt; no automatic retry was requested.";

pub(crate) fn correlated(command: &crate::model_selection::ModelCommand) -> bool {
    let crate::model_selection::ModelCommand::SelectInstalledGguf {
        request_id: Some(id),
        ..
    } = command
    else {
        return false;
    };
    uuid::Uuid::parse_str(id).is_ok_and(|parsed| !parsed.is_nil() && parsed.to_string() == *id)
}
pub(crate) fn rejection(
    node: &str,
    command: &crate::model_selection::ModelCommand,
    error: anyhow::Error,
) -> Result<Value> {
    if !correlated(command) {
        return Err(error);
    }
    Ok(rejection_for(
        node,
        command,
        error
            .downcast_ref::<CacheIssue>()
            .copied()
            .unwrap_or(CacheIssue::Unclassified),
    ))
}
pub(crate) fn busy_rejection(
    node: &str,
    command: &crate::model_selection::ModelCommand,
) -> Result<Value> {
    rejection(node, command, CacheIssue::VerificationBusy.into())
}
fn rejection_for(
    node: &str,
    command: &crate::model_selection::ModelCommand,
    issue: CacheIssue,
) -> Value {
    json!({"schema_version":1,"report_type":"installed_model_selection_rejection","node_id":node,"command":command,
        "success":false,"error_code":"installed_selection_preflight_rejected","cache_issue":issue,
        "configuration_write_attempted":false,"changed":false,"outcome_unknown":false,"automatically_retried":false,
        "error":issue.message(),"note":REJECTION_NOTE})
}
pub(crate) fn confirmed_rejection(
    value: &Value,
    node: &str,
    command: &crate::model_selection::ModelCommand,
) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Report {
        schema_version: u32,
        report_type: String,
        node_id: String,
        command: Value,
        success: bool,
        error_code: String,
        cache_issue: CacheIssue,
        configuration_write_attempted: bool,
        changed: bool,
        outcome_unknown: bool,
        automatically_retried: bool,
        error: String,
        note: String,
    }
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    correlated(command)
        && report.schema_version == 1
        && report.report_type == "installed_model_selection_rejection"
        && report.node_id == node
        && report.command == serde_json::to_value(command).unwrap_or(Value::Null)
        && !report.success
        && report.error_code == "installed_selection_preflight_rejected"
        && !report.configuration_write_attempted
        && !report.changed
        && !report.outcome_unknown
        && !report.automatically_retried
        && report.error == report.cache_issue.message()
        && report.note == REJECTION_NOTE
}

pub(crate) struct Prepared {
    pub snapshot: Value,
    pub index_entry: Value,
    pub slot: ModelSlot,
    pub artifact: GgufArtifact,
    pub interpreter: bool,
    pub existing: bool,
    // Retain the verified read handle through the configuration commit. Windows
    // denies writer/delete access while it is held, not after selection returns.
    file: tokio::fs::File,
}

fn exact_entry(entries: &[Value], id: &str) -> Result<Value> {
    let mut matches = entries.iter().filter(|entry| entry["id"] == id);
    let entry = matches.next().ok_or(CacheIssue::MetadataMissing)?;
    if matches.next().is_some() {
        return Err(CacheIssue::MetadataAmbiguous.into());
    }
    Ok(entry.clone())
}

pub(crate) fn check_index(directory: &Path, expected: &Value) -> Result<()> {
    let current = exact_entry(
        &crate::model_manager::installed_index(directory)?,
        expected["id"]
            .as_str()
            .context("Installed model identity missing")?,
    )?;
    if current != *expected {
        bail!("Installed-model record changed during verification; inspect before retrying")
    }
    Ok(())
}

pub(crate) async fn prepare(
    directory: PathBuf,
    snapshot: Value,
    id: String,
    interpreter: bool,
) -> Result<Prepared> {
    crate::model_selection::validate_id(&id).map_err(|_| CacheIssue::InvalidModelId)?;
    let mut prepared = tokio::task::spawn_blocking(move || -> Result<Prepared> {
        let catalog = crate::model_manager::catalog(&snapshot);
        let entry = exact_entry(catalog.as_array().ok_or(CacheIssue::MetadataInvalid)?, &id)?;
        if entry["modality"] != "text" {
            return Err(CacheIssue::TypeUnsupported.into());
        }
        let artifact =
            GgufArtifact::from_catalog(&entry).map_err(|_| CacheIssue::MetadataInvalid)?;
        let index_entry = exact_entry(
            &crate::model_manager::installed_index(&directory)
                .map_err(|_| CacheIssue::MetadataInvalid)?,
            &id,
        )?;
        if index_entry["sha256"].as_str().map(str::to_ascii_lowercase)
            != Some(artifact.sha256.to_ascii_lowercase())
            || index_entry["bytes"].as_u64() != Some(artifact.bytes)
        {
            return Err(CacheIssue::MetadataMismatch.into());
        }
        let root = directory
            .canonicalize()
            .map_err(|_| CacheIssue::CacheUnavailable)?;
        let expected = directory.join(format!("{}.gguf", artifact.sha256.to_ascii_lowercase()));
        if !std::fs::symlink_metadata(&expected)
            .map_err(|_| CacheIssue::CacheUnavailable)?
            .file_type()
            .is_file()
        {
            return Err(CacheIssue::CachePathMismatch.into());
        }
        let path = expected
            .canonicalize()
            .map_err(|_| CacheIssue::CacheUnavailable)?;
        if !path.starts_with(&root)
            || index_entry["path"].as_str().map(Path::new) != Some(path.as_path())
        {
            return Err(CacheIssue::CachePathMismatch.into());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        }
        let file = options
            .open(&path)
            .map_err(|_| CacheIssue::CacheUnavailable)?;
        let metadata = file.metadata().map_err(|_| CacheIssue::CacheUnavailable)?;
        if !metadata.is_file() {
            return Err(CacheIssue::CachePathMismatch.into());
        }
        if metadata.len() != artifact.bytes {
            return Err(CacheIssue::SizeMismatch.into());
        }
        let slots = snapshot
            .pointer("/models/llamacpp/slots")
            .and_then(Value::as_array);
        let existing: Vec<_> = slots
            .into_iter()
            .flatten()
            .filter(|slot| slot["model_id"] == id)
            .collect();
        if existing.len() > 1 {
            return Err(CacheIssue::MetadataAmbiguous.into());
        }
        let already_selected = !existing.is_empty();
        let slot = if let Some(existing) = existing.first() {
            let slot: ModelSlot = serde_json::from_value((*existing).clone())
                .map_err(|_| CacheIssue::CustomSelectionConflict)?;
            if slot.model_path != path || slot.mmproj_path.is_some() {
                return Err(CacheIssue::CustomSelectionConflict.into());
            }
            slot
        } else {
            ModelSlot {
                model_id: id,
                model_path: path,
                mmproj_path: None,
                port: 0,
                gpu_layers: 0,
                context: 4096,
                cpu_threads: None,
                job_memory_limit_mib: None,
                gpu_vram_mb: 0,
                hints: ModelHints::default(),
            }
        };
        slot.selection_metadata()
            .map_err(|_| CacheIssue::CustomSelectionConflict)?;
        Ok(Prepared {
            snapshot,
            index_entry,
            slot,
            artifact,
            interpreter,
            existing: already_selected,
            file: tokio::fs::File::from_std(file),
        })
    })
    .await??;
    let verify = async {
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = prepared
                .file
                .read(&mut buffer)
                .await
                .map_err(|_| CacheIssue::CacheUnavailable)?;
            if read == 0 {
                break;
            }
            bytes = bytes
                .checked_add(read as u64)
                .ok_or(CacheIssue::SizeMismatch)?;
            if bytes > prepared.artifact.bytes {
                return Err(CacheIssue::SizeMismatch.into());
            }
            hash.update(&buffer[..read]);
        }
        if bytes != prepared.artifact.bytes
            || hex::encode(hash.finalize()) != prepared.artifact.sha256.to_ascii_lowercase()
        {
            return Err(CacheIssue::ChecksumMismatch.into());
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), verify)
        .await
        .map_err(|_| CacheIssue::VerificationTimeout)??;
    Ok(prepared)
}

impl Prepared {
    pub fn proof(&self) -> Value {
        json!({"model_id":self.slot.model_id,"sha256":self.artifact.sha256.to_ascii_lowercase(),
            "bytes":self.artifact.bytes,"verified_at_selection":true})
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        NodeService,
        model_selection::{ModelCommand, ModelCommandRequest},
    };
    use futures_util::FutureExt;
    pub(crate) fn fixture(root: &Path) -> std::sync::Arc<NodeService> {
        let directory = root.join("models");
        std::fs::create_dir(&directory).unwrap();
        let bytes = b"GGUF synthetic cache fixture, not inference weights";
        let hash = hex::encode(Sha256::digest(bytes));
        let path = directory.join(format!("{hash}.gguf"));
        std::fs::write(&path, bytes).unwrap();
        let entry = json!({"id":"fixture","name":"Fixture","modality":"text","url":"https://example.invalid/model.gguf",
            "sha256":hash,"bytes":bytes.len(),"license":"test-only"});
        let mut installed = entry.clone();
        installed["path"] = json!(path.canonicalize().unwrap());
        crate::setup::write_json(&directory.join("installed.json"), &json!([installed])).unwrap();
        let mut config: Value =
            serde_json::from_str(include_str!("../../../config/node.example.json")).unwrap();
        config["node_id"] = json!("cache-node");
        config["name"] = json!("Cache fixture");
        config["model_catalog"] = json!([entry]);
        config["models"]["provider"] = json!("auto");
        config["models"]["ollama"]["selected"] = json!([{"model_id":"other","modality":"text",
            "selection":{"roles":["request_interpreter","other_role"]}}]);
        crate::setup::write_json(&root.join("node.json"), &config).unwrap();
        NodeService::new(root.join("node.json"), "cache-node".into())
    }
    fn request(interpreter: bool) -> ModelCommandRequest {
        ModelCommandRequest {
            schema_version: 1,
            expected_node_id: "cache-node".into(),
            command: ModelCommand::SelectInstalledGguf {
                model_id: "fixture".into(),
                interpreter,
                request_id: Some("00000000-0000-4000-8000-000000000001".into()),
            },
        }
    }
    #[tokio::test]
    async fn installed_cache_selection_verifies_without_paths_downloads_provider_changes_or_restart()
     {
        let root = tempfile::tempdir().unwrap();
        let service = fixture(root.path());
        let before = service.read_config().unwrap();
        let result = service
            .dispatch_model_command(request(true), false)
            .await
            .unwrap();
        assert!(crate::model_selection::confirmed_interpreter(
            &result,
            "cache-node",
            &request(true).command
        ));
        assert_eq!(result["changed"], true);
        assert_eq!(result["provider"], "auto");
        let after = service.read_config().unwrap();
        assert_eq!(after["permissions"], before["permissions"]);
        assert_eq!(
            after["models"]["ollama"]["selected"][0]["selection"]["roles"],
            json!(["other_role"])
        );
        assert!(
            after["models"]["llamacpp"]["slots"][0]["port"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(service.live.lock().unwrap()["download"]["state"], "idle");
        assert!(service.restart.notified().now_or_never().is_none());
        assert!(
            !result
                .to_string()
                .contains(&root.path().to_string_lossy().to_string())
        );
        let raw = std::fs::read(root.path().join("node.json")).unwrap();
        assert_eq!(
            service
                .dispatch_model_command(request(false), false)
                .await
                .unwrap()["changed"],
            false
        );
        assert_eq!(std::fs::read(root.path().join("node.json")).unwrap(), raw);
        for (key, value) in [
            ("sha256", json!("bad")),
            ("verified_at_selection", json!(false)),
            ("model_id", json!("wrong")),
        ] {
            let mut forged = result.clone();
            forged["installed_artifact"][key] = value;
            assert!(!crate::model_selection::confirmed_interpreter(
                &forged,
                "cache-node",
                &request(true).command
            ));
        }
    }
    #[tokio::test]
    async fn invalid_cache_records_and_corruption_never_change_config_or_delete_files() {
        for case in [
            "path",
            "digest",
            "duplicate-index",
            "duplicate-catalog",
            "vision",
            "corrupt",
            "custom-selection",
        ] {
            let root = tempfile::tempdir().unwrap();
            let service = fixture(root.path());
            let dir = root.path().join("models");
            let mut config = service.read_config().unwrap();
            let mut index: Value =
                serde_json::from_slice(&std::fs::read(dir.join("installed.json")).unwrap())
                    .unwrap();
            let path = PathBuf::from(index[0]["path"].as_str().unwrap());
            match case {
                "path" => index[0]["path"] = json!("//untrusted.invalid/never-read/model.gguf"),
                "digest" => index[0]["sha256"] = json!("a".repeat(64)),
                "duplicate-index" => {
                    let entry = index[0].clone();
                    index.as_array_mut().unwrap().push(entry);
                }
                "duplicate-catalog" => {
                    let entry = config["model_catalog"][0].clone();
                    config["model_catalog"].as_array_mut().unwrap().push(entry);
                }
                "vision" => config["model_catalog"][0]["modality"] = json!("vlm"),
                "corrupt" => {
                    let n = std::fs::metadata(&path).unwrap().len() as usize;
                    std::fs::write(&path, vec![b'x'; n]).unwrap();
                }
                "custom-selection" => {
                    config["models"]["llamacpp"]["slots"] = json!([{"model_id":"fixture","model_path":"//untrusted.invalid/never-read/other.gguf"}])
                }
                _ => unreachable!(),
            }
            crate::setup::write_json(&dir.join("installed.json"), &index).unwrap();
            crate::setup::write_json(&root.path().join("node.json"), &config).unwrap();
            let before = std::fs::read(root.path().join("node.json")).unwrap();
            let result = service
                .dispatch_model_command(request(true), false)
                .await
                .unwrap();
            let expected = match case {
                "path" => "cache_path_mismatch",
                "digest" => "metadata_mismatch",
                "duplicate-index" | "duplicate-catalog" => "metadata_ambiguous",
                "vision" => "type_unsupported",
                "corrupt" => "checksum_mismatch",
                "custom-selection" => "custom_selection_conflict",
                _ => unreachable!(),
            };
            assert_eq!(result["cache_issue"], expected, "{case}: {result}");
            assert!(confirmed_rejection(
                &result,
                "cache-node",
                &request(true).command
            ));
            assert_eq!(result["configuration_write_attempted"], false);
            assert_eq!(result["outcome_unknown"], false);
            assert!(!result.to_string().contains("untrusted.invalid"));
            assert_eq!(
                std::fs::read(root.path().join("node.json")).unwrap(),
                before,
                "{case}"
            );
            assert!(path.is_file(), "{case}");
        }
    }
    #[tokio::test]
    async fn installed_cache_remote_denial_and_single_flight_do_not_start_changes() {
        let root = tempfile::tempdir().unwrap();
        let service = fixture(root.path());
        let before = std::fs::read(root.path().join("node.json")).unwrap();
        assert_eq!(
            service
                .dispatch_model_command(request(true), true)
                .await
                .unwrap()["error_code"],
            "approval_required"
        );
        let _busy = service.installed_selection.lock().await;
        let result = service
            .dispatch_model_command(request(true), false)
            .await
            .unwrap();
        assert_eq!(result["cache_issue"], "verification_busy");
        assert!(confirmed_rejection(
            &result,
            "cache-node",
            &request(true).command
        ));
        assert_eq!(
            std::fs::read(root.path().join("node.json")).unwrap(),
            before
        );
    }

    #[test]
    fn rejection_confirmation_requires_exact_fresh_correlation_and_static_fields() {
        let command = request(true).command;
        let report = rejection(
            "cache-node",
            &command,
            anyhow::anyhow!("private path C:/private/model.gguf token=secret"),
        )
        .unwrap();
        assert!(confirmed_rejection(&report, "cache-node", &command));
        assert_eq!(report["cache_issue"], "unclassified");
        assert!(!report.to_string().contains("private"));
        assert!(!confirmed_rejection(&report, "other-node", &command));
        for id in [
            None,
            Some("bad"),
            Some("00000000-0000-0000-0000-000000000000"),
            Some("00000000-0000-4000-8000-000000000002"),
        ] {
            let mut other = request(true).command;
            if let ModelCommand::SelectInstalledGguf { request_id, .. } = &mut other {
                *request_id = id.map(str::to_owned);
            }
            assert!(!confirmed_rejection(&report, "cache-node", &other));
            if !correlated(&other) {
                assert!(
                    rejection("cache-node", &other, CacheIssue::CacheUnavailable.into()).is_err()
                );
            }
        }
        for (field, value) in [
            ("changed", json!(true)),
            ("configuration_write_attempted", json!(true)),
            ("outcome_unknown", json!(true)),
            ("automatically_retried", json!(true)),
            ("success", json!(true)),
            ("error", json!("private text")),
            ("unexpected", json!(1)),
        ] {
            let mut forged = report.clone();
            forged[field] = value;
            assert!(
                !confirmed_rejection(&forged, "cache-node", &command),
                "{field}"
            );
        }
    }

    #[tokio::test]
    async fn legacy_selection_still_succeeds_but_cannot_confirm_preflight_failure() {
        let root = tempfile::tempdir().unwrap();
        let service = fixture(root.path());
        let mut legacy = request(false);
        if let ModelCommand::SelectInstalledGguf { request_id, .. } = &mut legacy.command {
            *request_id = None;
        }
        assert_eq!(
            service
                .dispatch_model_command(legacy.clone(), false)
                .await
                .unwrap()["success"],
            true
        );
        let before = std::fs::read(root.path().join("node.json")).unwrap();
        let _busy = service.installed_selection.lock().await;
        assert!(service.dispatch_model_command(legacy, false).await.is_err());
        let mut invalid = request(false);
        if let ModelCommand::SelectInstalledGguf { request_id, .. } = &mut invalid.command {
            *request_id = Some("bad".into());
        }
        assert!(
            service
                .dispatch_model_command(invalid, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("canonical non-nil UUID")
        );
        assert_eq!(
            std::fs::read(root.path().join("node.json")).unwrap(),
            before
        );
    }
}
