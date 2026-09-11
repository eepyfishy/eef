//! Explicit, owner-requested model installation. Never called during setup.
use crate::service::NodeService;
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROGRESS_LINE: usize = 64 * 1024;

pub async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_METADATA_BYTES as u64)
    {
        bail!("The model service response is too large")
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > MAX_METADATA_BYTES.saturating_sub(bytes.len()) {
            bail!("The model service response is too large")
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn installed_index(directory: &std::path::Path) -> Result<Vec<Value>> {
    use std::io::Read;
    let file = match std::fs::File::open(directory.join("installed.json")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_METADATA_BYTES {
        bail!("Installed-model index is too large")
    }
    serde_json::from_slice(&bytes)
        .context("The installed-model index is damaged; it was not overwritten")
}

fn storage(directory: &std::path::Path, backend: &str) -> Value {
    if backend == "ollama" {
        return json!({"scope":"ollama","free_bytes":null,"message":"Ollama manages its own storage. Its free space is not reported here; this node's free space is not an Ollama disk check."});
    }
    let free = directory
        .ancestors()
        .find(|path| path.is_dir())
        .and_then(|path| fs2::available_space(path).ok());
    json!({"scope":"node","directory":directory,"free_bytes":free,"message":"Free space on this node's model drive. Download size is checked before and during transfer."})
}

fn catalog(config: &Value) -> Value {
    config.get("model_catalog").cloned().unwrap_or_else(|| {
        serde_json::from_str(include_str!("../../../config/model-catalog.json"))
            .expect("model catalog")
    })
}
fn base(config: &Value) -> String {
    config
        .pointer("/models/ollama/base_url")
        .and_then(Value::as_str)
        .unwrap_or("http://127.0.0.1:11434")
        .trim_end_matches('/')
        .into()
}
async fn ollama_models(config: &Value) -> Result<Value> {
    bounded_json(
        reqwest::Client::new()
            .get(format!("{}/api/tags", base(config)))
            .timeout(Duration::from_secs(2))
            .send()
            .await?
            .error_for_status()?,
    )
    .await
}
pub async fn list(state: &NodeService) -> Result<Value> {
    let config = state.read_config()?;
    let ollama = ollama_models(&config).await.ok();
    let provider = config
        .pointer("/models/provider")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let backend = if provider == "ollama" || (provider == "auto" && ollama.is_some()) {
        "ollama"
    } else {
        "llamacpp"
    };
    let directory = state.model_directory();
    let mut installed = installed_index(&directory)?;
    for item in &mut installed {
        item["available"] = json!(
            item["path"]
                .as_str()
                .is_some_and(|path| std::path::Path::new(path).is_file())
        );
    }
    Ok(
        json!({"backend":backend,"ollama_available":ollama.is_some(),"ollama_models":ollama.as_ref().and_then(|v|v.get("models")).cloned().unwrap_or(json!([])),"installed":installed,"catalog":catalog(&config),"download":state.live.lock().unwrap()["download"],"directory":directory,"storage":storage(&directory,backend)}),
    )
}

/// Query metadata only when the owner inspects/selects an installed model.
/// This does not load model weights or infer capabilities from model names.
pub async fn inspect(state: &NodeService, request: Value) -> Result<Value> {
    let id = request["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() < 200)
        .context("Choose an installed model")?;
    let config = state.read_config()?;
    let installed = ollama_models(&config).await?;
    if !installed["models"]
        .as_array()
        .is_some_and(|models| models.iter().any(|m| m["name"] == id))
    {
        bail!("This model is no longer installed. Refresh the model list.")
    }
    let result = async {
        let response = reqwest::Client::new()
            .post(format!("{}/api/show", base(&config)))
            .json(&json!({"model":id}))
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        bounded_json(response).await
    }
    .await;
    Ok(match result {
        Ok(details) => normalize_capabilities(id, &details),
        Err(_) => {
            json!({"id":id,"capabilities_known":false,"capabilities":[],"modality":null,"message":"The model service did not provide capabilities. Choose the model type explicitly."})
        }
    })
}

fn normalize_capabilities(id: &str, details: &Value) -> Value {
    let reported = details["capabilities"].as_array();
    let has =
        |capability| reported.is_some_and(|items| items.iter().any(|item| item == capability));
    let vision = has("vision");
    let completion = has("completion");
    let capabilities: Vec<&str> = if vision {
        vec!["llm.infer", "vlm.analyze"]
    } else if completion {
        vec!["llm.infer"]
    } else {
        vec![]
    };
    json!({"id":id,"capabilities_known":reported.is_some(),"capabilities":capabilities,"modality":if vision {Some("vlm")} else if completion {Some("text")} else {None}})
}
pub async fn install(state: Arc<NodeService>, request: Value) -> Result<()> {
    let config = state.read_config()?;
    let selected = request["id"]
        .as_str()
        .context("Choose a model to install")?;
    let entry = catalog(&config)
        .as_array()
        .context("Model catalog must be a list")?
        .iter()
        .find(|v| v["id"].as_str() == Some(selected))
        .cloned();
    let available = ollama_models(&config).await.is_ok();
    let provider = config
        .pointer("/models/provider")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let backend = if provider == "ollama" || (provider == "auto" && available) {
        "ollama"
    } else {
        "llamacpp"
    };
    let entry = match entry {
        Some(entry) => entry,
        None if backend == "ollama"
            && !selected.is_empty()
            && selected.len() < 200
            && selected
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/_:.-".contains(c)) =>
        {
            json!({"id":selected,"name":selected,"ollama":selected})
        }
        _ => bail!(
            "Choose a model from the list. Custom downloads can be added to the catalog in Advanced."
        ),
    };
    {
        let mut live = state.live.lock().unwrap();
        if live["download"]["state"] == "downloading" {
            bail!("A model is already downloading. Wait or cancel it first.")
        }
        live["download"] = json!({"id":uuid::Uuid::new_v4().to_string(),"backend":backend,"state":"downloading","phase":"Starting","name":entry["name"],"completed":0,"total":if backend=="ollama" {Value::Null} else {entry["bytes"].clone()},"cancel_requested":false});
    }
    tokio::spawn(async move {
        let transfer = async {
            if backend == "ollama" {
                pull(&state, &config, &entry).await
            } else {
                download(&state, &entry).await
            }
        };
        let result = tokio::select! {
            biased;
            result = transfer => result,
            _ = async {loop {tokio::time::sleep(Duration::from_millis(250)).await;
                if state.live.lock().unwrap()["download"]["cancel_requested"] == true {break;}
            }} => Err(anyhow::anyhow!("Download cancelled. You can try again later. The model service may retain reusable download layers.")),
        };
        let mut live = state.live.lock().unwrap();
        match result {
            Ok(()) => live["download"]["state"] = json!("installed"),
            Err(error) => {
                if live["download"]["cancel_requested"] == true {
                    live["download"]["state"] = json!("cancelled");
                    live["download"]["message"] = json!(if backend == "ollama" {
                        "Cancelled this node's pull request. Ollama may retain reusable layers or continue a shared download for another client."
                    } else {
                        "Cancelled this download. Any completed model file is kept; you can try again."
                    });
                } else {
                    live["download"]["state"] = json!("error");
                    live["download"]["error"] = json!(format!("{error:#}"));
                }
            }
        }
    });
    Ok(())
}
pub fn cancel(state: &NodeService, id: Option<&str>) -> Result<bool> {
    let mut live = state.live.lock().unwrap();
    if live["download"]["state"] != "downloading" {
        return Ok(false);
    }
    if id.is_some_and(|id| live["download"]["id"].as_str() != Some(id)) {
        bail!("That download is no longer active. Refresh its progress before cancelling.")
    }
    live["download"]["cancel_requested"] = json!(true);
    Ok(true)
}

#[derive(Default)]
struct PullProgress {
    buffer: Vec<u8>,
    layers: BTreeMap<String, (u64, u64)>,
    success: bool,
    phase: String,
}
impl PullProgress {
    fn line(&mut self) -> Result<()> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            self.buffer.clear();
            return Ok(());
        }
        let value: Value = serde_json::from_slice(&self.buffer)?;
        self.buffer.clear();
        if let Some(error) = value["error"].as_str() {
            bail!("Model download failed: {error}")
        }
        if let Some(phase) = value["status"].as_str() {
            self.phase = phase.to_owned();
        }
        self.success = value["status"] == "success";
        if value.get("total").is_some() || value.get("completed").is_some() {
            let digest = value["digest"].as_str().unwrap_or("legacy-progress");
            if digest.len() > 256
                || (!self.layers.contains_key(digest) && self.layers.len() >= 4096)
            {
                bail!("The model service reported too many download layers")
            }
            let entry = self.layers.entry(digest.into()).or_default();
            if let Some(total) = value["total"].as_u64() {
                entry.1 = total;
            }
            if let Some(completed) = value["completed"].as_u64() {
                entry.0 = completed;
            }
            if entry.1 > 0 {
                entry.0 = entry.0.min(entry.1);
            }
        }
        Ok(())
    }
    fn feed(&mut self, chunk: &[u8]) -> Result<()> {
        for part in chunk.split_inclusive(|b| *b == b'\n') {
            if part.len() > MAX_PROGRESS_LINE.saturating_sub(self.buffer.len()) {
                bail!("The model service sent an oversized progress line")
            }
            self.buffer.extend_from_slice(part);
            if part.last() == Some(&b'\n') {
                self.line()?;
            }
        }
        Ok(())
    }
    fn counts(&self) -> Result<(u64, u64)> {
        self.layers
            .values()
            .try_fold((0u64, 0u64), |(done, total), (d, t)| {
                Ok((
                    done.checked_add(*d)
                        .context("Model progress size overflow")?,
                    total
                        .checked_add(*t)
                        .context("Model progress size overflow")?,
                ))
            })
    }
    fn finish(&mut self) -> Result<()> {
        if !self.buffer.is_empty() {
            self.line()?;
        }
        if !self.success {
            bail!(
                "The model service did not confirm installation. Check the connection and try again."
            )
        }
        Ok(())
    }
}
fn progress(state: &NodeService, completed: u64, total: u64) -> Result<()> {
    let mut live = state.live.lock().unwrap();
    if live["download"]["cancel_requested"].as_bool() == Some(true) {
        bail!("Download cancelled. You can install the model again later.")
    }
    live["download"]["completed"] = json!(completed);
    live["download"]["total"] = json!(total);
    Ok(())
}
async fn pull(state: &NodeService, config: &Value, entry: &Value) -> Result<()> {
    let model = entry["ollama"]
        .as_str()
        .context("This catalog entry has no Ollama model")?;
    let response = reqwest::Client::new()
        .post(format!("{}/api/pull", base(config)))
        .json(&json!({"model":model,"stream":true}))
        .timeout(Duration::from_secs(3600))
        .send()
        .await?
        .error_for_status()?;
    let mut stream = response.bytes_stream();
    let mut tracker = PullProgress::default();
    while let Some(chunk) = stream.next().await {
        tracker.feed(&chunk?)?;
        let (completed, total) = tracker.counts()?;
        progress(state, completed, total)?;
        state.live.lock().unwrap()["download"]["phase"] = json!(tracker.phase);
    }
    tracker.finish()?;
    let (completed, total) = tracker.counts()?;
    progress(state, completed, total)?;
    Ok(())
}
struct PartialFile(std::path::PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
async fn download(state: &NodeService, entry: &Value) -> Result<()> {
    let url = reqwest::Url::parse(
        entry["url"]
            .as_str()
            .context("Model download address is missing")?,
    )?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        bail!("Model downloads require HTTPS without embedded credentials")
    }
    let expected = entry["sha256"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .context("Model catalog needs a SHA-256 checksum")?;
    let bytes = entry["bytes"]
        .as_u64()
        .filter(|n| *n > 0)
        .context("Model catalog needs the download size")?;
    let directory = state.model_directory();
    tokio::fs::create_dir_all(&directory).await?;
    let mut installed = installed_index(&directory)?;
    let destination = directory.join(format!("{expected}.gguf"));
    if !destination.is_file() {
        ensure_space(fs2::available_space(&directory)?, bytes)?;
        let temp = PartialFile(directory.join(format!("{}.partial", uuid::Uuid::new_v4())));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp.0)
            .await?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() > 8 || attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        let mut stream = client
            .get(url)
            .timeout(Duration::from_secs(3600))
            .send()
            .await?
            .error_for_status()?
            .bytes_stream();
        let mut count = 0u64;
        let mut hash = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let next = count
                .checked_add(chunk.len() as u64)
                .context("Model size overflow")?;
            if next > bytes {
                bail!("Model download is larger than its catalog entry")
            }
            ensure_space(fs2::available_space(&directory)?, bytes - count)?;
            hash.update(&chunk);
            file.write_all(&chunk).await?;
            count = next;
            progress(state, count, bytes)?;
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        if count != bytes || hex::encode(hash.finalize()) != expected.to_ascii_lowercase() {
            bail!("Model verification failed. The incomplete download was removed; try again.")
        }
        tokio::fs::rename(&temp.0, &destination).await?;
    } else if crate::updater::sha256_file(&destination)? != expected.to_ascii_lowercase() {
        bail!(
            "The existing model file failed verification. Move it aside before downloading again."
        )
    }
    let installed_path = directory.join("installed.json");
    installed.retain(|v| v["id"] != entry["id"]);
    let mut item = entry.clone();
    item["path"] = json!(destination.canonicalize()?);
    installed.push(item);
    crate::setup::write_json(&installed_path, &json!(installed))?;
    Ok(())
}

fn ensure_space(available: u64, required: u64) -> Result<()> {
    if available < required {
        bail!("Not enough disk space for this model. Choose a smaller model or free some space.")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn existing_gguf_is_verified_reused_and_corruption_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let state = NodeService::new(dir.path().join("node.json"), "node".into());
        let directory = state.model_directory();
        std::fs::create_dir(&directory).unwrap();
        let bytes = b"small verification fixture, not a real model";
        let hash = hex::encode(Sha256::digest(bytes));
        let path = directory.join(format!("{hash}.gguf"));
        std::fs::write(&path, bytes).unwrap();
        let entry = json!({"id":"fixture","url":"https://example.invalid/not-requested","sha256":hash,"bytes":bytes.len()});
        download(&state, &entry).await.unwrap();
        assert_eq!(installed_index(&directory).unwrap().len(), 1);
        download(&state, &entry).await.unwrap();
        assert_eq!(installed_index(&directory).unwrap().len(), 1);
        std::fs::write(&path, b"corrupted fixture").unwrap();
        assert!(
            download(&state, &entry)
                .await
                .unwrap_err()
                .to_string()
                .contains("existing model file failed")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupted fixture");
        assert_eq!(installed_index(&directory).unwrap().len(), 1);
    }

    #[test]
    fn model_types_come_from_reported_capabilities_not_names() {
        assert_eq!(
            normalize_capabilities(
                "vision-looking-name",
                &json!({"capabilities":["completion"]})
            )["modality"],
            "text"
        );
        let vision = normalize_capabilities(
            "arbitrary",
            &json!({"capabilities":["completion","vision"]}),
        );
        assert_eq!(vision["modality"], "vlm");
        assert_eq!(vision["capabilities"], json!(["llm.infer", "vlm.analyze"]));
        assert!(normalize_capabilities("text-looking-name",&json!({"capabilities":["embedding"]}))["modality"].is_null());
        assert_eq!(
            normalize_capabilities("older-service", &json!({}))["capabilities_known"],
            false
        );
    }

    #[test]
    fn pull_tracks_layers_and_retains_counts_during_verification() {
        let mut progress = PullProgress::default();
        for value in [
            json!({"digest":"a","total":100,"completed":80}),
            json!({"digest":"b","total":50,"completed":20}),
            json!({"status":"verifying sha256 digest"}),
        ] {
            let bytes = format!("{value}\n");
            for chunk in bytes.as_bytes().chunks(3) {
                progress.feed(chunk).unwrap();
            }
        }
        assert_eq!(progress.counts().unwrap(), (100, 150));
        progress.feed(br#"{"status":"success"}"#).unwrap();
        progress.finish().unwrap();
        assert_eq!(progress.counts().unwrap(), (100, 150));
    }

    #[test]
    fn progress_rejects_oversize_errors_truncation_and_size_overflow() {
        let mut progress = PullProgress::default();
        assert!(progress.feed(&vec![b'x'; MAX_PROGRESS_LINE + 1]).is_err());
        assert!(progress.buffer.len() <= MAX_PROGRESS_LINE);
        let mut progress = PullProgress::default();
        progress.feed(br#"{"status":"pulling"}"#).unwrap();
        assert!(progress.finish().is_err());
        let mut progress = PullProgress::default();
        assert!(progress.feed(b"{\"error\":\"disk full\"}\n").is_err());
        let mut progress = PullProgress::default();
        progress.feed(b"{\"status\":").unwrap();
        assert!(progress.finish().is_err());
        let mut progress = PullProgress::default();
        progress.layers.insert("a".into(), (u64::MAX, u64::MAX));
        progress.layers.insert("b".into(), (1, 1));
        assert!(progress.counts().is_err());
    }

    #[test]
    fn cancellation_is_active_job_scoped_and_does_not_poison_next_job() {
        let dir = tempfile::tempdir().unwrap();
        let state = NodeService::new(dir.path().join("node.json"), "node".into());
        assert!(!cancel(&state, None).unwrap());
        state.live.lock().unwrap()["download"] =
            json!({"state":"downloading","id":"current","cancel_requested":false});
        assert!(cancel(&state, Some("old")).is_err());
        assert_eq!(
            state.live.lock().unwrap()["download"]["cancel_requested"],
            false
        );
        assert!(cancel(&state, Some("current")).unwrap());
        state.live.lock().unwrap()["download"] =
            json!({"state":"installed","cancel_requested":false});
        assert!(!cancel(&state, None).unwrap());
        assert_eq!(
            state.live.lock().unwrap()["download"]["cancel_requested"],
            false
        );
    }

    #[test]
    fn storage_is_backend_scoped_and_corrupt_index_is_not_silently_reset() {
        let dir = tempfile::tempdir().unwrap();
        assert!(storage(dir.path(), "ollama")["free_bytes"].is_null());
        assert!(storage(&dir.path().join("not-created"), "llamacpp")["free_bytes"].is_u64());
        assert!(ensure_space(99, 100).is_err());
        assert!(ensure_space(100, 100).is_ok());
        assert!(installed_index(dir.path()).unwrap().is_empty());
        std::fs::write(dir.path().join("installed.json"), "damaged").unwrap();
        assert!(installed_index(dir.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("installed.json")).unwrap(),
            "damaged"
        );
    }

    #[tokio::test]
    async fn model_service_json_is_bounded_with_and_without_content_length() {
        use axum::{Router, body::Body, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/length",
                get(|| async { "x".repeat(MAX_METADATA_BYTES + 1) }),
            )
            .route(
                "/chunked",
                get(|| async {
                    Body::from_stream(futures_util::stream::iter(
                        (0..65).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 65536])),
                    ))
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for path in ["length", "chunked"] {
            let response = reqwest::get(format!("http://{address}/{path}"))
                .await
                .unwrap();
            assert!(
                bounded_json(response)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("too large")
            );
        }
        server.abort();
    }
}
