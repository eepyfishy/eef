//! Runtime-owned selected-GGUF lifecycle controls, not workload authorization.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleCommand {
    /// Inspect current selected GGUF state or one retained runtime receipt.
    Show {
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Explicitly load an applied selection; never download/select a model or infer.
    Load {
        #[arg(long)]
        model: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        operation_id: String,
    },
    /// Unload only when no EEF-owned inference pin is active; no forced stop.
    Unload {
        #[arg(long)]
        model: String,
        #[arg(long)]
        generation: u64,
        #[arg(long)]
        operation_id: String,
    },
    /// Cancel this exact queued/running load; never cancel inference or another receipt.
    Cancel {
        #[arg(long)]
        operation_id: String,
    },
}
#[derive(Debug, clap::Args)]
pub struct LifecycleArgs {
    /// Exact runtime from Show. Required for mutation; never refreshed implicitly.
    #[arg(long, global = true)]
    pub runtime: Option<String>,
    #[command(subcommand)]
    pub command: LifecycleCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: Option<String>,
    pub command: LifecycleCommand,
}
impl LifecycleRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported model lifecycle schema")
        }
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        if !matches!(self.command, LifecycleCommand::Show { .. })
            && self.expected_runtime_id.is_none()
        {
            bail!("Exact runtime required for model lifecycle mutation")
        }
        if let Some(id) = &self.expected_runtime_id {
            crate::automation_occurrence::uuid(id)?;
        }
        match &self.command {
            LifecycleCommand::Load {
                model,
                operation_id,
                ..
            }
            | LifecycleCommand::Unload {
                model,
                operation_id,
                ..
            } => {
                crate::model_selection::validate_id(model)?;
                crate::automation_occurrence::uuid(operation_id)?;
            }
            LifecycleCommand::Cancel { operation_id } => {
                crate::automation_occurrence::uuid(operation_id)?
            }
            LifecycleCommand::Show { operation_id } => {
                if let Some(id) = operation_id {
                    crate::automation_occurrence::uuid(id)?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    command: LifecycleCommand,
    phase: String,
    error_code: Option<String>,
}
struct Operation {
    receipt: Receipt,
    cancel: watch::Sender<bool>,
    remote: bool,
}
pub(crate) struct ModelRuntime {
    id: String,
    engine: Weak<crate::NodeEngine>,
    service: Weak<crate::NodeService>,
    server: Option<Arc<crate::ModelServer>>,
    models: Value,
    closed: AtomicBool,
    records: Mutex<BTreeMap<String, Operation>>,
    queue: mpsc::Sender<String>,
    receiver: tokio::sync::Mutex<Option<mpsc::Receiver<String>>>,
}
pub struct ModelRuntimeGuard {
    service: Weak<crate::NodeService>,
    runtime: Arc<ModelRuntime>,
}
impl Drop for ModelRuntimeGuard {
    fn drop(&mut self) {
        self.runtime.close();
        if let Some(service) = self.service.upgrade() {
            let mut current = service.model_runtime.lock().unwrap();
            if current.as_ref().is_some_and(|r| r.id == self.runtime.id) {
                *current = None;
            }
        }
    }
}
impl crate::NodeService {
    pub fn bind_model_runtime(
        self: &Arc<Self>,
        engine: &Arc<crate::NodeEngine>,
        id: String,
    ) -> Result<ModelRuntimeGuard> {
        crate::automation_occurrence::uuid(&id)?;
        let (queue, receiver) = mpsc::channel(8);
        let runtime = Arc::new(ModelRuntime {
            id,
            engine: Arc::downgrade(engine),
            service: Arc::downgrade(self),
            server: engine.model_server.clone(),
            models: self.read_config()?["models"].clone(),
            closed: AtomicBool::new(false),
            records: Mutex::new(BTreeMap::new()),
            queue,
            receiver: tokio::sync::Mutex::new(Some(receiver)),
        });
        let mut current = self.model_runtime.lock().unwrap();
        if let Some(old) = current.take() {
            old.close();
        }
        *current = Some(runtime.clone());
        Ok(ModelRuntimeGuard {
            service: Arc::downgrade(self),
            runtime,
        })
    }
    pub async fn model_lifecycle_command(
        &self,
        request: LifecycleRequest,
        remote: bool,
    ) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Model lifecycle node identity changed")
        }
        let runtime = self
            .model_runtime
            .lock()
            .unwrap()
            .clone()
            .context("Model lifecycle runtime unavailable")?;
        if request
            .expected_runtime_id
            .as_ref()
            .is_some_and(|id| *id != runtime.id)
            || runtime.closed.load(Ordering::SeqCst)
        {
            bail!("Model lifecycle runtime changed; no automatic retry")
        }
        let mut reused = false;
        let selected = match &request.command {
            LifecycleCommand::Show { operation_id } => operation_id.clone(),
            LifecycleCommand::Cancel { operation_id } => {
                let mut records = runtime.records.lock().unwrap();
                let old = records
                    .get_mut(operation_id)
                    .context("Model lifecycle receipt unavailable")?;
                if !matches!(old.receipt.command, LifecycleCommand::Load { .. }) {
                    bail!("Only an exact load can be cancelled")
                }
                if matches!(old.receipt.phase.as_str(), "queued" | "running") {
                    self.inspect_grant_configuration(|config| {
                        if remote
                            && config.pointer("/management/allow_remote") != Some(&json!(true))
                        {
                            bail!(
                                "Model lifecycle requires current local owner management approval"
                            )
                        }
                        old.cancel.send_replace(true);
                        old.receipt.phase = "cancel_requested".into();
                        Ok(())
                    })?;
                } else {
                    reused = true;
                }
                Some(operation_id.clone())
            }
            LifecycleCommand::Load {
                model,
                generation,
                operation_id,
            }
            | LifecycleCommand::Unload {
                model,
                generation,
                operation_id,
            } => {
                let mut records = runtime.records.lock().unwrap();
                if let Some(old) = records.get(operation_id) {
                    if old.receipt.command != request.command {
                        bail!("Model lifecycle receipt conflicts; inspect original")
                    }
                    reused = true;
                } else {
                    if records.len() >= 256 {
                        bail!("Model lifecycle receipt capacity reached; no admission")
                    }
                    let server = runtime.server.as_ref().context("Owned GGUF lifecycle unavailable for this active backend; no fallback or external unload")?;
                    self.inspect_grant_configuration(|config| {
                        if runtime.closed.load(Ordering::SeqCst) || runtime.engine.upgrade().is_none() {
                            bail!("Model runtime closed")
                        }
                        if config["node_id"] != self.node_id || config["models"] != runtime.models {
                            bail!("Saved model settings changed; explicitly restart before lifecycle changes")
                        }
                        if remote && config.pointer("/management/allow_remote") != Some(&json!(true)) {
                            bail!("Model lifecycle requires current local owner management approval")
                        }
                        server.reserve_control(model, *generation, operation_id, matches!(request.command, LifecycleCommand::Load { .. }))?;
                        if let Err(error) = runtime.queue.try_send(operation_id.clone()) {
                            server.release_control(model, operation_id);
                            return Err(error.into());
                        }
                        records.insert(operation_id.clone(), Operation {
                            receipt: Receipt { command: request.command.clone(), phase: "queued".into(), error_code: None },
                            cancel: watch::channel(false).0,
                            remote,
                        });
                        Ok(())
                    })?;
                }
                Some(operation_id.clone())
            }
        };
        let receipt = selected
            .as_ref()
            .map(|id| {
                runtime
                    .records
                    .lock()
                    .unwrap()
                    .get(id)
                    .map(|r| r.receipt.clone())
                    .context("Receipt unavailable")
            })
            .transpose()?;
        let models = runtime
            .server
            .as_ref()
            .map(|s| s.snapshot())
            .transpose()?
            .unwrap_or_default();
        let reply = json!({"schema_version":1,"report_type":"model_lifecycle","node_id":self.node_id,"runtime_id":runtime.id,
            "command":request.command,"supported":runtime.server.is_some(),"models":models,"receipt":receipt,"reused":reused,
            "execution_authorized":false,"model_inference_requested":false,"file_deletion":false,"automatically_retried":false,
            "note":"Private selected-GGUF runtime state. Load/unload controls owned processes, not inference permission or physical memory guarantees. Exact receipt reuse never replays. Runtime restart interrupts, never resumes, model operations."});
        if !confirmed(&reply, &request) {
            bail!("Model lifecycle reply not confirmed; inspect exact receipt")
        }
        Ok(reply)
    }
}
impl ModelRuntimeGuard {
    pub async fn run(&self) -> Result<()> {
        self.runtime.run().await
    }
}
impl ModelRuntime {
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        for op in self.records.lock().unwrap().values_mut() {
            if matches!(
                op.receipt.phase.as_str(),
                "queued" | "running" | "cancel_requested"
            ) {
                op.cancel.send_replace(true);
                op.receipt.phase = "interrupted".into();
                op.receipt.error_code = Some("runtime_closed".into());
            }
        }
        if let Some(server) = &self.server {
            server.close();
        }
        let _ = self.queue.try_send(String::new());
    }
    async fn run(&self) -> Result<()> {
        let mut receiver = self
            .receiver
            .lock()
            .await
            .take()
            .context("Model lifecycle worker already owned")?;
        while let Some(id) = receiver.recv().await {
            if self.closed.load(Ordering::SeqCst) {
                break;
            }
            let (command, mut cancel, remote) = {
                let mut records = self.records.lock().unwrap();
                let Some(op) = records.get_mut(&id) else {
                    continue;
                };
                if !matches!(op.receipt.phase.as_str(), "queued" | "cancel_requested") {
                    continue;
                }
                op.receipt.phase = "running".into();
                (op.receipt.command.clone(), op.cancel.subscribe(), op.remote)
            };
            let server = self
                .server
                .as_ref()
                .context("Owned model runtime unavailable")?;
            let model = match &command {
                LifecycleCommand::Load { model, .. } | LifecycleCommand::Unload { model, .. } => {
                    model
                }
                _ => unreachable!(),
            };
            let mut cancelled = *cancel.borrow_and_update();
            let outcome = if cancelled {
                Ok(())
            } else {
                tokio::select! {biased;
                    _=cancel.changed()=>{cancelled=true;Ok(())},
                    result=async {match &command {
                        LifecycleCommand::Load{..}=>server.load_reserved(model,&id,Duration::from_secs(300),|slot|{
                            let service=self.service.upgrade().context("model owner service closed")?;
                            service.inspect_grant_configuration(|config| {
                                self.authorized(config,remote)?;
                                server.spawn_slot(slot,Some(&id))
                            })
                        }).await,
                        LifecycleCommand::Unload{..}=>{
                            let service=self.service.upgrade().context("model owner service closed")?;
                            server.unload_reserved(model,&id,||service.guard_grant_configuration(|config|self.authorized(config,remote))).await
                        },
                        _=>unreachable!(),
                    }}=>result,
                }
            };
            server.release_control(model, &id);
            let mut records = self.records.lock().unwrap();
            let op = records.get_mut(&id).context("Model receipt disappeared")?;
            if op.receipt.phase == "interrupted" {
                continue;
            }
            op.receipt.phase = if cancelled {
                "cancelled"
            } else if outcome.is_ok() {
                "completed"
            } else {
                "failed"
            }
            .into();
            op.receipt.error_code = outcome.err().map(|_| "owned_model_operation_failed".into());
        }
        Ok(())
    }
    fn authorized(&self, config: &Value, remote: bool) -> Result<()> {
        let owner = self
            .service
            .upgrade()
            .context("model owner service closed")?;
        if self.closed.load(Ordering::SeqCst)
            || self.engine.upgrade().is_none()
            || config["node_id"] != owner.node_id
            || config["models"] != self.models
        {
            bail!("model runtime/settings changed before application")
        }
        if remote && config.pointer("/management/allow_remote") != Some(&json!(true)) {
            bail!("model management approval revoked before application")
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    model_id: String,
    state: crate::model_metadata::ModelLifecycle,
    generation: u64,
    active_requests: u32,
    control_pending: bool,
    owned_pid: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    command: LifecycleCommand,
    supported: bool,
    models: Vec<Snapshot>,
    receipt: Option<Receipt>,
    reused: bool,
    execution_authorized: bool,
    model_inference_requested: bool,
    file_deletion: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &LifecycleRequest) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "model_lifecycle"
        || r.node_id != request.expected_node_id
        || r.command != request.command
        || crate::automation_occurrence::uuid(&r.runtime_id).is_err()
        || request
            .expected_runtime_id
            .as_ref()
            .is_some_and(|id| *id != r.runtime_id)
        || r.execution_authorized
        || r.model_inference_requested
        || r.file_deletion
        || r.automatically_retried
        || r.note.len() > 1024
        || r.models.len() > 256
    {
        return false;
    }
    let mut last = None;
    for s in &r.models {
        if crate::model_selection::validate_id(&s.model_id).is_err()
            || last.is_some_and(|id: &str| id >= s.model_id.as_str())
            || s.owned_pid == Some(0)
            || (s.active_requests > 0 && s.state != crate::model_metadata::ModelLifecycle::Active)
        {
            return false;
        }
        let _ = (s.generation, s.control_pending);
        last = Some(s.model_id.as_str());
    }
    if !r.supported && !r.models.is_empty() {
        return false;
    }
    if matches!(request.command, LifecycleCommand::Show { .. }) && r.reused {
        return false;
    }
    let selected = match &request.command {
        LifecycleCommand::Show { operation_id } => operation_id.as_deref(),
        LifecycleCommand::Load { operation_id, .. }
        | LifecycleCommand::Unload { operation_id, .. }
        | LifecycleCommand::Cancel { operation_id } => Some(operation_id.as_str()),
    };
    if selected.is_some() != r.receipt.is_some() {
        return false;
    }
    if let Some(receipt) = r.receipt {
        let nested = LifecycleRequest {
            schema_version: 1,
            expected_node_id: r.node_id.clone(),
            expected_runtime_id: Some(r.runtime_id.clone()),
            command: receipt.command.clone(),
        };
        if nested.validate().is_err() || !r.supported {
            return false;
        }
        if matches!(request.command, LifecycleCommand::Cancel { .. })
            && !matches!(receipt.command, LifecycleCommand::Load { .. })
        {
            return false;
        }
        if matches!(receipt.phase.as_str(), "failed" | "interrupted")
            != receipt.error_code.is_some()
        {
            return false;
        }
        if !matches!(
            receipt.phase.as_str(),
            "queued"
                | "running"
                | "cancel_requested"
                | "completed"
                | "failed"
                | "cancelled"
                | "interrupted"
        ) || receipt.error_code.as_ref().is_some_and(|e| e.len() > 64)
        {
            return false;
        }
        match &receipt.command {
            LifecycleCommand::Load { operation_id, .. }
            | LifecycleCommand::Unload { operation_id, .. } => {
                if Some(operation_id.as_str()) != selected {
                    return false;
                }
            }
            _ => return false,
        }
        if matches!(
            request.command,
            LifecycleCommand::Load { .. } | LifecycleCommand::Unload { .. }
        ) && receipt.command != request.command
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        tempfile::TempDir,
        Arc<crate::NodeService>,
        Arc<crate::NodeEngine>,
        ModelRuntimeGuard,
    ) {
        fixture_selections(1)
    }
    fn fixture_selections(
        count: usize,
    ) -> (
        tempfile::TempDir,
        Arc<crate::NodeService>,
        Arc<crate::NodeEngine>,
        ModelRuntimeGuard,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"owner","models":{"provider":"llamacpp"},"management":{"allow_remote":true}})).unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        let slots = (0..count).map(|i| serde_json::from_value(
            json!({"model_id":if i == 0 {"selected".to_string()} else {format!("selected-{i}")},"model_path":"PRIVATE-NOT-PRESENT"}),
        ).unwrap()).collect();
        let server = crate::ModelServer::new(dir.path().join("missing.exe"), slots);
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        (dir, service, engine, guard)
    }
    fn request(guard: &ModelRuntimeGuard, command: LifecycleCommand) -> LifecycleRequest {
        LifecycleRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: Some(guard.runtime.id.clone()),
            command,
        }
    }
    #[tokio::test]
    async fn exact_load_receipts_are_bounded_inspection_and_cancel_without_spawning() {
        let (_dir, service, _engine, guard) = fixture();
        let id = uuid::Uuid::new_v4().to_string();
        let cmd = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: id.clone(),
            },
        );
        let first = service
            .model_lifecycle_command(cmd.clone(), false)
            .await
            .unwrap();
        assert_eq!(first["receipt"]["phase"], "queued");
        assert_eq!(first["models"][0]["owned_pid"], Value::Null);
        assert!(confirmed(&first, &cmd));
        assert_eq!(
            service
                .model_lifecycle_command(cmd.clone(), false)
                .await
                .unwrap()["reused"],
            true
        );
        let cancel = request(
            &guard,
            LifecycleCommand::Cancel {
                operation_id: id.clone(),
            },
        );
        service
            .model_lifecycle_command(cancel, false)
            .await
            .unwrap();
        let worker = guard.runtime.clone();
        let task = tokio::spawn(async move { worker.run().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let r = service
                    .model_lifecycle_command(
                        request(
                            &guard,
                            LifecycleCommand::Show {
                                operation_id: Some(id.clone()),
                            },
                        ),
                        false,
                    )
                    .await
                    .unwrap();
                if r["receipt"]["phase"] == "cancelled" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            service.model_lifecycle_command(cmd, false).await.unwrap()["reused"],
            true
        );
        guard.runtime.close();
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn stale_runtime_generation_conflicting_receipt_or_revoked_owner_deny_admission() {
        let (_dir, service, _engine, guard) = fixture();
        let cmd = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 1,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        assert!(service.model_lifecycle_command(cmd, false).await.is_err());
        let mut cmd = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        cmd.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(
            service
                .model_lifecycle_command(cmd.clone(), false)
                .await
                .is_err()
        );
        cmd.expected_runtime_id = Some(guard.runtime.id.clone());
        let mut config = service.read_config().unwrap();
        config["management"]["allow_remote"] = json!(false);
        service.save_config(config).unwrap();
        assert!(
            service
                .model_lifecycle_command(cmd.clone(), true)
                .await
                .is_err()
        );
        assert!(guard.runtime.records.lock().unwrap().is_empty());
        service
            .model_lifecycle_command(cmd.clone(), false)
            .await
            .unwrap();
        if let LifecycleCommand::Load { model, .. } = &mut cmd.command {
            *model = "different".into();
        }
        assert!(service.model_lifecycle_command(cmd, false).await.is_err());
    }
    #[tokio::test]
    async fn queued_application_rechecks_permission_and_closed_runtime_cannot_spawn_late() {
        let (_dir, service, engine, guard) = fixture();
        let id = uuid::Uuid::new_v4().to_string();
        let cmd = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: id.clone(),
            },
        );
        service
            .model_lifecycle_command(cmd.clone(), true)
            .await
            .unwrap();
        let mut config = service.read_config().unwrap();
        config["management"]["allow_remote"] = json!(false);
        service.save_config(config).unwrap();
        assert!(
            guard
                .runtime
                .authorized(&service.read_config().unwrap(), true)
                .is_err()
        );
        let worker = guard.runtime.clone();
        let task = tokio::spawn(async move { worker.run().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if guard.runtime.records.lock().unwrap()[&id].receipt.phase == "failed" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            engine.model_server.as_ref().unwrap().snapshot().unwrap()[0]["owned_pid"].is_null()
        );
        assert_eq!(
            service.model_lifecycle_command(cmd, true).await.unwrap()["reused"],
            true
        );
        guard.runtime.close();
        task.await.unwrap().unwrap();
        assert!(
            engine
                .model_server
                .as_ref()
                .unwrap()
                .start(Duration::from_secs(1))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn future_extra_authority_or_false_inference_reply_is_never_confirmed() {
        let (_dir, service, _engine, guard) = fixture();
        let cmd = request(&guard, LifecycleCommand::Show { operation_id: None });
        let good = service
            .model_lifecycle_command(cmd.clone(), false)
            .await
            .unwrap();
        assert!(confirmed(&good, &cmd));
        for field in [
            "execution_authorized",
            "file_deletion",
            "model_inference_requested",
            "automatically_retried",
        ] {
            let mut bad = good.clone();
            bad[field] = json!(true);
            assert!(!confirmed(&bad, &cmd));
        }
        let mut bad = good.clone();
        bad["extra_permission"] = json!(true);
        assert!(!confirmed(&bad, &cmd));
        let mut raw = json!(cmd);
        raw["command"]["force"] = json!(true);
        assert!(serde_json::from_value::<LifecycleRequest>(raw).is_err());
    }
    #[tokio::test]
    async fn concurrent_exact_commands_admit_once_and_reserved_actions_cannot_load() {
        let (_dir, service, _engine, guard) = fixture();
        let command = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        for action in ["model_lifecycle_status", "model_lifecycle_cancel"] {
            assert!(service.remote(action, json!(command)).await.is_err());
        }
        assert!(guard.runtime.records.lock().unwrap().is_empty());
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let service = service.clone();
            let command = command.clone();
            tasks.spawn(async move {
                service
                    .model_lifecycle_command(command, true)
                    .await
                    .unwrap()
            });
        }
        let mut fresh = 0;
        while let Some(result) = tasks.join_next().await {
            let reply = result.unwrap();
            assert!(confirmed(&reply, &command));
            if reply["reused"] == false {
                fresh += 1;
            }
        }
        assert_eq!(fresh, 1);
        assert_eq!(guard.runtime.records.lock().unwrap().len(), 1);
        let mut receiver = guard.runtime.receiver.lock().await;
        assert!(receiver.as_mut().unwrap().try_recv().is_ok());
        assert!(receiver.as_mut().unwrap().try_recv().is_err());
    }
    #[tokio::test]
    async fn queue_overflow_withdraws_only_unadmitted_control_and_keeps_existing_receipts() {
        let (_dir, service, _engine, guard) = fixture_selections(9);
        for i in 0..8 {
            let cmd = request(
                &guard,
                LifecycleCommand::Load {
                    model: if i == 0 {
                        "selected".into()
                    } else {
                        format!("selected-{i}")
                    },
                    generation: 0,
                    operation_id: uuid::Uuid::new_v4().to_string(),
                },
            );
            assert!(service.model_lifecycle_command(cmd, false).await.is_ok());
        }
        let cmd = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected-8".into(),
                generation: 0,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        assert!(
            service
                .model_lifecycle_command(cmd.clone(), false)
                .await
                .is_err()
        );
        let rows = guard.runtime.server.as_ref().unwrap().snapshot().unwrap();
        assert_eq!(
            rows.iter().filter(|s| s["control_pending"] == true).count(),
            8
        );
        assert_eq!(guard.runtime.records.lock().unwrap().len(), 8);
        assert!(rows.iter().all(|s| s["owned_pid"].is_null()));
        guard
            .runtime
            .receiver
            .lock()
            .await
            .as_mut()
            .unwrap()
            .try_recv()
            .unwrap();
        assert!(service.model_lifecycle_command(cmd, false).await.is_ok());
    }
    #[tokio::test]
    async fn receipt_capacity_never_evicts_replay_protection_or_admits_an_unrecorded_command() {
        let (_dir, service, _engine, guard) = fixture();
        let command = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        let mut records = guard.runtime.records.lock().unwrap();
        for _ in 0..256 {
            records.insert(
                uuid::Uuid::new_v4().to_string(),
                Operation {
                    receipt: Receipt {
                        command: command.command.clone(),
                        phase: "cancelled".into(),
                        error_code: None,
                    },
                    cancel: watch::channel(false).0,
                    remote: false,
                },
            );
        }
        drop(records);
        assert!(
            service
                .model_lifecycle_command(command, false)
                .await
                .is_err()
        );
        assert_eq!(guard.runtime.records.lock().unwrap().len(), 256);
        assert_eq!(
            guard.runtime.server.as_ref().unwrap().snapshot().unwrap()[0]["control_pending"],
            false
        );
        assert!(
            guard
                .runtime
                .receiver
                .lock()
                .await
                .as_mut()
                .unwrap()
                .try_recv()
                .is_err()
        );
    }
    #[tokio::test]
    async fn queued_load_cannot_apply_after_saved_identity_changes_or_engine_disappears() {
        for missing_engine in [false, true] {
            let (_dir, service, engine, guard) = fixture();
            let id = uuid::Uuid::new_v4().to_string();
            service
                .model_lifecycle_command(
                    request(
                        &guard,
                        LifecycleCommand::Load {
                            model: "selected".into(),
                            generation: 0,
                            operation_id: id.clone(),
                        },
                    ),
                    false,
                )
                .await
                .unwrap();
            let mut retained_engine = Some(engine);
            if missing_engine {
                drop(retained_engine.take());
            } else {
                let mut saved = service.read_config().unwrap();
                saved["node_id"] = json!("replacement-owner");
                // Normal owner saves deliberately preserve stable identity.
                // Simulate an out-of-band file replacement before application.
                crate::setup::write_json(&service.config_path, &saved).unwrap();
            }
            assert!(
                guard
                    .runtime
                    .authorized(&service.read_config().unwrap(), false)
                    .is_err()
            );
            let worker = guard.runtime.clone();
            let task = tokio::spawn(async move { worker.run().await });
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if guard.runtime.records.lock().unwrap()[&id].receipt.phase == "failed" {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(
                guard.runtime.server.as_ref().unwrap().snapshot().unwrap()[0]["owned_pid"]
                    .is_null()
            );
            guard.runtime.close();
            task.await.unwrap().unwrap();
            drop(retained_engine);
        }
    }
}
