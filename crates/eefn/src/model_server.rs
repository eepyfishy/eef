use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;

use crate::model_metadata::{ModelAvailability, ModelLifecycle};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, watch};
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
    #[serde(
        default,
        rename = "selection",
        skip_serializing_if = "crate::model_selection::ModelHints::is_empty"
    )]
    pub hints: crate::model_selection::ModelHints,
}

const fn default_port() -> u16 {
    8082
}
const fn default_context() -> u32 {
    4096
}

impl ModelSlot {
    pub fn selection_metadata(&self) -> Result<crate::model_metadata::ModelMetadata> {
        self.hints
            .metadata(if self.is_vlm() { "vlm" } else { "text" })
    }
    pub fn is_vlm(&self) -> bool {
        self.mmproj_path
            .as_ref()
            .is_some_and(|path| !path.as_os_str().is_empty())
    }

    pub fn advertise(&self) -> Result<Value> {
        let modality = if self.is_vlm() { "vlm" } else { "text" };
        Ok(json!({
            "model_id": self.model_id,
            "backend": "llamacpp",
            "hardware": if self.gpu_layers != 0 { "gpu" } else { "cpu" },
            "vram_mb": self.gpu_vram_mb,
            "port": self.port,
            "modality": modality,
            "model_metadata": self.selection_metadata()?,
        }))
    }
}

#[cfg(test)]
mod advertisement_tests {
    use super::*;
    #[tokio::test]
    async fn failed_group_is_visible_but_never_routable() {
        let fixture = tempfile::tempdir().unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"fixture","model_path":"PRIVATE"})).unwrap();
        let server = ModelServer::new(fixture.path().join("missing.exe"), vec![slot]);
        let mut changes = server.subscribe();
        assert!(server.require_ready().is_err());
        assert!(server.start(Duration::from_secs(1)).await.is_err());
        assert_eq!(*changes.borrow_and_update(), ModelLifecycle::Error);
        let models = server.models().unwrap();
        assert!(!models[0].to_string().contains("PRIVATE"));
        let metadata: crate::model_metadata::ModelMetadata =
            serde_json::from_value(models[0]["model_metadata"].clone()).unwrap();
        assert!(!metadata.routable());
        assert!(server.require_ready().is_err());
        assert!(server.processes.lock().unwrap().is_empty());
        server.stop().await;
        assert_eq!(*changes.borrow(), ModelLifecycle::Unloaded);
        let invalid = serde_json::from_value(json!({
            "model_id":"invalid-hints", "selection":{"roles":["not a valid role"]}
        }))
        .unwrap();
        let invalid_server = ModelServer::new(fixture.path().join("missing.exe"), vec![invalid]);
        assert!(invalid_server.models().unwrap().is_empty());
        assert!(invalid_server.start(Duration::from_secs(1)).await.is_err());
        assert!(invalid_server.models().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelled_startup_clears_state_even_with_retained_server() {
        let server = ModelServer::new(PathBuf::new(), vec![]);
        server.lifecycle.send_replace(ModelLifecycle::Loading);
        {
            let _cleanup = StartupCleanup {
                server: &server,
                armed: true,
            };
        }
        assert_eq!(*server.subscribe().borrow(), ModelLifecycle::Unloaded);
        assert!(server.require_ready().is_err());
    }

    #[tokio::test]
    async fn missing_owned_process_withdraws_ready_group() {
        let slot = serde_json::from_value(json!({"model_id":"missing-process"})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        server.lifecycle.send_replace(ModelLifecycle::Ready);
        let mut changes = server.subscribe();
        assert!(server.monitor_processes().await.is_err());
        assert!(changes.has_changed().unwrap());
        assert_eq!(*changes.borrow_and_update(), ModelLifecycle::Error);
        assert!(server.require_ready().is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "unavailable"
        );
        assert!(server.processes.lock().unwrap().is_empty());
        // No automatic restart/retry and no replacement of a terminal error.
        server.monitor_processes().await.unwrap();
        assert_eq!(*changes.borrow(), ModelLifecycle::Error);
    }

    #[tokio::test]
    async fn process_monitor_does_not_change_non_ready_states_or_block_stop() {
        let server = ModelServer::new(PathBuf::new(), vec![]);
        for state in [
            ModelLifecycle::Unloaded,
            ModelLifecycle::Loading,
            ModelLifecycle::Error,
        ] {
            server.lifecycle.send_replace(state);
            server.monitor_processes().await.unwrap();
            assert_eq!(*server.subscribe().borrow(), state);
        }
        server.lifecycle.send_replace(ModelLifecycle::Ready);
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(server.monitor_processes(), async {
                sleep(Duration::from_millis(20)).await;
                server.stop().await;
            })
            .0
            .unwrap();
        })
        .await
        .unwrap();
        assert_eq!(*server.subscribe().borrow(), ModelLifecycle::Unloaded);
    }

    #[test]
    fn gguf_advertises_compatible_metadata_without_private_paths_or_fake_state() {
        let mut slot: ModelSlot =
            serde_json::from_value(json!({"model_id":"fixture", "model_path":"PRIVATE"})).unwrap();
        for vision in [false, true] {
            slot.mmproj_path = vision.then(|| PathBuf::from("PRIVATE-PROJECTOR"));
            let value = slot.advertise().unwrap();
            assert!(!value.to_string().contains("PRIVATE"));
            let metadata: crate::model_metadata::ModelMetadata =
                serde_json::from_value(value["model_metadata"].clone()).unwrap();
            metadata.validate().unwrap();
            assert!(metadata.supports("llm.infer"));
            assert_eq!(metadata.supports("vlm.analyze"), vision);
            assert!(metadata.lifecycle.is_none());
            assert!(metadata.resource_estimates.vram_mb.is_none());
            assert_eq!(value["modality"], if vision { "vlm" } else { "text" });
        }
    }
}

