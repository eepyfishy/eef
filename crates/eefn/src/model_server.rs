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
    /// Requested generation/prompt worker count, not a CPU-percent or RAM quota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_threads: Option<u16>,
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
        validate_cpu_threads(self.cpu_threads)?;
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
        let mut value = json!({
            "model_id": self.model_id,
            "backend": "llamacpp",
            "hardware": if self.gpu_layers != 0 { "gpu" } else { "cpu" },
            "vram_mb": self.gpu_vram_mb,
            "port": self.port,
            "modality": modality,
            "model_metadata": self.selection_metadata()?,
        });
        if let Some(threads) = self.cpu_threads {
            value["cpu_threads"] = json!(threads);
        }
        Ok(value)
    }
}

pub(crate) fn validate_cpu_threads(threads: Option<u16>) -> Result<()> {
    if threads.is_some_and(|n| !(1..=256).contains(&n)) {
        bail!("CPU threads must be 1-256, or omitted for backend defaults")
    }
    Ok(())
}

fn configure_cpu_threads(command: &mut Command, threads: Option<u16>) -> Result<()> {
    validate_cpu_threads(threads)?;
    if let Some(threads) = threads {
        // Set both: inherited environment must not widen prompt worker counts.
        command.arg("--threads").arg(threads.to_string());
        command.arg("--threads-batch").arg(threads.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod advertisement_tests {
    use super::*;
    #[test]
    fn resident_child_helper() {
        if std::env::args().any(|s| s == "eef-owned-model-lifecycle-test") {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    fn resident_server() -> Arc<ModelServer> {
        let slot = serde_json::from_value(json!({"model_id":"owned-model"})).unwrap();
        let server = ModelServer::new(PathBuf::new(), vec![slot]);
        server.initialize_residency().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "model_server::advertisement_tests::resident_child_helper",
                "--skip",
                "eef-owned-model-lifecycle-test",
                "--test-threads=1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let (child, tree) = crate::process_lifetime::spawn(&mut command).unwrap();
        server
            .processes
            .lock()
            .unwrap()
            .insert("owned-model".into(), OwnedProcess { child, _tree: tree });
        server.set_all(ModelLifecycle::Ready);
        server
    }
    #[tokio::test]
    async fn inference_pins_block_unload_and_close_fences_old_pins_and_late_loads() {
        let server = resident_server();
        let one = server.pin(Some("owned-model")).unwrap();
        let two = server.pin(Some("owned-model")).unwrap();
        assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 2);
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "active"
        );
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(
            server
                .reserve_control("owned-model", generation, &id, false)
                .is_err()
        );
        drop(one);
        assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 1);
        drop(two);
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "warm_idle"
        );
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        assert!(server.pin(Some("owned-model")).is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "unavailable"
        );
        server.release_control("owned-model", &id);
        let old_pin = server.pin(Some("owned-model")).unwrap();
        server.close();
        drop(old_pin);
        assert!(!server.model_ready(Some("owned-model")));
        assert!(server.start(Duration::from_secs(1)).await.is_err());
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "unloaded"
        );
    }
    #[tokio::test]
    async fn unload_owns_exact_generation_and_never_reuses_an_old_reservation() {
        let server = resident_server();
        let id = uuid::Uuid::new_v4().to_string();
        let generation = server.snapshot().unwrap()[0]["generation"]
            .as_u64()
            .unwrap();
        assert!(
            server
                .reserve_control("owned-model", generation + 1, &id, false)
                .is_err()
        );
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        server
            .unload_reserved("owned-model", &id, || Ok(()))
            .await
            .unwrap();
        assert!(!server.model_ready(Some("owned-model")));
        let unloaded = server.snapshot().unwrap();
        assert_eq!(unloaded[0]["state"], "unloaded");
        assert!(unloaded[0]["owned_pid"].is_null());
        assert!(
            server
                .unload_reserved("owned-model", &id, || Ok(()))
                .await
                .is_err()
        );
        assert!(
            server
                .reserve_control(
                    "owned-model",
                    generation,
                    &uuid::Uuid::new_v4().to_string(),
                    true
                )
                .is_err()
        );
    }
    #[tokio::test]
    async fn unload_rechecks_authority_after_waiting_and_preserves_residency_on_denial() {
        let server = resident_server();
        let before = server.snapshot().unwrap();
        let generation = before[0]["generation"].as_u64().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        server
            .reserve_control("owned-model", generation, &id, false)
            .unwrap();
        let operation = server.operation.lock().await;
        let checked = std::sync::atomic::AtomicBool::new(false);
        let unload = server.unload_reserved("owned-model", &id, || {
            checked.store(true, std::sync::atomic::Ordering::SeqCst);
            Err::<(), _>(anyhow::anyhow!("owner authority revoked"))
        });
        tokio::pin!(unload);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut unload)
                .await
                .is_err()
        );
        assert!(!checked.load(std::sync::atomic::Ordering::SeqCst));
        drop(operation);
        assert!(unload.await.is_err());
        assert!(checked.load(std::sync::atomic::Ordering::SeqCst));
        server.release_control("owned-model", &id);
        assert_eq!(server.snapshot().unwrap(), before);
        assert!(server.pin(Some("owned-model")).is_ok());
        server.close();
    }
    #[tokio::test]
    async fn withdrawn_load_reservation_cannot_spawn_a_late_child() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = directory.path().join("synthetic.gguf");
        std::fs::write(&artifact, b"not real weights").unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"selected","model_path":artifact})).unwrap();
        let server = ModelServer::new(std::env::current_exe().unwrap(), vec![slot]);
        let id = uuid::Uuid::new_v4().to_string();
        server.reserve_control("selected", 0, &id, true).unwrap();
        let result = server
            .load_reserved("selected", &id, Duration::from_secs(1), |slot| {
                // An independently observed sibling failure withdraws group ownership.
                server.set_all(ModelLifecycle::Error);
                server.spawn_slot(slot, Some(&id))
            })
            .await;
        assert!(result.is_err());
        let rows = server.snapshot().unwrap();
        assert!(rows[0]["owned_pid"].is_null());
        assert_eq!(rows[0]["state"], "error");
        assert_eq!(rows[0]["control_pending"], false);
        server.close();
    }
    #[test]
    fn explicit_threads_cover_both_worker_pools_and_legacy_settings_stay_unknown() {
        for threads in [None, Some(1), Some(4), Some(256)] {
            let mut command = Command::new("test-only-not-spawned");
            configure_cpu_threads(&mut command, threads).unwrap();
            let args: Vec<_> = command
                .as_std()
                .get_args()
                .map(|v| v.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                args,
                threads
                    .map(|n| vec![
                        "--threads".into(),
                        n.to_string(),
                        "--threads-batch".into(),
                        n.to_string()
                    ])
                    .unwrap_or_default()
            );
            let slot: ModelSlot =
                serde_json::from_value(json!({"model_id":"legacy","cpu_threads":threads})).unwrap();
            assert_eq!(slot.advertise().unwrap()["cpu_threads"], json!(threads));
            assert!(
                slot.selection_metadata()
                    .unwrap()
                    .resource_estimates
                    .ram_mb
                    .is_none()
            );
        }
        for threads in [0, 257] {
            let mut command = Command::new("test-only-not-spawned");
            assert!(configure_cpu_threads(&mut command, Some(threads)).is_err());
            assert_eq!(command.as_std().get_args().count(), 0);
            let slot: ModelSlot =
                serde_json::from_value(json!({"model_id":"invalid","cpu_threads":threads}))
                    .unwrap();
            assert!(slot.selection_metadata().is_err());
        }
    }
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
        server.initialize_residency().unwrap();
        server.set_all(ModelLifecycle::Ready);
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
    processes: SyncMutex<HashMap<String, OwnedProcess>>,
    residency: SyncMutex<Residency>,
    operation: Mutex<()>,
    lifecycle: watch::Sender<ModelLifecycle>,
    client: reqwest::Client,
}

