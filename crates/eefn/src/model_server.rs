use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelSlot {
    pub model_id: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub model_path: PathBuf,
    #[serde(default)]
    pub mmproj_path: Option<PathBuf>,
    #[serde(default)]
    pub gpu_layers: i32,
    #[serde(default = "default_context")]
    pub context: u32,
    #[serde(default)]
    pub gpu_vram_mb: u64,
}

const fn default_port() -> u16 {
    8082
}
const fn default_context() -> u32 {
    4096
}

impl ModelSlot {
    pub fn is_vlm(&self) -> bool {
        self.mmproj_path
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty())
    }

    pub fn advertise(&self) -> Value {
        json!({
            "model_id": self.model_id,
            "backend": "llamacpp",
            "hardware": if self.gpu_layers != 0 { "gpu" } else { "cpu" },
            "vram_mb": self.gpu_vram_mb,
            "port": self.port,
            "modality": if self.is_vlm() { "vlm" } else { "text" },
        })
    }
}

pub struct ModelServer {
    pub host: String,
    pub binary: PathBuf,
    pub slots: Vec<ModelSlot>,
    processes: Mutex<HashMap<String, Child>>,
    client: reqwest::Client,
}

impl ModelServer {
    pub fn new(binary: PathBuf, slots: Vec<ModelSlot>) -> Arc<Self> {
        Arc::new(Self {
            host: "127.0.0.1".into(),
            binary,
            slots,
            processes: Mutex::new(HashMap::new()),
            client: reqwest::Client::new(),
        })
    }

    pub async fn start(&self, health_timeout: Duration) -> Result<()> {
        if self.slots.is_empty() {
            return Ok(());
        }
        if !self.binary.is_file() {
            bail!(
                "llama-server binary '{}' does not exist",
                self.binary.display()
            );
        }
        for slot in &self.slots {
            if slot.model_path.as_os_str().is_empty() || !slot.model_path.is_file() {
                bail!(
                    "GGUF for '{}' does not exist: {}",
                    slot.model_id,
                    slot.model_path.display()
                );
            }
            let mut command = Command::new(&self.binary);
            command
                .arg("--model")
                .arg(&slot.model_path)
                .arg("--host")
                .arg(&self.host)
                .arg("--port")
                .arg(slot.port.to_string())
                .arg("-c")
                .arg(slot.context.to_string())
                .arg("--alias")
                .arg(&slot.model_id)
                .kill_on_drop(true)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            if let Some(mmproj) = &slot.mmproj_path {
                command.arg("--mmproj").arg(mmproj);
            }
            if slot.gpu_layers != 0 {
                command.arg("-ngl").arg(slot.gpu_layers.to_string());
            }
            let child = command
                .spawn()
                .with_context(|| format!("spawn model '{}'", slot.model_id))?;
            self.processes
                .lock()
                .await
                .insert(slot.model_id.clone(), child);
        }
        let deadline = Instant::now() + health_timeout;
        for slot in &self.slots {
            while Instant::now() < deadline && !self.healthy(slot).await {
                sleep(Duration::from_millis(250)).await;
            }
            if !self.healthy(slot).await {
                self.stop().await;
                bail!("model '{}' did not become healthy", slot.model_id);
            }
        }
        Ok(())
    }

    pub async fn stop(&self) {
        let mut processes = self.processes.lock().await;
        for child in processes.values_mut() {
            let _ = child.kill().await;
        }
        processes.clear();
    }

    pub fn models(&self) -> Vec<Value> {
        self.slots.iter().map(ModelSlot::advertise).collect()
    }

    pub fn slot(&self, model_id: Option<&str>) -> Option<&ModelSlot> {
        match model_id {
            Some(id) => self.slots.iter().find(|slot| slot.model_id == id),
            None => self.slots.first(),
        }
    }

    pub fn base_url(&self, model_id: Option<&str>) -> Option<String> {
        self.slot(model_id)
            .map(|slot| format!("http://{}:{}", self.host, slot.port))
    }

    async fn healthy(&self, slot: &ModelSlot) -> bool {
        self.client
            .get(format!("http://{}:{}/health", self.host, slot.port))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }
}
