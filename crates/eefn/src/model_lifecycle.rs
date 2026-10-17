//! Runtime-owned model lifecycle controls, not workload authorization.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
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
    /// Prepare an installed applied Ollama selection without generating text.
    /// Shared residency can change; cancellation never unloads the shared model.
    PrepareOllama {
        #[arg(long)]
        model: String,
        /// Exact provider-reported digest from installed-model inspection.
        #[arg(long)]
        digest: String,
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
    /// Cancel this exact queued/running load/preparation; never cancel inference.
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
            LifecycleCommand::PrepareOllama {
                model,
                digest,
                operation_id,
            } => {
                crate::model_selection::validate_id(model)?;
                crate::automation_occurrence::uuid(operation_id)?;
                if !valid_provider_digest(digest) {
                    bail!("Inspect the installed Ollama model's exact SHA-256 digest")
                }
            }
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    shared_service_effects_possible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preparation: Option<OllamaPreparationObservation>,
}
/// A point-in-time provider report, not a verified file hash or residency lease.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OllamaPreparationObservation {
    pub provider_reported_digest: String,
    pub observed_context_tokens: Option<u32>,
}
pub(crate) fn valid_provider_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
    policy: crate::model_lifecycle_policy::ModelLifecyclePolicy,
    policy_revision: u64,
    idle_events: Mutex<VecDeque<IdleEvent>>,
    closed: AtomicBool,
    records: Mutex<BTreeMap<String, Operation>>,
    queue: mpsc::Sender<String>,
    receiver: tokio::sync::Mutex<Option<mpsc::Receiver<String>>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdleEvent {
    model_id: String,
    generation: u64,
    outcome: String,
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
    pub(crate) async fn acquire_demand_model<'a>(
        &self,
        server: &'a crate::ModelServer,
        model: &str,
    ) -> Result<crate::model_server::ModelUse<'a>> {
        let runtime = self
            .model_runtime
            .lock()
            .unwrap()
            .clone()
            .context("model runtime unavailable")?;
        if !runtime
            .server
            .as_ref()
            .is_some_and(|owned| std::ptr::eq(owned.as_ref(), server))
            || !runtime.demand_authorized()
        {
            bail!("on-demand model policy unavailable or changed")
        }
        server
            .acquire_demand_pin(
                model,
                Duration::from_secs(runtime.policy.load_timeout_seconds),
                |slot, id| {
                    self.inspect_grant_configuration(|config| {
                        runtime.authorized(config, false)?;
                        server.spawn_slot(slot, Some(id))
                    })
                },
                || self.guard_grant_configuration(|config| runtime.authorized(config, false)),
            )
            .await
    }
    pub fn bind_model_runtime(
        self: &Arc<Self>,
        engine: &Arc<crate::NodeEngine>,
        id: String,
    ) -> Result<ModelRuntimeGuard> {
        let config = self.read_config()?;
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy::from_config(&config)?;
        let revision =
            crate::model_lifecycle_policy::ModelLifecyclePolicy::revision_from_config(&config)?;
        self.bind_model_runtime_with_policy(engine, id, policy, revision)
    }
    pub fn bind_model_runtime_with_policy(
        self: &Arc<Self>,
        engine: &Arc<crate::NodeEngine>,
        id: String,
        policy: crate::model_lifecycle_policy::ModelLifecyclePolicy,
        policy_revision: u64,
    ) -> Result<ModelRuntimeGuard> {
        crate::automation_occurrence::uuid(&id)?;
        policy.validate()?;
        if engine.applied_ollama_keep_alive_seconds() != policy.ollama_keep_alive_seconds {
            bail!("Applied Ollama preference does not match the engine; no runtime replacement")
        }
        if engine.applied_ollama_context_tokens() != policy.ollama_context_tokens {
            bail!("Applied Ollama context does not match the engine; no runtime replacement")
        }
        if engine.applied_thinking_mode() != policy.thinking_mode {
            bail!("Applied thinking preference does not match the engine; no runtime replacement")
        }
        if engine.applied_interpreter_sampling() != &policy.interpreter_sampling {
            bail!("Applied interpreter sampling does not match the engine; no runtime replacement")
        }
        let (queue, receiver) = mpsc::channel(policy.queue_capacity);
        let runtime = Arc::new(ModelRuntime {
            id,
            engine: Arc::downgrade(engine),
            service: Arc::downgrade(self),
            server: engine.model_server.clone(),
            models: self.read_config()?["models"].clone(),
            policy,
            policy_revision,
            idle_events: Mutex::new(VecDeque::new()),
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
        if let Some(server) = &runtime.server {
            server.bind_demand_runtime(Arc::downgrade(&runtime));
        }
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
                if !matches!(
                    old.receipt.command,
                    LifecycleCommand::Load { .. } | LifecycleCommand::PrepareOllama { .. }
                ) {
                    bail!("Only an exact load or preparation can be cancelled")
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
            LifecycleCommand::PrepareOllama {
                model,
                operation_id,
                ..
            } => {
                let mut records = runtime.records.lock().unwrap();
                if let Some(old) = records.get(operation_id) {
                    if old.receipt.command != request.command {
                        bail!("Model lifecycle receipt conflicts; inspect original")
                    }
                    reused = true;
                } else {
                    if records.len() >= runtime.policy.receipt_capacity {
                        bail!("Model lifecycle receipt capacity reached; no admission")
                    }
                    let engine = runtime.engine.upgrade().context("Model runtime closed")?;
                    self.inspect_grant_configuration(|config| {
                        runtime.authorized(config, remote)?;
                        if !matches!(config.pointer("/models/provider").and_then(Value::as_str).unwrap_or("auto"), "auto" | "ollama")
                            || !engine.can_prepare_ollama(model)
                            || runtime.policy.ollama_keep_alive_seconds == Some(0) {
                            bail!("Applied Ollama preparation unavailable; inspect selection and retention")
                        }
                        runtime.queue.try_send(operation_id.clone())?;
                        records.insert(operation_id.clone(), Operation {
                            receipt: Receipt { command: request.command.clone(), phase: "queued".into(), error_code: None,
                                shared_service_effects_possible: true, preparation: None },
                            cancel: watch::channel(false).0, remote,
                        });
                        Ok(())
                    })?;
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
                    if records.len() >= runtime.policy.receipt_capacity {
                        bail!("Model lifecycle receipt capacity reached; no admission")
                    }
                    let server = runtime.server.as_ref().context("Owned GGUF lifecycle unavailable for this active backend; no fallback or external unload")?;
                    self.inspect_grant_configuration(|config| {
                        if runtime.closed.load(Ordering::SeqCst) || runtime.engine.upgrade().is_none() {
                            bail!("Model runtime closed")
                        }
                        if config["node_id"] != self.node_id || config["models"] != runtime.models
                            || crate::model_lifecycle_policy::ModelLifecyclePolicy::from_config(config)? != runtime.policy
                            || crate::model_lifecycle_policy::ModelLifecyclePolicy::revision_from_config(config)? != runtime.policy_revision {
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
                            receipt: Receipt { command: request.command.clone(), phase: "queued".into(), error_code: None,
                                shared_service_effects_possible: false, preparation: None },
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
        let mut reply = json!({"schema_version":1,"report_type":"model_lifecycle","node_id":self.node_id,"runtime_id":runtime.id,
            "command":request.command,"supported":runtime.server.is_some(),"models":models,"receipt":receipt,"reused":reused,
            "idle_events":runtime.idle_events.lock().unwrap().iter().cloned().collect::<Vec<_>>(),
            "execution_authorized":false,"model_inference_requested":false,"file_deletion":false,"automatically_retried":false,
            "note":"Load/unload controls owned GGUF processes. Ollama preparation observes shared residency only; cancellation/timeout/restart does not undo shared effects. Neither grants inference permission or physical memory guarantees. Exact receipt reuse never replays. Restart interrupts, never resumes, model operations."});
        if runtime
            .engine
            .upgrade()
            .is_some_and(|engine| engine.supports_ollama_preparation())
        {
            reply["ollama_preparation_supported"] = json!(true);
        }
        if !confirmed(&reply, &request) {
            bail!("Model lifecycle reply not confirmed; inspect exact receipt")
        }
        Ok(reply)
    }
}
impl ModelRuntimeGuard {
    pub fn policy(&self) -> &crate::model_lifecycle_policy::ModelLifecyclePolicy {
        &self.runtime.policy
    }
    pub async fn run(&self) -> Result<()> {
        self.runtime.run().await
    }
}
impl ModelRuntime {
    pub(crate) fn demand_authorized(&self) -> bool {
        self.policy.startup == crate::model_lifecycle_policy::StartupMode::OnDemand
            && self
                .service
                .upgrade()
                .and_then(|service| service.read_config().ok())
                .is_some_and(|config| self.authorized(&config, false).is_ok())
    }
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub(crate) fn policy(&self) -> &crate::model_lifecycle_policy::ModelLifecyclePolicy {
        &self.policy
    }
    pub(crate) fn policy_revision(&self) -> u64 {
        self.policy_revision
    }
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
        let mut idle_tick = tokio::time::interval(Duration::from_secs(1));
        idle_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let id = tokio::select! {biased;
                next=receiver.recv()=>{let Some(id)=next else {break};id},
                _=idle_tick.tick(),if self.policy.idle_unload_seconds.is_some()=>{
                    if self.closed.load(Ordering::SeqCst) {break;}
                    self.unload_idle().await;
                    continue;
                },
            };
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
            let model = match &command {
                LifecycleCommand::Load { model, .. }
                | LifecycleCommand::Unload { model, .. }
                | LifecycleCommand::PrepareOllama { model, .. } => model,
                _ => unreachable!(),
            };
            let mut cancelled = *cancel.borrow_and_update();
            let outcome = if cancelled {
                Ok(None)
            } else {
                tokio::select! {biased;
                    _=cancel.changed()=>{cancelled=true;Ok(None)},
                    result=async {match &command {
                        LifecycleCommand::PrepareOllama{digest,..}=>{
                            let service=self.service.upgrade().context("model owner service closed")?;
                            let engine=self.engine.upgrade().context("model engine closed")?;
                            tokio::time::timeout(Duration::from_secs(self.policy.load_timeout_seconds),
                                engine.prepare_ollama_checked(model,digest,||service.inspect_grant_configuration(|config|self.authorized(config,remote))))
                                .await.context("Ollama preparation deadline exceeded")?.map(Some)
                        },
                        LifecycleCommand::Load{..}=>{
                            let server=self.server.as_ref().context("Owned model runtime unavailable")?;
                            let service=self.service.upgrade().context("model owner service closed")?;
                            server.load_reserved_checked(model,&id,Duration::from_secs(self.policy.load_timeout_seconds),|slot|{
                            service.inspect_grant_configuration(|config| {
                                self.authorized(config,remote)?;
                                server.spawn_slot(slot,Some(&id))
                            })
                            },||service.guard_grant_configuration(|config|self.authorized(config,remote))).await.map(|()|None)
                        },
                        LifecycleCommand::Unload{..}=>{
                            let server=self.server.as_ref().context("Owned model runtime unavailable")?;
                            let service=self.service.upgrade().context("model owner service closed")?;
                            server.unload_reserved_with_timeout(model,&id,Duration::from_secs(self.policy.termination_wait_seconds),||service.guard_grant_configuration(|config|self.authorized(config,remote))).await.map(|()|None)
                        },
                        _=>unreachable!(),
                    }}=>result,
                }
            };
            if !matches!(command, LifecycleCommand::PrepareOllama { .. }) {
                if let Some(server) = &self.server {
                    server.release_control(model, &id);
                }
            }
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
            match outcome {
                Ok(observation) => op.receipt.preparation = observation,
                Err(_) => {
                    op.receipt.error_code = Some(
                        if matches!(command, LifecycleCommand::PrepareOllama { .. }) {
                            "ollama_preparation_failed"
                        } else {
                            "owned_model_operation_failed"
                        }
                        .into(),
                    )
                }
            }
        }
        Ok(())
    }
    async fn unload_idle(&self) {
        let Some(seconds) = self.policy.idle_unload_seconds else {
            return;
        };
        let Some(server) = &self.server else { return };
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let idle = Duration::from_secs(seconds);
        let Ok(candidates) = server.idle_candidates(idle) else {
            return;
        };
        for (model, generation) in candidates {
            let id = uuid::Uuid::new_v4().to_string();
            let reserved = service.inspect_grant_configuration(|config| {
                self.authorized(config, false)?;
                server.reserve_idle(&model, generation, idle, &id)
            });
            if reserved.is_err() {
                continue;
            }
            let outcome = server
                .unload_reserved_with_timeout(
                    &model,
                    &id,
                    Duration::from_secs(self.policy.termination_wait_seconds),
                    || service.guard_grant_configuration(|config| self.authorized(config, false)),
                )
                .await;
            server.release_control(&model, &id);
            let mut events = self.idle_events.lock().unwrap();
            if events.len() == 32 {
                events.pop_front();
            }
            events.push_back(IdleEvent {
                model_id: model,
                generation,
                outcome: if outcome.is_ok() {
                    "completed"
                } else {
                    "failed"
                }
                .into(),
            });
        }
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
            || crate::model_lifecycle_policy::ModelLifecyclePolicy::from_config(config)?
                != self.policy
            || crate::model_lifecycle_policy::ModelLifecyclePolicy::revision_from_config(config)?
                != self.policy_revision
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
    #[serde(default)]
    job_accounting: Option<crate::job_accounting::JobAccounting>,
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
    #[serde(default)]
    ollama_preparation_supported: bool,
    models: Vec<Snapshot>,
    #[serde(default)]
    idle_events: Vec<IdleEvent>,
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
        || r.idle_events.len() > 32
    {
        return false;
    }
    let mut last = None;
    if r.idle_events.iter().any(|event| {
        crate::model_selection::validate_id(&event.model_id).is_err()
            || !matches!(event.outcome.as_str(), "completed" | "failed")
    }) {
        return false;
    }
    for s in &r.models {
        if crate::model_selection::validate_id(&s.model_id).is_err()
            || last.is_some_and(|id: &str| id >= s.model_id.as_str())
            || s.owned_pid == Some(0)
            || s.job_accounting.as_ref().is_some_and(|sample| {
                !sample.validate()
                    || (matches!(
                        sample,
                        crate::job_accounting::JobAccounting::NoOwnedJob { .. }
                    ) && s.owned_pid.is_some())
            })
            || (s.active_requests > 0 && s.state != crate::model_metadata::ModelLifecycle::Active)
        {
            return false;
        }
        let _ = (s.generation, s.control_pending);
        last = Some(s.model_id.as_str());
    }
    if !r.supported && (!r.models.is_empty() || !r.idle_events.is_empty()) {
        return false;
    }
    if matches!(request.command, LifecycleCommand::Show { .. }) && r.reused {
        return false;
    }
    let selected = match &request.command {
        LifecycleCommand::Show { operation_id } => operation_id.as_deref(),
        LifecycleCommand::Load { operation_id, .. }
        | LifecycleCommand::Unload { operation_id, .. }
        | LifecycleCommand::PrepareOllama { operation_id, .. }
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
        let shared = matches!(receipt.command, LifecycleCommand::PrepareOllama { .. });
        if nested.validate().is_err()
            || (if shared {
                !r.ollama_preparation_supported
            } else {
                !r.supported
            })
            || receipt.shared_service_effects_possible != shared
            || receipt.preparation.is_some() != (shared && receipt.phase == "completed")
        {
            return false;
        }
        if matches!(request.command, LifecycleCommand::Cancel { .. })
            && !matches!(
                receipt.command,
                LifecycleCommand::Load { .. } | LifecycleCommand::PrepareOllama { .. }
            )
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
            LifecycleCommand::PrepareOllama {
                operation_id,
                digest,
                ..
            } => {
                if Some(operation_id.as_str()) != selected
                    || receipt.preparation.as_ref().is_some_and(|p| {
                        p.provider_reported_digest != *digest
                            || !valid_provider_digest(&p.provider_reported_digest)
                            || p.observed_context_tokens == Some(0)
                    })
                {
                    return false;
                }
            }
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
            LifecycleCommand::Load { .. }
                | LifecycleCommand::Unload { .. }
                | LifecycleCommand::PrepareOllama { .. }
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
    fn ollama_fixture() -> (
        tempfile::TempDir,
        Arc<crate::NodeService>,
        Arc<crate::NodeEngine>,
        ModelRuntimeGuard,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            ollama_context_tokens: Some(4096),
            ollama_keep_alive_seconds: Some(60),
            ..Default::default()
        };
        let selected: crate::SelectedModel =
            serde_json::from_value(json!({"model_id":"selected","modality":"text"})).unwrap();
        crate::setup::write_json(&path, &json!({"node_id":"owner","models":{"provider":"ollama",
            "ollama":{"base_url":"http://127.0.0.1:1","selected":[selected]}},"model_lifecycle":policy,"management":{"allow_remote":true}})).unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone())
                .with_ollama("http://127.0.0.1:1".into(), [selected])
                .unwrap()
                .with_model_request_preferences(&policy)
                .unwrap(),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        (dir, service, engine, guard)
    }
    fn prepare_command() -> LifecycleCommand {
        LifecycleCommand::PrepareOllama {
            model: "selected".into(),
            digest: "a".repeat(64),
            operation_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    #[tokio::test]
    async fn auto_provider_prepares_only_when_ollama_is_actually_applied() {
        let (_dir, service, engine, guard) = ollama_fixture();
        let mut config = service.read_config().unwrap();
        config["models"]["provider"] = json!("auto");
        service.save_config(config).unwrap();
        drop(guard);
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        let reply = service
            .model_lifecycle_command(request(&guard, prepare_command()), false)
            .await
            .unwrap();
        assert_eq!(reply["ollama_preparation_supported"], true);
        assert_eq!(reply["receipt"]["phase"], "queued");
        let (dir, service, _old_engine, guard) = fixture();
        let mut config = service.read_config().unwrap();
        config["models"]["provider"] = json!("auto");
        service.save_config(config).unwrap();
        drop(guard);
        // A real restart builds a fresh engine. The old owned GGUF server is
        // permanently closed by its guard and must never be reused here.
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        let reply = service
            .model_lifecycle_command(
                request(&guard, LifecycleCommand::Show { operation_id: None }),
                false,
            )
            .await
            .unwrap();
        assert!(reply.get("ollama_preparation_supported").is_none());
        assert!(
            service
                .model_lifecycle_command(request(&guard, prepare_command()), false)
                .await
                .is_err()
        );
    }
    #[test]
    fn preparation_requires_exact_digest_runtime_and_explicit_cli_arguments() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            args: LifecycleArgs,
        }
        let runtime = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let digest = "a".repeat(64);
        let cli = Cli::try_parse_from([
            "policy",
            "--runtime",
            &runtime,
            "prepare-ollama",
            "--model",
            "selected",
            "--digest",
            &digest,
            "--operation-id",
            &id,
        ])
        .unwrap();
        let mut request = LifecycleRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: cli.args.runtime,
            command: cli.args.command,
        };
        request.validate().unwrap();
        for bad in [
            "A".repeat(64),
            "a".repeat(63),
            "x".repeat(64),
            "sha256:".to_string() + &digest,
        ] {
            if let LifecycleCommand::PrepareOllama { digest, .. } = &mut request.command {
                *digest = bad;
            }
            assert!(request.validate().is_err());
        }
        request.command = prepare_command();
        request.expected_runtime_id = None;
        assert!(request.validate().is_err());
        assert!(
            Cli::try_parse_from([
                "policy",
                "prepare-ollama",
                "--model",
                "selected",
                "--operation-id",
                &id
            ])
            .is_err()
        );
    }
    #[tokio::test]
    async fn shared_preparation_admission_cancel_and_historical_reuse_never_claim_owned_unload() {
        let (_dir, service, engine, guard) = ollama_fixture();
        assert!(engine.can_prepare_ollama("selected"));
        assert!(!engine.can_prepare_ollama("not-selected"));
        let req = request(&guard, prepare_command());
        let queued = service
            .model_lifecycle_command(req.clone(), true)
            .await
            .unwrap();
        assert!(confirmed(&queued, &req));
        assert_eq!(queued["supported"], false);
        assert_eq!(queued["ollama_preparation_supported"], true);
        assert_eq!(queued["receipt"]["shared_service_effects_possible"], true);
        assert!(queued["models"].as_array().unwrap().is_empty());
        let LifecycleCommand::PrepareOllama { operation_id, .. } = &req.command else {
            unreachable!()
        };
        service
            .model_lifecycle_command(
                request(
                    &guard,
                    LifecycleCommand::Cancel {
                        operation_id: operation_id.clone(),
                    },
                ),
                false,
            )
            .await
            .unwrap();
        let worker = guard.runtime.clone();
        let task = tokio::spawn(async move { worker.run().await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if guard.runtime.records.lock().unwrap()[operation_id]
                    .receipt
                    .phase
                    == "cancelled"
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reply = service
            .model_lifecycle_command(req.clone(), false)
            .await
            .unwrap();
        assert_eq!(reply["reused"], true);
        assert_eq!(reply["receipt"]["shared_service_effects_possible"], true);
        assert!(reply["receipt"].get("preparation").is_none());
        assert!(confirmed(&reply, &req));
        let mut conflict = req.clone();
        if let LifecycleCommand::PrepareOllama { digest, .. } = &mut conflict.command {
            *digest = "b".repeat(64);
        }
        assert!(
            service
                .model_lifecycle_command(conflict, false)
                .await
                .is_err()
        );
        guard.runtime.close();
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn shared_preparation_denies_wrong_backend_selection_approval_and_pending_policy() {
        let (_dir, service, engine, guard) = ollama_fixture();
        let req = request(&guard, prepare_command());
        let mut wrong = req.clone();
        if let LifecycleCommand::PrepareOllama { model, .. } = &mut wrong.command {
            *model = "other".into();
        }
        assert!(service.model_lifecycle_command(wrong, false).await.is_err());
        let mut config = service.read_config().unwrap();
        config["management"]["allow_remote"] = json!(false);
        service.save_config(config.clone()).unwrap();
        assert!(
            service
                .model_lifecycle_command(req.clone(), true)
                .await
                .is_err()
        );
        config["model_lifecycle"]["ollama_context_tokens"] = json!(8192);
        service.save_config(config).unwrap();
        assert!(service.model_lifecycle_command(req, false).await.is_err());
        assert!(guard.runtime.records.lock().unwrap().is_empty());
        let no_network = std::sync::atomic::AtomicUsize::new(0);
        assert!(
            engine
                .prepare_ollama_checked("selected", &"a".repeat(64), || {
                    no_network.fetch_add(1, Ordering::SeqCst);
                    bail!("revoked")
                })
                .await
                .is_err()
        );
        assert_eq!(no_network.load(Ordering::SeqCst), 1);
        let (_dir, service, _engine, guard) = fixture();
        assert!(
            service
                .model_lifecycle_command(request(&guard, prepare_command()), false)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn shared_preparation_queue_and_receipts_share_bounded_admission() {
        let (_dir, service, _engine, guard) = ollama_fixture();
        for _ in 0..guard.policy().queue_capacity {
            service
                .model_lifecycle_command(request(&guard, prepare_command()), false)
                .await
                .unwrap();
        }
        assert!(
            service
                .model_lifecycle_command(request(&guard, prepare_command()), false)
                .await
                .is_err()
        );
        assert_eq!(
            guard.runtime.records.lock().unwrap().len(),
            guard.policy().queue_capacity
        );
        guard.runtime.close();
        assert!(
            guard
                .runtime
                .records
                .lock()
                .unwrap()
                .values()
                .all(|op| op.receipt.phase == "interrupted"
                    && op.receipt.shared_service_effects_possible)
        );
    }
    #[tokio::test]
    async fn shared_preparation_completion_reports_cannot_drop_effects_or_substitute_identity() {
        let (_dir, service, _engine, guard) = ollama_fixture();
        let req = request(&guard, prepare_command());
        let mut report = service
            .model_lifecycle_command(req.clone(), false)
            .await
            .unwrap();
        report["receipt"]["phase"] = json!("completed");
        assert!(!confirmed(&report, &req));
        report["receipt"]["preparation"] =
            json!({"provider_reported_digest":"a".repeat(64),"observed_context_tokens":4096});
        assert!(confirmed(&report, &req));
        for (pointer, value) in [
            ("/receipt/shared_service_effects_possible", json!(false)),
            ("/ollama_preparation_supported", json!(false)),
            (
                "/receipt/preparation/provider_reported_digest",
                json!("b".repeat(64)),
            ),
            ("/receipt/preparation/observed_context_tokens", json!(0)),
            ("/receipt/phase", json!("cancelled")),
            ("/model_inference_requested", json!(true)),
            ("/execution_authorized", json!(true)),
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(!confirmed(&bad, &req), "{pointer}");
        }
        let (_dir, service, _engine, guard) = fixture();
        let legacy = request(
            &guard,
            LifecycleCommand::Load {
                model: "selected".into(),
                generation: 0,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        let old = service
            .model_lifecycle_command(legacy.clone(), false)
            .await
            .unwrap();
        assert!(old.get("ollama_preparation_supported").is_none());
        assert!(
            old["receipt"]
                .get("shared_service_effects_possible")
                .is_none()
        );
        assert!(confirmed(&old, &legacy));
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
    async fn lifecycle_accounting_rejects_invalid_claims_and_preserves_legacy_reports() {
        let (_dir, service, _engine, guard) = fixture();
        let request = request(&guard, LifecycleCommand::Show { operation_id: None });
        let good = service
            .model_lifecycle_command(request.clone(), false)
            .await
            .unwrap();
        assert!(confirmed(&good, &request));
        assert_eq!(
            good["models"][0]["job_accounting"],
            json!({"state":"no_owned_job"})
        );
        let mut legacy = good.clone();
        legacy["models"][0]
            .as_object_mut()
            .unwrap()
            .remove("job_accounting");
        assert!(confirmed(&legacy, &request));
        for sample in [
            json!({"state":"unavailable","peak_job_commit_bytes":0}),
            json!({"state":"available","job_commit_limit_bytes":0,
            "peak_job_commit_bytes":0,"active_processes":1,"total_user_cpu_100ns":0,"total_kernel_cpu_100ns":0}),
            json!({"state":"available","job_commit_limit_bytes":null,"peak_job_commit_bytes":-1,
                "active_processes":1,"total_user_cpu_100ns":0,"total_kernel_cpu_100ns":0}),
        ] {
            let mut bad = good.clone();
            bad["models"][0]["job_accounting"] = sample;
            assert!(!confirmed(&bad, &request));
        }
        let mut bad = good;
        bad["models"][0]["owned_pid"] = json!(1);
        assert!(!confirmed(&bad, &request));
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
                        shared_service_effects_possible: false,
                        preparation: None,
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
    #[tokio::test]
    async fn applied_idle_policy_uses_owned_unload_without_consuming_manual_receipts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.json");
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            idle_unload_seconds: Some(1),
            ..Default::default()
        };
        crate::setup::write_json(
            &path,
            &json!({"node_id":"owner","models":{"provider":"llamacpp"},"model_lifecycle":policy}),
        )
        .unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        let server = crate::ModelServer::test_resident();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        server.test_age_idle(Duration::from_secs(2));
        let pin = server.pin(Some("owned-model")).unwrap();
        guard.runtime.unload_idle().await;
        assert!(server.snapshot().unwrap()[0]["owned_pid"].is_number());
        drop(pin);
        guard.runtime.unload_idle().await;
        assert!(guard.runtime.idle_events.lock().unwrap().is_empty());
        server.test_age_idle(Duration::from_secs(2));
        let worker = guard.runtime.clone();
        let task = tokio::spawn(async move { worker.run().await });
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if !guard.runtime.idle_events.lock().unwrap().is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(server.snapshot().unwrap()[0]["owned_pid"].is_null());
        assert!(guard.runtime.records.lock().unwrap().is_empty());
        let shown = service
            .model_lifecycle_command(
                request(&guard, LifecycleCommand::Show { operation_id: None }),
                false,
            )
            .await
            .unwrap();
        assert_eq!(shown["idle_events"].as_array().unwrap().len(), 1);
        assert_eq!(shown["idle_events"][0]["outcome"], "completed");
        guard.runtime.close();
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn default_disabled_or_saved_policy_mismatch_never_autonomously_unloads() {
        for idle_enabled in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("node.json");
            let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
                idle_unload_seconds: idle_enabled.then_some(1),
                ..Default::default()
            };
            crate::setup::write_json(&path,&json!({"node_id":"owner","models":{"provider":"llamacpp"},"model_lifecycle":policy})).unwrap();
            let service = crate::NodeService::new(path, "owner".into());
            let server = crate::ModelServer::test_resident();
            let engine = Arc::new(
                crate::NodeEngine::new(false, vec![], directory.path().into())
                    .unwrap()
                    .with_service(service.clone())
                    .with_model_server(server.clone()),
            );
            let guard = service
                .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
                .unwrap();
            server.test_age_idle(Duration::from_secs(2));
            if idle_enabled {
                let mut config = service.read_config().unwrap();
                config["model_lifecycle"]["idle_unload_seconds"] = json!(null);
                service.save_config(config).unwrap();
            }
            guard.runtime.unload_idle().await;
            assert!(server.snapshot().unwrap()[0]["owned_pid"].is_number());
            assert!(guard.runtime.idle_events.lock().unwrap().is_empty());
            guard.runtime.close();
            guard.runtime.unload_idle().await;
            assert!(guard.runtime.idle_events.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn restoring_same_policy_content_cannot_resume_an_old_idle_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.json");
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            idle_unload_seconds: Some(1),
            ..Default::default()
        };
        let original =
            json!({"node_id":"owner","models":{"provider":"llamacpp"},"model_lifecycle":policy});
        crate::setup::write_json(&path, &original).unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        let server = crate::ModelServer::test_resident();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], directory.path().into())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        let mut disabled = original.clone();
        disabled["model_lifecycle"]["idle_unload_seconds"] = Value::Null;
        service.save_config(disabled).unwrap();
        service.save_config(original).unwrap();
        assert_eq!(
            crate::model_lifecycle_policy::ModelLifecyclePolicy::revision_from_config(
                &service.read_config().unwrap()
            )
            .unwrap(),
            2
        );
        server.test_age_idle(Duration::from_secs(2));
        guard.runtime.unload_idle().await;
        assert!(server.snapshot().unwrap()[0]["owned_pid"].is_number());
        assert!(guard.runtime.idle_events.lock().unwrap().is_empty());
        let shown = service
            .model_lifecycle_policy_command(
                crate::model_lifecycle_policy::PolicyRequest {
                    schema_version: 1,
                    expected_node_id: "owner".into(),
                    expected_runtime_id: None,
                    command: crate::model_lifecycle_policy::PolicyCommand::Show {},
                },
                false,
                false,
            )
            .unwrap();
        assert_eq!(shown["saved_policy"], shown["applied_policy"]);
        assert_eq!(shown["restart_required"], true);
        guard.runtime.close();
    }
    #[test]
    fn unapplied_interpreter_sampling_cannot_replace_the_running_owner() {
        let (_directory, service, engine, guard) = fixture();
        let original = guard.runtime.id.clone();
        let mut policy = guard.policy().clone();
        policy.interpreter_sampling.temperature_milli = 1000;
        assert!(
            service
                .bind_model_runtime_with_policy(
                    &engine,
                    uuid::Uuid::new_v4().to_string(),
                    policy.clone(),
                    0
                )
                .is_err()
        );
        assert_eq!(
            service.model_runtime.lock().unwrap().as_ref().unwrap().id,
            original
        );
        assert!(!guard.runtime.is_closed());
        let mut config = service.read_config().unwrap();
        config["model_lifecycle"] = json!(policy);
        service.save_config(config).unwrap();
        let next = Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone())
                .with_model_request_preferences(&policy)
                .unwrap(),
        );
        let applied = service
            .bind_model_runtime(&next, uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert_eq!(
            applied.policy().interpreter_sampling.temperature_milli,
            1000
        );
        assert!(guard.runtime.is_closed());
    }
    #[test]
    fn unapplied_ollama_context_cannot_replace_running_owner() {
        let (_directory, service, engine, guard) = fixture();
        let original = guard.runtime.id.clone();
        let mut policy = guard.policy().clone();
        policy.ollama_context_tokens = Some(4096);
        assert!(
            service
                .bind_model_runtime_with_policy(
                    &engine,
                    uuid::Uuid::new_v4().to_string(),
                    policy,
                    0
                )
                .is_err()
        );
        assert_eq!(
            service.model_runtime.lock().unwrap().as_ref().unwrap().id,
            original
        );
        assert!(!guard.runtime.is_closed());
    }
    #[test]
    fn applied_thinking_report_cannot_replace_runtime_with_unapplied_preference() {
        use crate::model_lifecycle_policy::ThinkingMode;
        let (_directory, service, engine, guard) = fixture();
        let original = guard.runtime.id.clone();
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            thinking_mode: ThinkingMode::Disabled,
            ..guard.policy().clone()
        };
        assert!(
            service
                .bind_model_runtime_with_policy(
                    &engine,
                    uuid::Uuid::new_v4().to_string(),
                    policy.clone(),
                    0
                )
                .is_err()
        );
        assert_eq!(
            service.model_runtime.lock().unwrap().as_ref().unwrap().id,
            original
        );
        assert!(!guard.runtime.is_closed());
        let mut config = service.read_config().unwrap();
        config["model_lifecycle"] = json!(policy);
        service.save_config(config).unwrap();
        let next = Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone())
                .with_model_request_preferences(&policy)
                .unwrap(),
        );
        let applied = service
            .bind_model_runtime(&next, uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert_eq!(applied.policy().thinking_mode, ThinkingMode::Disabled);
        assert_eq!(next.applied_thinking_mode(), ThinkingMode::Disabled);
        assert!(guard.runtime.is_closed());
    }
    #[test]
    fn applied_ollama_report_cannot_claim_a_preference_missing_from_the_engine() {
        let (_directory, service, engine, guard) = fixture();
        let original = guard.runtime.id.clone();
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            ollama_keep_alive_seconds: Some(120),
            ..guard.policy().clone()
        };
        assert!(
            service
                .bind_model_runtime_with_policy(
                    &engine,
                    uuid::Uuid::new_v4().to_string(),
                    policy.clone(),
                    0
                )
                .is_err()
        );
        assert_eq!(
            service.model_runtime.lock().unwrap().as_ref().unwrap().id,
            original
        );
        assert!(
            !guard.runtime.is_closed(),
            "a rejected replacement must retain the running owner"
        );
        let mut config = service.read_config().unwrap();
        config["model_lifecycle"] = json!(policy);
        service.save_config(config).unwrap();
        let next = Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone())
                .with_ollama_residency(&policy)
                .unwrap(),
        );
        let applied = service
            .bind_model_runtime(&next, uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert_eq!(applied.policy().ollama_keep_alive_seconds, Some(120));
        assert_eq!(next.applied_ollama_keep_alive_seconds(), Some(120));
        assert!(guard.runtime.is_closed());
        let report = service
            .model_lifecycle_policy_command(
                crate::model_lifecycle_policy::PolicyRequest {
                    schema_version: 1,
                    expected_node_id: service.node_id.clone(),
                    expected_runtime_id: None,
                    command: crate::model_lifecycle_policy::PolicyCommand::Show {},
                },
                false,
                false,
            )
            .unwrap();
        assert_eq!(report["applied_policy"]["ollama_keep_alive_seconds"], 120);
        assert_eq!(report["restart_required"], false);
    }

    #[tokio::test]
    async fn demand_cold_availability_never_spawns_on_read_and_failed_load_never_retries() {
        let (_directory, service, engine, old_guard) = fixture();
        drop(old_guard);
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            startup: crate::model_lifecycle_policy::StartupMode::OnDemand,
            ..Default::default()
        };
        let mut config = service.read_config().unwrap();
        config["model_lifecycle"] = json!(policy);
        service.save_config(config).unwrap();
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        // A new runtime owns a fresh server; retained closed servers cannot revive.
        assert!(
            !engine
                .model_server
                .as_ref()
                .unwrap()
                .can_infer(Some("selected"))
        );
        drop(guard);
        let artifact = _directory.path().join("synthetic.gguf");
        std::fs::write(&artifact, b"test-only").unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"selected","model_path":artifact})).unwrap();
        let server = crate::ModelServer::new(std::env::current_exe().unwrap(), vec![slot]);
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert!(server.can_infer(Some("selected")));
        assert!(!server.model_ready(Some("selected")));
        assert!(engine.capabilities().contains(&"llm.infer".to_owned()));
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["lifecycle"],
            "unloaded"
        );
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "available"
        );
        assert!(server.snapshot().unwrap()[0]["owned_pid"].is_null());
        assert!(
            service
                .acquire_demand_model(&server, "selected")
                .await
                .is_err()
        );
        let failed = server.snapshot().unwrap();
        assert_eq!(failed[0]["state"], "error");
        assert!(failed[0]["owned_pid"].is_null());
        assert!(!server.can_infer(Some("selected")));
        assert!(
            service
                .acquire_demand_model(&server, "selected")
                .await
                .is_err()
        );
        assert_eq!(server.snapshot().unwrap(), failed);
        assert!(guard.runtime.records.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn demand_authority_is_withdrawn_by_policy_revision_and_old_runtime_close() {
        let (_directory, service, _old_engine, old_guard) = fixture();
        drop(old_guard);
        let policy = crate::model_lifecycle_policy::ModelLifecyclePolicy {
            startup: crate::model_lifecycle_policy::StartupMode::OnDemand,
            ..Default::default()
        };
        let mut config = service.read_config().unwrap();
        config["model_lifecycle"] = json!(policy);
        service.save_config(config.clone()).unwrap();
        let artifact = _directory.path().join("synthetic.gguf");
        std::fs::write(&artifact, b"test-only").unwrap();
        let slot =
            serde_json::from_value(json!({"model_id":"selected","model_path":artifact})).unwrap();
        let server = crate::ModelServer::new(std::env::current_exe().unwrap(), vec![slot]);
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], std::env::temp_dir())
                .unwrap()
                .with_service(service.clone())
                .with_model_server(server.clone()),
        );
        let guard = service
            .bind_model_runtime(&engine, uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert!(server.can_infer(Some("selected")));
        let mut changed = config.clone();
        changed["model_lifecycle"]["startup"] = json!("manual");
        service.save_config(changed).unwrap();
        service.save_config(config).unwrap();
        assert!(!server.can_infer(Some("selected")));
        assert_eq!(
            server.models().unwrap()[0]["model_metadata"]["availability"],
            "unavailable"
        );
        assert!(
            service
                .acquire_demand_model(&server, "selected")
                .await
                .is_err()
        );
        guard.runtime.close();
        assert!(!server.can_infer(Some("selected")));
    }
}
