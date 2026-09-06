//! Explicit, owner-requested model installation. Never called during setup.
use crate::dashboard::NodeDashboard;
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

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
    Ok(reqwest::Client::new()
        .get(format!("{}/api/tags", base(config)))
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
pub async fn list(state: &NodeDashboard) -> Result<Value> {
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
    let installed = std::fs::read(directory.join("installed.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or(json!([]));
    Ok(
        json!({"backend":backend,"ollama_available":ollama.is_some(),"ollama_models":ollama.as_ref().and_then(|v|v.get("models")).cloned().unwrap_or(json!([])),"installed":installed,"catalog":catalog(&config),"download":state.live.lock().unwrap()["download"],"directory":directory}),
    )
}
pub async fn install(state: Arc<NodeDashboard>, request: Value) -> Result<()> {
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
        live["download"] = json!({"state":"downloading","name":entry["name"],"completed":0,"total":entry["bytes"],"cancel_requested":false});
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
            result = transfer => result,
            _ = async {loop {tokio::time::sleep(Duration::from_millis(250)).await;
                if state.live.lock().unwrap()["download"]["cancel_requested"] == true {break;}
            }} => Err(anyhow::anyhow!("Download cancelled. You can try again later. The model service may retain reusable download layers.")),
        };
        let mut live = state.live.lock().unwrap();
        match result {
            Ok(()) => live["download"]["state"] = json!("installed"),
            Err(error) => {
                live["download"]["state"] = json!("error");
                live["download"]["error"] = json!(format!("{error:#}"));
            }
        }
    });
    Ok(())
}
fn progress(state: &NodeDashboard, completed: u64, total: u64) -> Result<()> {
    let mut live = state.live.lock().unwrap();
    if live["download"]["cancel_requested"].as_bool() == Some(true) {
        bail!("Download cancelled. You can install the model again later.")
    }
    live["download"]["completed"] = json!(completed);
    live["download"]["total"] = json!(total);
    Ok(())
}
async fn pull(state: &NodeDashboard, config: &Value, entry: &Value) -> Result<()> {
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
    let mut buffer = Vec::new();
    let mut success = false;
    while let Some(chunk) = stream.next().await {
        progress(state, 0, 0)?;
        buffer.extend_from_slice(&chunk?);
        if buffer.len() > 1024 * 1024 {
            bail!("The model service sent an invalid progress response")
        }
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = buffer.drain(..=end).collect();
            let value: Value = serde_json::from_slice(&line)?;
            if let Some(error) = value["error"].as_str() {
                bail!("Model download failed: {error}")
            }
            progress(
                state,
                value["completed"].as_u64().unwrap_or(0),
                value["total"].as_u64().unwrap_or(0),
            )?;
            success |= value["status"] == "success";
        }
    }
    if !success {
        bail!("The model service did not confirm installation. Check the connection and try again.")
    }
    Ok(())
}
struct PartialFile(std::path::PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
async fn download(state: &NodeDashboard, entry: &Value) -> Result<()> {
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
    if fs2::available_space(&directory)? < bytes {
        bail!("Not enough disk space for this model. Choose a smaller model or free some space.")
    }
    let destination = directory.join(format!("{expected}.gguf"));
    if !destination.is_file() {
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
            count += chunk.len() as u64;
            if count > bytes {
                bail!("Model download is larger than its catalog entry")
            }
            progress(state, count, bytes)?;
            hash.update(&chunk);
            file.write_all(&chunk).await?;
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
    let mut installed: Vec<Value> = std::fs::read(&installed_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    installed.retain(|v| v["id"] != entry["id"]);
    let mut item = entry.clone();
    item["path"] = json!(destination.canonicalize()?);
    installed.push(item);
    crate::setup::write_json(&installed_path, &json!(installed))?;
    Ok(())
}