pub struct ModelServer {
    pub host: String,
    pub binary: PathBuf,
    pub slots: Vec<ModelSlot>,
    processes: SyncMutex<HashMap<String, Child>>,
    operation: Mutex<()>,
    lifecycle: watch::Sender<ModelLifecycle>,
    client: reqwest::Client,
}

impl ModelServer {
    pub fn new(binary: PathBuf, slots: Vec<ModelSlot>) -> Arc<Self> {
        Arc::new(Self {
            host: "127.0.0.1".into(),
            binary,
            slots,
            processes: SyncMutex::new(HashMap::new()),
            operation: Mutex::new(()),
            lifecycle: watch::channel(ModelLifecycle::Unloaded).0,
            client: reqwest::Client::new(),
        })
    }

    pub async fn start(&self, health_timeout: Duration) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("model operation already running")?;
        if *self.lifecycle.borrow() == ModelLifecycle::Ready {
            bail!("model group already started");
        }
        self.lifecycle.send_replace(ModelLifecycle::Loading);
        // Dropping a startup future (restart/shutdown) must kill its children,
        // even if another Arc still references this server.
        let mut cleanup = StartupCleanup {
            server: self,
            armed: true,
        };
        let result = tokio::time::timeout(health_timeout, self.start_inner())
            .await
            .context("model startup timed out")
            .and_then(|result| result);
        match &result {
            Ok(()) => {
                self.lifecycle.send_replace(ModelLifecycle::Ready);
            }
            Err(_) => {
                self.stop_children().await;
                self.lifecycle.send_replace(ModelLifecycle::Error);
            }
        }
        cleanup.armed = false;
        result
    }

    async fn start_inner(&self) -> Result<()> {
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
            slot.selection_metadata()?;
            if slot.model_path.as_os_str().is_empty() || !slot.model_path.is_file() {
                bail!(
                    "GGUF for '{}' does not exist: {}",
                    slot.model_id,
                    slot.model_path.display()
                );
            }
            let mut command = Command::new(&self.binary);
            #[cfg(windows)]
            command.creation_flags(0x0800_0000);
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
                .unwrap()
                .insert(slot.model_id.clone(), child);
        }
        for slot in &self.slots {
            loop {
                self.check_children()?;
                if self.healthy(slot).await {
                    break;
                }
                sleep(Duration::from_millis(250)).await;
            }
        }
        self.check_children()?;
        Ok(())
    }

    pub async fn stop(&self) {
        let _operation = self.operation.lock().await;
        self.lifecycle.send_replace(ModelLifecycle::Unloaded);
        self.stop_children().await;
    }

    async fn stop_children(&self) {
        let mut processes = std::mem::take(&mut *self.processes.lock().unwrap());
        for child in processes.values_mut() {
            let _ = child.kill().await;
        }
    }

    fn check_children(&self) -> Result<()> {
        let mut processes = self.processes.lock().unwrap();
        if processes.len() != self.slots.len() {
            bail!("model group does not own every configured process");
        }
        for child in processes.values_mut() {
            if child.try_wait()?.is_some() {
                bail!("model process exited");
            }
        }
        Ok(())
    }

    /// Observe owned process exits after startup, without restarting or replaying
    /// work. This does not probe ongoing HTTP health or prove inference works.
    /// The caller owns this future as part of the runtime, never a detached task.
    pub async fn monitor_processes(&self) -> Result<()> {
        loop {
            {
                let _operation = self.operation.lock().await;
                if *self.lifecycle.borrow() != ModelLifecycle::Ready {
                    return Ok(());
                }
                if let Err(error) = self.check_children() {
                    // Withdraw routing before asynchronous sibling cleanup.
                    self.lifecycle.send_replace(ModelLifecycle::Error);
                    self.stop_children().await;
                    return Err(error);
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<ModelLifecycle> {
        self.lifecycle.subscribe()
    }

    pub fn require_ready(&self) -> Result<()> {
        if *self.lifecycle.borrow() != ModelLifecycle::Ready {
            bail!("model_not_ready: local model group is not ready; inspect model status");
        }
        Ok(())
    }

    pub fn models(&self) -> Result<Vec<Value>> {
        let state = *self.lifecycle.borrow();
        self.slots
            .iter()
            // Invalid optional selection hints fail startup, but must never
            // poison the node's registration snapshot or command connection.
            .filter(|slot| slot.selection_metadata().is_ok())
            .map(|slot| {
                let mut advertised = slot.advertise()?;
                let mut metadata = slot.selection_metadata()?;
                metadata.lifecycle = Some(state);
                metadata.availability = Some(if state == ModelLifecycle::Ready {
                    ModelAvailability::Available
                } else {
                    ModelAvailability::Unavailable
                });
                advertised["model_metadata"] = json!(metadata);
                Ok(advertised)
            })
            .collect()
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

struct StartupCleanup<'a> {
    server: &'a ModelServer,
    armed: bool,
}
impl Drop for StartupCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Child::kill_on_drop is enabled. This is synchronous so cancellation
            // cannot leave a detached task spawning children after a restart.
            self.server.processes.lock().unwrap().clear();
            self.server.lifecycle.send_replace(ModelLifecycle::Unloaded);
        }
    }
}