struct OwnedProcess {
    child: Child,
    _tree: crate::process_lifetime::ProcessTree,
}
#[derive(Clone)]
struct SlotResidency {
    state: ModelLifecycle,
    generation: u64,
    active: u32,
    control: Option<String>,
}
struct Residency {
    closed: bool,
    slots: HashMap<String, SlotResidency>,
}
/// Inference-only lifetime pin. A stale pin cannot reactivate a later generation.
pub(crate) struct ModelUse<'a> {
    server: &'a ModelServer,
    model: String,
    generation: u64,
}
impl Drop for ModelUse<'_> {
    fn drop(&mut self) {
        let mut gate = self.server.residency.lock().unwrap();
        if let Some(slot) = gate.slots.get_mut(&self.model) {
            if slot.generation == self.generation && slot.active > 0 {
                slot.active -= 1;
                if slot.active == 0 && slot.state == ModelLifecycle::Active {
                    slot.state = ModelLifecycle::WarmIdle;
                }
            }
        }
        drop(gate);
        self.server.changed();
    }
}

impl ModelServer {
    pub fn new(binary: PathBuf, slots: Vec<ModelSlot>) -> Arc<Self> {
        Arc::new(Self {
            host: "127.0.0.1".into(),
            binary,
            slots,
            processes: SyncMutex::new(HashMap::new()),
            residency: SyncMutex::new(Residency {
                closed: false,
                slots: HashMap::new(),
            }),
            operation: Mutex::new(()),
            lifecycle: watch::channel(ModelLifecycle::Unloaded).0,
            client: reqwest::Client::new(),
        })
    }

