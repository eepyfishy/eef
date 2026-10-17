//! Resolve only node-installed text GGUFs; never trust an index-provided path.
use crate::{ModelSlot, model_manifest::GgufArtifact, model_selection::ModelHints};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

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
    let entry = matches
        .next()
        .context("Model is not in the node's installed catalog cache")?;
    if matches.next().is_some() {
        bail!("Duplicate model IDs in catalog/cache")
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
    crate::model_selection::validate_id(&id)?;
    let mut prepared = tokio::task::spawn_blocking(move || -> Result<Prepared> {
        let catalog = crate::model_manager::catalog(&snapshot);
        let entry = exact_entry(catalog.as_array().context("Model catalog must be a list")?, &id)?;
        if entry["modality"] != "text" {
            bail!("Automatic cache selection supports text GGUFs only; select projectors/types explicitly")
        }
        let artifact = GgufArtifact::from_catalog(&entry)?;
        let index_entry = exact_entry(&crate::model_manager::installed_index(&directory)?, &id)?;
        if index_entry["sha256"].as_str().map(str::to_ascii_lowercase) != Some(artifact.sha256.to_ascii_lowercase())
            || index_entry["bytes"].as_u64() != Some(artifact.bytes) {
            bail!("Installed-model metadata does not match the current catalog")
        }
        let root = directory.canonicalize()?;
        let expected = directory.join(format!("{}.gguf", artifact.sha256.to_ascii_lowercase()));
        if std::fs::symlink_metadata(&expected)?.file_type().is_symlink() {
            bail!("Installed model must be a regular cache file, not a symbolic link")
        }
        let path = expected.canonicalize()?;
        if !path.starts_with(&root) || index_entry["path"].as_str().map(Path::new) != Some(path.as_path()) {
            bail!("Installed model path does not identify this node's content-addressed cache")
        }
        let mut options = std::fs::OpenOptions::new(); options.read(true);
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
        }
        let file = options.open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != artifact.bytes {
            bail!("Installed model size does not match the current catalog")
        }
        let slots = snapshot.pointer("/models/llamacpp/slots").and_then(Value::as_array);
        let existing: Vec<_> = slots.into_iter().flatten().filter(|slot| slot["model_id"] == id).collect();
        if existing.len() > 1 { bail!("Duplicate selected model IDs") }
        let already_selected = !existing.is_empty();
        let slot = if let Some(existing) = existing.first() {
            let slot: ModelSlot = serde_json::from_value((*existing).clone())?;
            if slot.model_path != path || slot.mmproj_path.is_some() {
                bail!("An existing custom selection uses this ID; inspect it instead of replacing its file/projector")
            }
            slot
        } else { ModelSlot {
            model_id: id, model_path: path, mmproj_path: None, port: 0,
            gpu_layers: 0, context: 4096, cpu_threads: None, job_memory_limit_mib: None,
            gpu_vram_mb: 0, hints: ModelHints::default(),
        }};
        slot.selection_metadata()?;
        Ok(Prepared {snapshot, index_entry, slot, artifact, interpreter, existing:already_selected, file: tokio::fs::File::from_std(file)})
    }).await??;
    let verify = async {
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = prepared.file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            bytes = bytes
                .checked_add(read as u64)
                .context("Model verification size overflow")?;
            if bytes > prepared.artifact.bytes {
                bail!("Installed model grew during verification")
            }
            hash.update(&buffer[..read]);
        }
        if bytes != prepared.artifact.bytes
            || hex::encode(hash.finalize()) != prepared.artifact.sha256.to_ascii_lowercase()
        {
            bail!("Installed model checksum verification failed; no selection changed")
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), verify)
        .await
        .context("Installed model verification timed out; no selection changed")??;
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
            let error = service
                .dispatch_model_command(request(true), false)
                .await
                .unwrap_err();
            let expected = match case {
                "path" => "Installed model path",
                "digest" => "metadata does not match",
                "duplicate-index" | "duplicate-catalog" => "Duplicate model IDs",
                "vision" => "text GGUFs only",
                "corrupt" => "checksum verification failed",
                "custom-selection" => "custom selection",
                _ => unreachable!(),
            };
            assert!(error.to_string().contains(expected), "{case}: {error:#}");
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
        assert!(
            service
                .dispatch_model_command(request(true), false)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("node.json")).unwrap(),
            before
        );
    }
}