    fn initialize_residency(&self) -> Result<()> {
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed; no new load or inference")
        }
        if self.slots.len() > 256 {
            bail!("selected local model capacity exceeded")
        }
        if gate.slots.is_empty() {
            for slot in &self.slots {
                crate::model_selection::validate_id(&slot.model_id)?;
                if gate
                    .slots
                    .insert(
                        slot.model_id.clone(),
                        SlotResidency {
                            state: ModelLifecycle::Unloaded,
                            generation: 0,
                            active: 0,
                            control: None,
                        },
                    )
                    .is_some()
                {
                    bail!("duplicate model selection")
                }
            }
        }
        if gate.slots.len() != self.slots.len() {
            bail!("invalid model selection history")
        }
        Ok(())
    }
    fn changed(&self) {
        let gate = self.residency.lock().unwrap();
        let state = if gate.slots.values().any(|s| {
            matches!(
                s.state,
                ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
            )
        }) {
            ModelLifecycle::Ready
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::Loading)
        {
            ModelLifecycle::Loading
        } else if gate
            .slots
            .values()
            .any(|s| s.state == ModelLifecycle::Error)
        {
            ModelLifecycle::Error
        } else {
            ModelLifecycle::Unloaded
        };
        self.lifecycle.send_replace(state);
    }
    fn set_all(&self, state: ModelLifecycle) {
        let mut gate = self.residency.lock().unwrap();
        for slot in gate.slots.values_mut() {
            slot.state = state;
            slot.generation = slot.generation.saturating_add(1);
            slot.active = 0;
            slot.control = None;
        }
        self.lifecycle.send_replace(state);
    }
    pub(crate) fn snapshot(&self) -> Result<Vec<Value>> {
        self.initialize_residency()?;
        let gate = self.residency.lock().unwrap();
        let processes = self.processes.lock().unwrap();
        let mut records=self.slots.iter().map(|slot| {
            let s=&gate.slots[&slot.model_id];json!({"model_id":slot.model_id,"state":s.state,"generation":s.generation,"active_requests":s.active,
                "control_pending":s.control.is_some(),"owned_pid":processes.get(&slot.model_id).and_then(|p|p.child.id())})
        }).collect::<Vec<_>>();
        records.sort_by(|a, b| a["model_id"].as_str().cmp(&b["model_id"].as_str()));
        Ok(records)
    }
    pub fn model_ready(&self, model: Option<&str>) -> bool {
        if self.initialize_residency().is_err() {
            return false;
        }
        let Some(selected) = self.slot(model) else {
            return false;
        };
        let gate = self.residency.lock().unwrap();
        let ready = gate.slots.get(&selected.model_id).is_some_and(|s| {
            s.control.is_none()
                && matches!(
                    s.state,
                    ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
                )
        });
        ready
            && self
                .processes
                .lock()
                .unwrap()
                .get_mut(&selected.model_id)
                .is_some_and(|p| p.child.try_wait().is_ok_and(|status| status.is_none()))
    }
    pub(crate) fn pin(&self, model: Option<&str>) -> Result<ModelUse<'_>> {
        self.initialize_residency()?;
        let selected = self.slot(model).context("Selected local model missing")?;
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed")
        }
        let s = gate.slots.get_mut(&selected.model_id).unwrap();
        if s.control.is_some()
            || !matches!(
                s.state,
                ModelLifecycle::Ready | ModelLifecycle::Active | ModelLifecycle::WarmIdle
            )
        {
            bail!("model_not_ready: selected model not ready")
        }
        if !self
            .processes
            .lock()
            .unwrap()
            .get_mut(&selected.model_id)
            .is_some_and(|p| p.child.try_wait().is_ok_and(|status| status.is_none()))
        {
            bail!("model_not_ready: owned process missing/exited")
        }
        s.active = s
            .active
            .checked_add(1)
            .context("model pin capacity exceeded")?;
        s.state = ModelLifecycle::Active;
        let generation = s.generation;
        drop(gate);
        self.changed();
        Ok(ModelUse {
            server: self,
            model: selected.model_id.clone(),
            generation,
        })
    }
    pub(crate) fn reserve_control(
        &self,
        model: &str,
        generation: u64,
        id: &str,
        load: bool,
    ) -> Result<()> {
        self.initialize_residency()?;
        let mut gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed")
        }
        let s = gate.slots.get_mut(model).context("model not selected")?;
        if s.generation != generation
            || s.active != 0
            || s.control.is_some()
            || s.state == ModelLifecycle::Loading
        {
            bail!("model generation changed, busy or in use")
        }
        if load && !matches!(s.state, ModelLifecycle::Unloaded | ModelLifecycle::Error) {
            bail!("model already resident; inspect exact receipt")
        }
        if !load && !matches!(s.state, ModelLifecycle::Ready | ModelLifecycle::WarmIdle) {
            bail!("model not resident")
        }
        s.control = Some(id.into());
        drop(gate);
        self.changed();
        Ok(())
    }
    pub(crate) fn release_control(&self, model: &str, id: &str) {
        let mut gate = self.residency.lock().unwrap();
        if let Some(s) = gate.slots.get_mut(model) {
            if s.control.as_deref() == Some(id) {
                s.control = None;
            }
        }
        drop(gate);
        self.changed();
    }
    pub(crate) fn close(&self) {
        let mut gate = self.residency.lock().unwrap();
        gate.closed = true;
        for s in gate.slots.values_mut() {
            s.generation = s.generation.saturating_add(1);
            s.state = ModelLifecycle::Unloaded;
            s.active = 0;
            s.control = None;
        }
        self.processes.lock().unwrap().clear();
        drop(gate);
        self.changed();
    }

    pub async fn start(&self, health_timeout: Duration) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("model operation already running")?;
        self.initialize_residency()?;
        {
            let gate = self.residency.lock().unwrap();
            if gate
                .slots
                .values()
                .any(|s| s.active > 0 || s.control.is_some())
            {
                bail!("explicit model control/inference already owns startup")
            }
        }
        if *self.lifecycle.borrow() == ModelLifecycle::Ready {
            bail!("model group already started");
        }
        self.set_all(ModelLifecycle::Loading);
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
                self.set_all(ModelLifecycle::Ready);
            }
            Err(_) => {
                self.stop_children().await;
                self.set_all(ModelLifecycle::Error);
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
            self.spawn_slot(slot, None)
                .with_context(|| format!("spawn model '{}'", slot.model_id))?;
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
        self.set_all(ModelLifecycle::Unloaded);
        self.stop_children().await;
    }

    async fn stop_children(&self) {
        let mut processes = std::mem::take(&mut *self.processes.lock().unwrap());
        for process in processes.values_mut() {
            let _ = process.child.kill().await;
        }
    }

    fn check_children(&self) -> Result<()> {
        let gate = self.residency.lock().unwrap();
        let mut processes = self.processes.lock().unwrap();
        let expected = gate
            .slots
            .values()
            .filter(|s| {
                matches!(
                    s.state,
                    ModelLifecycle::Loading
                        | ModelLifecycle::Ready
                        | ModelLifecycle::Active
                        | ModelLifecycle::WarmIdle
                )
            })
            .count();
        if processes.len() != expected {
            bail!("model group does not own every configured process");
        }
        for process in processes.values_mut() {
            if process.child.try_wait()?.is_some() {
                bail!("model process exited");
            }
        }
        Ok(())
    }
    pub(crate) async fn load_reserved(
        &self,
        model: &str,
        id: &str,
        timeout: Duration,
        spawn_authorized: impl FnOnce(&ModelSlot) -> Result<()>,
    ) -> Result<()> {
        let _operation = self.operation.lock().await;
        let slot = self.slot(Some(model)).context("selected model missing")?;
        {
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed")
            }
            let s = gate.slots.get_mut(model).context("model state missing")?;
            if s.control.as_deref() != Some(id)
                || s.active != 0
                || !matches!(s.state, ModelLifecycle::Unloaded | ModelLifecycle::Error)
            {
                bail!("reserved model state changed")
            }
            let generation = s
                .generation
                .checked_add(1)
                .context("model generation exhausted")?;
            s.state = ModelLifecycle::Loading;
            s.generation = generation;
        }
        self.changed();
        let mut cleanup = SlotStartupCleanup {
            server: self,
            model: model.into(),
            id: id.into(),
            armed: true,
        };
        let outcome = tokio::time::timeout(timeout, async {
            spawn_authorized(slot)?;
            loop {
                {
                    let gate = self.residency.lock().unwrap();
                    if gate.closed {
                        bail!("model runtime closed")
                    }
                    let mut processes = self.processes.lock().unwrap();
                    let p = processes.get_mut(model).context("owned model missing")?;
                    if p.child.try_wait()?.is_some() {
                        bail!("owned model exited during load")
                    }
                }
                if self.healthy(slot).await {
                    break;
                }
                sleep(Duration::from_millis(250)).await;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("model load timed out")
        .and_then(|r| r);
        if outcome.is_ok() {
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed before load confirmation")
            }
            let s = gate.slots.get_mut(model).unwrap();
            if s.control.as_deref() != Some(id) {
                bail!("model load ownership changed")
            }
            s.state = ModelLifecycle::Ready;
            s.control = None;
            cleanup.armed = false;
            drop(gate);
            self.changed();
        }
        outcome
    }
    pub(crate) async fn unload_reserved<G>(
        &self,
        model: &str,
        id: &str,
        authorize: impl FnOnce() -> Result<G>,
    ) -> Result<()> {
        let _operation = self.operation.lock().await;
        let process = {
            // Acquire current authority only after asynchronous operation admission.
            // Retain its guard through withdrawal, never across the child wait.
            let _authority = authorize()?;
            let mut gate = self.residency.lock().unwrap();
            if gate.closed {
                bail!("model runtime closed")
            }
            let s = gate.slots.get_mut(model).context("model state missing")?;
            if s.control.as_deref() != Some(id)
                || s.active != 0
                || !matches!(s.state, ModelLifecycle::Ready | ModelLifecycle::WarmIdle)
            {
                bail!("reserved model unload state changed")
            }
            let generation = s
                .generation
                .checked_add(1)
                .context("model generation exhausted")?;
            s.state = ModelLifecycle::Unloaded;
            s.generation = generation;
            s.control = None;
            self.processes.lock().unwrap().remove(model)
        };
        self.changed();
        if let Some(mut process) = process {
            // The OS job handle remains owned until this bounded root wait ends.
            tokio::time::timeout(Duration::from_secs(5), async {
                process.child.kill().await?;
                process.child.wait().await?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .context("owned model termination wait timed out")??;
        }
        Ok(())
    }
    pub(crate) fn spawn_slot(&self, slot: &ModelSlot, control: Option<&str>) -> Result<()> {
        slot.selection_metadata()?;
        if !self.binary.is_file() || !slot.model_path.is_file() {
            bail!("configured model runtime/artifact unavailable")
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
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(mmproj) = &slot.mmproj_path {
            if !mmproj.is_file() {
                bail!("configured projector unavailable")
            }
            command.arg("--mmproj").arg(mmproj);
        }
        configure_cpu_threads(&mut command, slot.cpu_threads)?;
        command.arg("-ngl").arg(slot.gpu_layers.to_string());
        let gate = self.residency.lock().unwrap();
        if gate.closed {
            bail!("model runtime closed before spawn")
        }
        let state = gate
            .slots
            .get(&slot.model_id)
            .context("model state missing before spawn")?;
        if state.state != ModelLifecycle::Loading || state.control.as_deref() != control {
            bail!("model load ownership withdrawn before spawn")
        }
        let (child, tree) = crate::process_lifetime::spawn(&mut command)?;
        let mut owned = self.processes.lock().unwrap();
        if owned.contains_key(&slot.model_id) {
            bail!("owned model already present")
        }
        owned.insert(slot.model_id.clone(), OwnedProcess { child, _tree: tree });
        Ok(())
    }

    /// Observe owned process exits after startup, without restarting or replaying
    /// work. This does not probe ongoing HTTP health or prove inference works.
    /// The caller owns this future as part of the runtime, never a detached task.
    pub async fn monitor_processes(&self) -> Result<()> {
        loop {
            {
                if *self.lifecycle.borrow() != ModelLifecycle::Ready {
                    return Ok(());
                }
                let outcome = {
                    let gate = self.residency.lock().unwrap();
                    let mut owned = self.processes.lock().unwrap();
                    gate.slots
                        .iter()
                        .filter(|(_, s)| {
                            matches!(
                                s.state,
                                ModelLifecycle::Ready
                                    | ModelLifecycle::Active
                                    | ModelLifecycle::WarmIdle
                            )
                        })
                        .try_for_each(|(id, _)| -> Result<()> {
                            let p = owned
                                .get_mut(id)
                                .context("ready model has no owned process")?;
                            if p.child.try_wait()?.is_some() {
                                bail!("ready model process exited")
                            };
                            Ok(())
                        })
                };
                if let Err(error) = outcome {
                    // Withdraw routing before asynchronous sibling cleanup.
                    self.set_all(ModelLifecycle::Error);
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
        // Invalid selections remain unroutable; do not poison node control.
        let _ = self.initialize_residency();
        let gate = self.residency.lock().unwrap();
        self.slots
            .iter()
            // Invalid optional selection hints fail startup, but must never
            // poison the node's registration snapshot or command connection.
            .filter(|slot| slot.selection_metadata().is_ok())
            .map(|slot| {
                let mut advertised = slot.advertise()?;
                let mut metadata = slot.selection_metadata()?;
                let (state, busy) = gate
                    .slots
                    .get(&slot.model_id)
                    .map(|s| (s.state, s.control.is_some() || gate.closed))
                    .unwrap_or((ModelLifecycle::Error, true));
                metadata.lifecycle = Some(state);
                metadata.availability = Some(
                    if !busy
                        && matches!(
                            state,
                            ModelLifecycle::Ready
                                | ModelLifecycle::Active
                                | ModelLifecycle::WarmIdle
                        )
                    {
                        ModelAvailability::Available
                    } else {
                        ModelAvailability::Unavailable
                    },
                );
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
struct SlotStartupCleanup<'a> {
    server: &'a ModelServer,
    model: String,
    id: String,
    armed: bool,
}
impl Drop for SlotStartupCleanup<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut gate = self.server.residency.lock().unwrap();
        self.server.processes.lock().unwrap().remove(&self.model);
        if let Some(s) = gate.slots.get_mut(&self.model) {
            if s.control.as_deref() == Some(&self.id) {
                s.state = ModelLifecycle::Error;
                s.control = None;
                s.generation = s.generation.saturating_add(1);
            }
        }
        drop(gate);
        self.server.changed();
    }
}
impl Drop for StartupCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Child::kill_on_drop is enabled. This is synchronous so cancellation
            // cannot leave a detached task spawning children after a restart.
            self.server.processes.lock().unwrap().clear();
            self.server.set_all(ModelLifecycle::Unloaded);
        }
    }
}
