use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use eefn::firmware::FirmwareConfig;
use eefn::{NodeEvent, NodeServer};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::adapters::AdapterManager;
use crate::assistant::{ResponseEngine, ResponseRule};
use crate::brain::Brain;
use crate::capability::{CapabilityProvider, CapabilityRegistry, NodeSpecs};
use crate::config::Config;
use crate::event::EventBus;
use crate::firmware::FirmwareService;
use crate::memory::{IdentityMemory, MutableMemory, WorkingMemory};
use crate::model::{LlmService, ModelRegistry};
use crate::task::{Dispatcher, TaskEngine};
use crate::update::{UpdatePolicy, UpdateService};
use crate::world::WorldState;

pub struct Runtime {
    pub config: Config,
    pub bus: EventBus,
    pub identity: IdentityMemory,
    pub memory: Arc<MutableMemory>,
    pub working: Arc<Mutex<WorkingMemory>>,
    pub world: Arc<WorldState>,
    pub registry: CapabilityRegistry,
    pub adapters: AdapterManager,
    pub model_registry: ModelRegistry,
    pub llm: Arc<LlmService>,
    pub engine: Arc<TaskEngine>,
    pub node_server: NodeServer,
    pub dispatcher: Arc<Dispatcher>,
    pub firmware: FirmwareService,
    pub assistant: Arc<ResponseEngine>,
    pub brain: Arc<Brain>,
    pub update: Arc<UpdateService>,
    pub coordinator_id: String,
    pub coordinator_priority: i32,
    discovery: crate::discovery::DiscoveryPolicy,
    pub(crate) instance_id: String,
    pub(crate) automation_timers: Arc<crate::automation_timer::TimerManager>,
    pub restart: Arc<tokio::sync::Notify>,
    pub pending_restart: AtomicBool,
    initialized: AtomicBool,
    node_event_loss_count: AtomicU64,
    node_state_reconciliation_count: AtomicU64,
    background: AsyncMutex<Vec<JoinHandle<()>>>,
    submissions: AsyncMutex<tokio::task::JoinSet<()>>,
    pub(crate) model_verification: AsyncMutex<()>,
}

impl Runtime {
    /// Public correlation metadata, not authentication or leadership authority.
    pub fn runtime_id(&self) -> &str {
        &self.instance_id
    }

    pub async fn initialize(
        config: Config,
        db_path: impl AsRef<Path>,
        start_brain: bool,
    ) -> Result<Arc<Self>> {
        let discovery = crate::discovery::DiscoveryPolicy::parse(config.get("discovery"))?;
        let psk = std::env::var("EEF_NODE_PSK")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| config.string("node.psk", ""));
        let host = config.string("node.host", "0.0.0.0");
        let port = u16::try_from(config.u64("node.port", eefn::DEFAULT_PORT.into()))
            .context("node.port exceeds 65535")?;
        let authentication =
            eefn::authentication::AuthenticationPolicy::parse(config.get("node.authentication"))?;
        if authentication.allow_legacy {
            validate_secret(&psk)?;
        }
        let node_server = NodeServer::with_authentication(&psk, host, port, authentication)?;
        let instance_id = node_server.runtime_id().to_owned();

        let bus = EventBus::new(config.u64("logging.history", 500) as usize);
        let identity = IdentityMemory::new(config.clone());
        let memory = MutableMemory::open(
            db_path.as_ref(),
            config.u64("memory.max_conversation", 200) as usize,
        )?;
        let working = Arc::new(Mutex::new(WorkingMemory::new(
            config.u64("memory.max_working_items", 100) as usize,
        )));
        let world = WorldState::new();
        let registry = CapabilityRegistry::with_policy(
            crate::scheduling::SchedulingPolicy::parse(config.get("scheduling"))?,
        )?;
        let policy = Value::Object(config.object("capability_policy"));
        let adapters = AdapterManager::new(registry.clone(), policy);
        let coordinator_id = config.string("coordinator.id", "").trim().to_owned();
        let coordinator_id = if coordinator_id.is_empty() {
            default_coordinator_id()
        } else {
            coordinator_id
        };
        let coordinator_priority = i32::try_from(config.u64("coordinator.priority", 100))
            .context("coordinator.priority exceeds the supported range")?;

        let model_registry = ModelRegistry::default();
        let tiers = model_tiers(&config);
        let llm = LlmService::new(
            model_registry.clone(),
            tiers,
            node_server.clone(),
            bus.clone(),
        );
        let jobs = crate::jobs::JobStore::open(
            db_path.as_ref(),
            config.u64("jobs.max_count", 500) as usize,
            config.u64("jobs.max_bytes", 64 * 1024 * 1024) as usize,
        )?;
        let engine = TaskEngine::with_store(bus.clone(), jobs);
        let dispatcher = Dispatcher::new(
            registry.clone(),
            adapters.clone(),
            llm.clone(),
            node_server.clone(),
        );
        engine.set_executor(dispatcher.clone());
        if !engine.has_executor() {
            bail!("task engine executor must be wired")
        }

        let firmware_defaults = FirmwareConfig {
            ssid: config.string("firmware.ssid", ""),
            wifi_pass: config.string("firmware.wifi_pass", ""),
            host: config.string("firmware.host", "127.0.0.1"),
            port,
            node_id: "esp-01".into(),
            node_name: "esp-01".into(),
            psk: psk.clone(),
        };
        let firmware_repository =
            firmware_repository(&config, &eefn::runtime_layout::RuntimeLayout::current()?);
        let firmware =
            FirmwareService::open(firmware_repository, node_server.clone(), firmware_defaults)?;
        let assistant =
            ResponseEngine::new(bus.clone(), world.clone(), llm.clone(), dispatcher.clone());
        for rule in default_rules() {
            assistant.add_rule(rule)?;
        }
        for raw in config
            .get("assistant.rules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            assistant
                .add_rule(serde_json::from_value(raw.clone()).context("invalid assistant rule")?)?;
        }
        let brain = Brain::new(
            bus.clone(),
            world.clone(),
            identity.clone(),
            memory.clone(),
            engine.clone(),
            llm.clone(),
            assistant.clone(),
            Duration::from_millis(config.u64("brain.tick_ms", 2_000).max(100)),
        );
        let update = UpdateService::new(
            config
                .get("update.manifest_url")
                .and_then(Value::as_str)
                .map(str::to_owned),
            UpdatePolicy::parse(&config.string("update.policy", "prompt"))?,
            Duration::from_secs(config.u64("update.check_interval_seconds", 21_600)),
            eefn::updater::installation_root(std::env::current_exe()?)?,
        );

        let runtime = Arc::new(Self {
            automation_timers: crate::automation_timer::TimerManager::new(&config)?,
            config,
            bus,
            identity,
            memory,
            working,
            world,
            registry,
            adapters,
            model_registry,
            llm,
            engine,
            node_server,
            dispatcher,
            firmware,
            assistant,
            brain,
            update,
            coordinator_id,
            coordinator_priority,
            discovery,
            restart: Arc::new(tokio::sync::Notify::new()),
            pending_restart: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            node_event_loss_count: AtomicU64::new(0),
            node_state_reconciliation_count: AtomicU64::new(0),
            background: AsyncMutex::new(Vec::new()),
            instance_id,
            submissions: AsyncMutex::new(tokio::task::JoinSet::new()),
            model_verification: AsyncMutex::new(()),
        });
        runtime.start(start_brain).await?;
        Ok(runtime)
    }

    pub async fn model_inventory(
        &self,
        query: crate::model_inventory::InventoryQuery,
    ) -> Result<Value> {
        crate::model_inventory::list(&self.node_server, self.discovery.freshness_seconds, query)
            .await
    }

    pub async fn model_route_preview(&self, query: crate::model::RouteQuery) -> Result<Value> {
        let request = query.request()?;
        let connected = self.node_server.connected_nodes().await;
        if query
            .node_id
            .as_ref()
            .is_some_and(|id| !connected.contains_key(id))
        {
            bail!("node is not connected")
        }
        let candidates = self
            .model_registry
            .route(&request, &Default::default())
            .into_iter()
            .filter(|spec| connected.contains_key(&spec.node_id))
            .collect::<Vec<_>>();
        let matched = candidates.len();
        let models=candidates.into_iter().take(query.limit).map(|spec|json!({
            "instance":{"node_id":spec.node_id,"backend":spec.backend,"model_id":spec.model_id},
            "model_metadata":spec.model_metadata,
        })).collect::<Vec<_>>();
        Ok(
            json!({"schema_version":1,"success":true,"report_type":"model_route_preview","runtime_id":self.instance_id,
            "capability":query.capability,"role":query.role,"candidates":models,"matched_count":matched,"truncated":matched>query.limit,
            "execution_authorized":false,"note":"Current connected registry candidates in existing load/latency order, not an execution grant, model reservation or readiness check. State may change before dispatch."}),
        )
    }

    pub fn request_restart(&self) -> Value {
        let restart = self.restart.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            restart.notify_one();
        });
        json!({"schema_version":1,"success":true,"restarting":true,
            "runtime_id":self.instance_id,"restart_requested":true,
            "note":"Restart requested, not completion. Verify a new runtime ID and node reconnection."})
    }

    pub fn discovery_command(&self, request: crate::discovery::DiscoveryRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!(
                "command schema or coordinator runtime does not match; inspect current state and retry explicitly"
            )
        }
        let mut result =
            crate::discovery::command(&self.config, &self.discovery, &request.command)?;
        if result["changed"] == true {
            self.pending_restart.store(true, Ordering::Relaxed);
        }
        result["runtime_id"] = json!(self.instance_id);
        Ok(result)
    }

    pub fn discovery_status(&self) -> Result<Value> {
        self.discovery_command(crate::discovery::DiscoveryRequest {
            schema_version: 1,
            expected_runtime_id: self.instance_id.clone(),
            command: crate::discovery::DiscoveryCommand::Show,
        })
    }

    pub fn credential_command(
        &self,
        request: crate::credentials::CredentialRequest,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("credential command schema or runtime does not match")
        }
        let mut result = crate::credentials::command(
            &self.config,
            self.node_server.authentication_policy(),
            &request.command,
        )?;
        if result["changed"] == true {
            self.pending_restart.store(true, Ordering::Relaxed);
        }
        result["runtime_id"] = json!(self.instance_id);
        Ok(result)
    }

    pub fn credential_status(&self) -> Result<Value> {
        self.credential_command(crate::credentials::CredentialRequest {
            schema_version: 1,
            expected_runtime_id: self.instance_id.clone(),
            command: crate::credentials::CredentialCommand::Show,
        })
    }

    pub fn scheduling_command(
        &self,
        request: crate::scheduling::SchedulingRequest,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("scheduling command schema or runtime does not match")
        }
        let mut result = crate::scheduling::command(
            &self.config,
            self.registry.scheduling_policy(),
            &request.command,
        )?;
        if result["changed"] == true {
            self.pending_restart.store(true, Ordering::Relaxed);
        }
        result["runtime_id"] = json!(self.instance_id);
        result["admission"] = self.registry.admission_snapshot();
        Ok(result)
    }

    pub fn scheduling_status(&self) -> Result<Value> {
        self.scheduling_command(crate::scheduling::SchedulingRequest {
            schema_version: 1,
            expected_runtime_id: self.instance_id.clone(),
            command: crate::scheduling::SchedulingCommand::Show {},
        })
    }

    pub fn save_configuration(&self, mut value: Value) -> Result<()> {
        if !value.is_object() || !value["node"].is_object() {
            bail!("configuration and node settings must be objects")
        }
        self.config.edit_for_restart(|saved| {
            // Scheduling policy is owned by its typed commands too; stale
            // generic settings clients must not drop or rewrite applied limits.
            if let Some(policy) = saved.get("scheduling") {
                value["scheduling"] = policy.clone();
            } else {
                value
                    .as_object_mut()
                    .expect("validated object")
                    .remove("scheduling");
            }
            // Dedicated owner commands own authentication changes. Generic
            // settings saves must not provision/revoke/drop node credentials.
            if let Some(policy) = saved.pointer("/node/authentication") {
                value["node"]["authentication"] = policy.clone();
            } else if let Some(node) = value.get_mut("node").and_then(Value::as_object_mut) {
                node.remove("authentication");
            }
            if value.pointer("/node/psk").and_then(Value::as_str)
                == Some("__KEEP_EXISTING_SECRET__")
            {
                value["node"]["psk"] = saved["node"]["psk"].clone();
            }
            *saved = value;
            Ok(((), true))
        })?;
        self.pending_restart.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn start(self: &Arc<Self>, start_brain: bool) -> Result<()> {
        let mut background = self.background.lock().await;
        background.push(self.world.start(&self.bus));
        background.push(self.assistant.start_event_listener());
        background.push(self.start_node_event_listener());
        if let Some(task) = self.update.start(self.bus.clone()) {
            background.push(task);
        }
        drop(background);
        self.node_server.start().await?;
        if start_brain {
            self.brain.start().await;
        }
        self.initialized.store(true, Ordering::SeqCst);
        self.background
            .lock()
            .await
            .push(self.automation_timers.start(self.clone())?);
        self.bus
            .publish(
                "runtime.started",
                json!({"version": crate::VERSION, "node_port": self.node_server.port()}),
            )
            .await;
        info!(capabilities = ?self.registry.all_capabilities(), "eef runtime initialized");
        Ok(())
    }

    fn start_node_event_listener(self: &Arc<Self>) -> JoinHandle<()> {
        self.node_event_listener(self.node_server.subscribe())
    }

    fn node_event_listener(
        self: &Arc<Self>,
        mut events: tokio::sync::broadcast::Receiver<NodeEvent>,
    ) -> JoinHandle<()> {
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.reconcile_node_states().await;
            loop {
                match events.recv().await {
                    Ok(NodeEvent::Registered(message)) => {
                        if let Some(id) = message["node_id"].as_str() {
                            runtime.sync_node_state(id, true).await;
                        }
                    }
                    Ok(NodeEvent::Heartbeat(message)) => {
                        if let Some(id) = message["node_id"].as_str() {
                            if let Some(heartbeat) = runtime.node_server.node_heartbeat(id).await {
                                runtime.on_node_heartbeat(heartbeat, true).await;
                            }
                        }
                    }
                    Ok(NodeEvent::Submission(message)) => {
                        let mut submissions = runtime.submissions.lock().await;
                        while submissions.try_join_next().is_some() {}
                        if submissions.len() >= 128 {
                            let _=runtime.node_server.send_submission_reply(&message,json!({"type":"submission_result","id":message["id"],"success":false,"error":"Network is busy. This request was not started."})).await;
                            continue;
                        }
                        let worker = runtime.clone();
                        submissions.spawn(async move { worker.on_node_submission(message).await });
                    }
                    Ok(NodeEvent::Down(node_id)) => runtime.sync_node_state(&node_id, true).await,
                    Ok(NodeEvent::GrantAccepted(_)) => {} // Direct durable observers own grant checkpoints; this broadcast is informational only.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        let _ = runtime.node_event_loss_count.fetch_update(
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                            |value| Some(value.saturating_add(count)),
                        );
                        warn!(
                            count,
                            "node event listener lagged; reconciling live state without replaying submissions"
                        );
                        runtime.reconcile_node_states().await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    async fn sync_node_state(&self, node_id: &str, publish_registration: bool) {
        match self.node_server.node_state(node_id).await {
            Some(snapshot) => self.apply_node_state(snapshot, publish_registration).await,
            None => self.on_node_down(node_id, publish_registration).await,
        }
    }

    async fn apply_node_state(
        &self,
        snapshot: eefn::server::NodeStateSnapshot,
        publish_registration: bool,
    ) {
        self.on_node_register(snapshot.registration, publish_registration)
            .await;
        if let Some(heartbeat) = snapshot.heartbeat {
            self.on_node_heartbeat(heartbeat, publish_registration)
                .await;
        }
    }

    async fn reconcile_node_states(&self) {
        let snapshots = self.node_server.node_states().await;
        let mut known = self
            .registry
            .all_providers()
            .into_iter()
            .map(|p| p.node_id)
            .chain(self.model_registry.node_ids())
            .collect::<std::collections::BTreeSet<_>>();
        known.remove("local");
        for node_id in known.into_iter().filter(|id| !snapshots.contains_key(id)) {
            // Consult live state again: a reconnect may have happened while
            // prior observer updates yielded. Old events cannot undo new state.
            self.sync_node_state(&node_id, false).await;
        }
        for node_id in snapshots.keys() {
            self.sync_node_state(node_id, false).await;
        }
        let _ = self.node_state_reconciliation_count.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |value| Some(value.saturating_add(1)),
        );
        self.bus
            .publish(
                "node.state_reconciled",
                json!({"registered_node_count":snapshots.len(),
            "submissions_replayed":false}),
            )
            .await;
    }

    async fn on_node_register(&self, message: Value, publish_registration: bool) {
        let Some(node_id) = message
            .get("node_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        let name = message
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(node_id);
        let specs = node_specs(message.get("specs"));
        let capabilities = message
            .get("capabilities")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut providers = Vec::new();
        for value in &capabilities {
            let Some(capability) = value.as_str() else {
                continue;
            };
            providers.push(CapabilityProvider {
                capability: capability.into(),
                action: "*".into(),
                node_id: node_id.into(),
                node_name: name.into(),
                version: message
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or("1.0")
                    .into(),
                properties: json!({"area":message.pointer("/metadata/area"),"resources":message.pointer("/metadata/resources")}),
                node_specs: specs.clone(),
                load: 0.0,
                latency_ms: 0.0,
                priority: 0,
                healthy: true,
                in_flight: 0,
                max_concurrent: 0,
                last_heartbeat_ms: message["gateway_received_ms"].as_u64().unwrap_or(0),
            });
        }
        self.registry.refresh_node(node_id, providers);
        let models = message
            .get("models")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Err(error) = self.model_registry.register_remote(node_id, models) {
            warn!(%error, "invalid registered model snapshot");
        }
        self.publish_or_observe_node(
                    "node.connected",
                    json!({"node_id": node_id, "name": name, "capabilities": capabilities,"specs":message.get("specs"),"models":message.get("models"),"metadata":message.get("metadata"),"network":message.get("network")}),
                    publish_registration,
                )
                .await;
    }

    async fn publish_or_observe_node(&self, topic: &str, data: Value, publish: bool) {
        if publish {
            self.bus.publish(topic, data).await;
        } else {
            // State repair must not replay connection/metric triggers into
            // automation. Update the observer directly without a bus callback.
            self.world
                .apply(crate::event::Event {
                    topic: topic.into(),
                    data,
                    timestamp_ms: eefn::protocol::now_ms(),
                })
                .await;
        }
    }

    async fn on_node_heartbeat(&self, message: Value, publish: bool) {
        let Some(node_id) = message
            .get("node_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        let load = message.get("load").cloned().unwrap_or_else(|| json!({}));
        let combined = load
            .get("cpu")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(load.get("ram").and_then(Value::as_f64).unwrap_or(0.0));
        let latency = message
            .get("latency_ms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let specs = message.get("specs").map(|value| node_specs(Some(value)));
        self.registry.update_metrics_at(
            node_id,
            combined,
            latency,
            specs,
            message["gateway_received_ms"].as_u64().unwrap_or(0),
        );
        self.model_registry
            .update_metrics(node_id, combined, latency);
        self.publish_or_observe_node(
            "node.updated",
            json!({"node_id": node_id, "load": combined, "latency_ms": latency}),
            publish,
        )
        .await;
    }

    async fn on_node_down(&self, node_id: &str, publish: bool) {
        self.registry.unregister_node(node_id);
        self.model_registry.remove_node(node_id);
        self.publish_or_observe_node("node.disconnected", json!({"node_id": node_id}), publish)
            .await;
    }

    pub(crate) async fn disclosed_interpretation_capabilities(
        &self,
        origin: &str,
    ) -> Result<Vec<String>> {
        self.discovery
            .interpretation_capabilities(origin, &self.node_server)
            .await
    }
    pub(crate) async fn disclosed_planning_nodes(
        &self,
        origin: &str,
    ) -> Result<std::collections::BTreeSet<String>> {
        self.discovery
            .planning_nodes(origin, &self.node_server)
            .await
    }

    pub(crate) async fn disclosed_audio_endpoint(
        &self,
        origin: &str,
        node: &str,
    ) -> Result<eefn::audio_submission::EndpointResources> {
        if !self.disclosed_planning_nodes(origin).await?.contains(node) {
            anyhow::bail!("Audio node unavailable or not authorized")
        }
        self.node_server
            .audio_authoring_resources(node, self.discovery.freshness_seconds)
            .await
    }

    async fn on_node_submission(&self, message: Value) {
        let Some(node_id) = message
            .get("node_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        let Some(id) = message.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return;
        };
        let context: eefn::context::RequestContext =
            match serde_json::from_value(message["request_context"].clone()) {
                Ok(context) => context,
                Err(error) => {
                    warn!(%error,"submission missing authenticated origin");
                    return;
                }
            };
        self.bus
            .publish(
                "node.submission",
                json!({"node_id": node_id, "id": id, "kind": message.get("kind"),"request_context":context}),
            )
            .await;
        let kind = message
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("message");
        let result = match kind {
            "automations.command" => {
                match serde_json::from_value::<eefn::automation::AutomationCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command)
                        if command
                            .receipt()
                            .is_none_or(|id| id == context.request_id()) =>
                    {
                        self.engine
                            .store
                            .automation_command(command, Some(context.origin_node()))
                    }
                    _ => Err(anyhow::anyhow!(
                        "Invalid automation command or receipt; no timer or execution"
                    )),
                }
            }
            "message" => Err(anyhow::anyhow!(
                "Legacy message execution is retired. Interpret through the running node and save a reviewed request; no keyword, model or workload fallback."
            )),
            "requests.review" => {
                match self
                    .discovery
                    .interpretation_capabilities(context.origin_node(), &self.node_server)
                    .await
                {
                    Ok(capabilities) => crate::request_review::review(
                        message["payload"].clone(),
                        &context,
                        &capabilities,
                    ),
                    Err(error) => Err(error),
                }
            }
            "requests.open"
            | "requests.draft_get"
            | "requests.draft_answer"
            | "requests.draft_revise"
            | "requests.draft_resolve" => {
                // Inspection/answers do not need currently available executors.
                // Only a new/revised proposal needs a fresh network vocabulary.
                let capabilities = if matches!(
                    kind,
                    "requests.open" | "requests.draft_revise" | "requests.draft_resolve"
                ) {
                    self.discovery
                        .interpretation_capabilities(context.origin_node(), &self.node_server)
                        .await
                } else {
                    Ok(Vec::new())
                };
                match capabilities {
                    Ok(capabilities) if kind == "requests.open" => crate::request_drafts::open(
                        &self.engine,
                        message["payload"].clone(),
                        &context,
                        &capabilities,
                    ),
                    Ok(capabilities) => crate::request_drafts::command(
                        &self.engine,
                        kind,
                        message["payload"].clone(),
                        &context,
                        &capabilities,
                    ),
                    Err(error) => Err(error),
                }
            }
            "requests.capabilities" => {
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                match serde_json::from_value::<Empty>(message["payload"].clone()) {
                    Ok(_) => self
                        .discovery
                        .interpretation_capabilities(context.origin_node(), &self.node_server)
                        .await
                        .map(|capabilities| {
                            json!(eefn::interpretation::CapabilityContext {
                                schema_version: 1,
                                report_type: "coordinator_interpretation_context".into(),
                                request_context: context.clone(),
                                capabilities,
                                freshness_seconds: self.discovery.freshness_seconds,
                                execution_authorized: false,
                                direct_access_authorized: false,
                            })
                        }),
                    Err(_) => Err(anyhow::anyhow!("Invalid interpretation context query")),
                }
            }
            "requests.planning" => {
                match serde_json::from_value::<eefn::request_planning::PlanningCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) => self.origin_planning_command(command, &context).await,
                    Err(_) => Err(anyhow::anyhow!(
                        "Invalid origin planning command; approval is not available"
                    )),
                }
            }
            "requests.preparation_checkpoint" => {
                match serde_json::from_value::<eefn::request_preparation::PreparationCheckpoint>(
                    message["payload"].clone(),
                ) {
                    Ok(checkpoint) => {
                        self.origin_preparation_checkpoint(checkpoint, &context)
                            .await
                    }
                    Err(_) => Err(anyhow::anyhow!(
                        "Invalid source-bound preparation checkpoint"
                    )),
                }
            }
            "requests.audio" => match serde_json::from_value::<eefn::audio_submission::AudioInput>(
                message["payload"].clone(),
            ) {
                Ok(input) => self.origin_audio_request(input, &context).await,
                Err(_) => Err(anyhow::anyhow!("Invalid typed audio source")),
            },
            "requests.concrete" => {
                match serde_json::from_value::<eefn::request_submission::ConcreteInput>(
                    message["payload"].clone(),
                ) {
                    Ok(input) => self.origin_concrete_request(input, &context).await,
                    Err(_) => Err(anyhow::anyhow!("Invalid caller-supplied concrete request")),
                }
            }
            "network.peers" => {
                self.discovery
                    .view(
                        context.origin_node(),
                        message["payload"].clone(),
                        &self.node_server,
                    )
                    .await
            }
            "requests.abandon" => {
                match serde_json::from_value::<eefn::request_abandon::AbandonCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) if command.operation_id == context.request_id() => {
                        self.abandon_request(command, Some(context.origin_node()))
                    }
                    _ => Err(anyhow::anyhow!(
                        "Abandonment receipt does not match authenticated origin"
                    )),
                }
            }
            "requests.stop_receipt" => {
                match serde_json::from_value::<eefn::request_stop_receipt::Command>(
                    message["payload"].clone(),
                ) {
                    Ok(command) => {
                        self.inspect_request_stop_receipt(command, Some(context.origin_node()))
                            .await
                    }
                    Err(_) => Err(anyhow::anyhow!("Invalid stop inspection command")),
                }
            }
            "requests.receipt" => {
                match serde_json::from_value::<eefn::request_receipt::ReceiptCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) => {
                        self.inspect_request_receipt(command, Some(context.origin_node()))
                            .await
                    }
                    Err(_) => Err(anyhow::anyhow!("Invalid receipt query")),
                }
            }
            "requests.result" => {
                match serde_json::from_value::<eefn::request_result::ResultCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) => {
                        self.inspect_request_result(command, Some(context.origin_node()))
                            .await
                    }
                    Err(_) => Err(anyhow::anyhow!("Invalid retained result query")),
                }
            }
            "requests.recover_result" => {
                match serde_json::from_value::<eefn::request_recovery::RecoverCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) if command.operation_id == context.request_id() => {
                        self.recover_request(command, Some(context.origin_node()))
                            .await
                    }
                    _ => Err(anyhow::anyhow!(
                        "Recovery receipt does not match authenticated origin"
                    )),
                }
            }
            "requests.execute" => {
                match serde_json::from_value::<eefn::request_execution::ExecuteCommand>(
                    message["payload"].clone(),
                ) {
                    Ok(command) if command.operation_id == context.request_id() => {
                        self.execute_reviewed(command, Some(context.origin_node()))
                            .await
                    }
                    _ => Err(anyhow::anyhow!(
                        "Execution receipt does not match authenticated origin context"
                    )),
                }
            }
            kind if kind.starts_with("jobs.") => {
                self.engine
                    .job_request(kind, message["payload"].clone(), Some(context))
            }
            kind => Err(anyhow::anyhow!("unsupported submission kind '{kind}'")),
        };
        let response = match result {
            Ok(data) => {
                json!({"type": "submission_result", "id": id, "success": true, "data": data})
            }
            Err(error) => json!({
                "type": "submission_result", "id": id, "success": false,
                "error": error.to_string(), "error_type": "SubmissionError"
            }),
        };
        if let Err(error) = self
            .node_server
            .send_submission_reply(&message, response)
            .await
        {
            warn!(%node_id, %error, "could not return submission result to originating node");
        }
    }

    pub async fn handle_message(&self, _message: &str) -> Result<String> {
        anyhow::bail!(
            "Submit input through the running node. Coordinator chat has no authenticated origin and cannot plan or execute work."
        )
    }

    pub async fn diagnostics(&self) -> Value {
        let connections = self.node_server.connection_counts().await;
        let world_event_loss_count = self.world.event_loss_count().await;
        let (remote_providers, healthy_remote_providers) = self.registry.remote_provider_counts();
        json!({"schema_version":1,"success":true,"report_type":"coordinator_diagnostics",
            "version":crate::VERSION,"runtime_id":self.instance_id,
            "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
            "connected_node_count":connections.connected_node_count,
            "registered_node_count":connections.registered_node_count,
            "registration_pending_node_count":connections.registration_pending_node_count,
            "node_event_loss_count":self.node_event_loss_count.load(Ordering::Relaxed),
            "node_state_reconciliation_count":self.node_state_reconciliation_count.load(Ordering::Relaxed),
            "assistant_event_loss_count":self.assistant.event_loss_count(),
            "world_event_loss_count":world_event_loss_count,
            "world_state_may_be_stale":world_event_loss_count > 0,
            "world_rules_suspended":world_event_loss_count > 0,
            "node_state_recovery_note":"Reconciliation restores live registration/metrics after missed gateway events. Lost submissions are not replayed; these counters do not confirm work completion.",
            "remote_provider_count":remote_providers,
            "healthy_remote_provider_count":healthy_remote_providers,
            "readiness_note":"Gateway, provider and model counts are separate snapshots, not a readiness guarantee. Connected means authenticated; registered means the gateway accepted registration. Check the required capability, model and permissions before work; execution can still fail.",
            "model_count":self.model_registry.list().len(),
            "initialized":self.initialized.load(Ordering::Relaxed),
            "pending_restart":self.pending_restart.load(Ordering::Relaxed),
            "privacy":"Runtime ID included for correlation; no automatic upload."})
    }

    /// Explicit owner-requested ping only. Never runs shell, models or device I/O.
    pub async fn diagnostic_probe(&self, node_id: &str, samples: usize) -> Result<Value> {
        eefn::network::validate_node_id(node_id)?;
        if !(1..=10).contains(&samples) {
            bail!("diagnostic samples must be 1-10")
        }
        if !self
            .node_server
            .connected_nodes()
            .await
            .contains_key(node_id)
        {
            bail!("node is not connected")
        }
        let mut results = Vec::new();
        for _ in 0..samples {
            let started = std::time::Instant::now();
            let response = self
                .node_server
                .invoke_remote(
                    node_id,
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(2),
                )
                .await;
            let valid = response.as_ref().is_ok_and(|v| {
                v["success"] == true && v.pointer("/data/pong") == Some(&json!(true))
            });
            let version = response
                .as_ref()
                .ok()
                .and_then(|v| v.pointer("/data/version"))
                .and_then(Value::as_str)
                .filter(|s| {
                    s.len() <= 64
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
                });
            results.push(json!({"success":valid,"round_trip_ms":started.elapsed().as_secs_f64()*1000.0,
                "node_version":version,"matches_coordinator_version":version==Some(crate::VERSION)}));
        }
        let passed = results.iter().filter(|v| v["success"] == true).count();
        Ok(
            json!({"schema_version":1,"success":passed==samples,"report_type":"node_ping_probe",
            "coordinator_version":crate::VERSION,"coordinator_runtime_id":self.instance_id,
            "node_id":node_id,"samples":results,"passed":passed,"failed":samples-passed,
            "scope":"Authenticated system.ping only; no inference or filesystem/device test."}),
        )
    }

    pub async fn summary(&self) -> Value {
        let connected_nodes = self.node_server.connected_nodes().await.len();
        let update = self.update.state().await;
        json!({
            "pending_restart": self.pending_restart.load(Ordering::Relaxed) || update.restart_required,
            "update": update,
            "name": self.identity.name(), "version": crate::VERSION,
            "runtime": "rust", "runtime_id": self.instance_id, "python_plugins": self.config.strings("python.plugins"),
            "initialized": self.initialized.load(Ordering::Relaxed), "brain_alive": self.brain.alive(),
            "adapters": self.adapters.capabilities(), "capabilities": self.registry.all_capabilities(),
            "models": self.model_registry.list(), "tiers": model_tiers(&self.config), "world": self.world.summary().await,
            "operational": connected_nodes > 0,
            "coordinator": {
                "id": self.coordinator_id,
                "priority": self.coordinator_priority,
                "state": if connected_nodes > 0 { "active" } else { "waiting_for_node" },
                "connected_nodes": connected_nodes,
            },
        })
    }

    pub async fn shutdown(&self) {
        if !self.initialized.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Err(error) = self.automation_timers.close(&self.engine.store) {
            warn!(%error,"Schedule shutdown checkpoint failed; no cancellation proof");
        }
        self.brain.stop().await;
        for task in self.background.lock().await.drain(..) {
            task.abort();
            let _ = task.await;
        }
        self.submissions.lock().await.shutdown().await;
        // Checkpoint in-flight jobs as interrupted before closing their transport.
        // Otherwise gateway shutdown can win the race and persist "failed" for
        // a remote action whose real outcome is unknown (and may still finish).
        self.engine.shutdown().await;
        self.node_server.stop().await;
        self.adapters.shutdown().await;
        self.bus.publish("runtime.stopped", json!({})).await;
    }
    /// Consuming process-lifecycle boundary after HTTP draining. Shutdown closes
    /// admission, but detached final drops may still own the old journal lease.
    /// Do not initialize another runtime until that exact ownership has retired.
    pub async fn retire(self: Arc<Self>, budget: Duration) -> Result<()> {
        self.shutdown().await;
        let journal = self.engine.store.retirement();
        drop(self);
        journal.wait(budget).await
    }
}

pub(crate) fn validate_secret(psk: &str) -> Result<()> {
    if psk.len() < 12 || psk.eq_ignore_ascii_case("change-me-eef") || psk.contains("CHANGE_ME") {
        bail!("set EEF_NODE_PSK to a non-placeholder secret of at least 12 characters")
    }
    Ok(())
}

fn firmware_repository(config: &Config, layout: &eefn::runtime_layout::RuntimeLayout) -> PathBuf {
    let path = PathBuf::from(config.string("firmware.repository", "data/firmware"));
    if path == Path::new("data/firmware") {
        layout.default_data_directory(
            config.source().unwrap_or_else(|| Path::new("")),
            Path::new("config/default_identity.yaml"),
            &path,
        )
    } else {
        path
    }
}

pub(crate) fn model_tiers(config: &Config) -> BTreeMap<String, Vec<String>> {
    config
        .object("models.tiers")
        .into_iter()
        .map(|(tier, values)| {
            let models = values
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            (tier, models)
        })
        .collect()
}

fn node_specs(value: Option<&Value>) -> NodeSpecs {
    value
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn default_coordinator_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "host".into());
    format!("eef-{}", host.to_lowercase())
}

fn default_rules() -> Vec<ResponseRule> {
    vec![
        ResponseRule::text(
            "greeting",
            "hello",
            "Hi! I'm eef — I'm running and watching.",
            10.0,
        ),
        ResponseRule::text(
            "acknowledge_remember",
            "remember",
            "Got it — I've written that to memory.",
            5.0,
        ),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn managed_stock_firmware_setting_uses_installation_but_custom_paths_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("installation");
        std::fs::create_dir_all(root.join("config")).unwrap();
        let executable = root.join("eef.exe");
        std::fs::write(&executable, b"fixture, never execute").unwrap();
        std::fs::write(root.join("bundle.json"), b"{}").unwrap();
        let layout = eefn::runtime_layout::RuntimeLayout::from_executable(executable).unwrap();
        let path = root.join("config/default_identity.yaml");
        std::fs::write(&path, b"{}").unwrap();
        let config = crate::config::Config::load(Some(&path)).unwrap();
        assert_eq!(
            super::firmware_repository(&config, &layout),
            root.canonicalize().unwrap().join("data/firmware")
        );
        std::fs::write(&path, b"firmware:\n  repository: owner/firmware\n").unwrap();
        let custom = crate::config::Config::load(Some(&path)).unwrap();
        assert_eq!(
            super::firmware_repository(&custom, &layout),
            std::path::Path::new("owner/firmware")
        );
        let external = dir.path().join("owner.yaml");
        std::fs::write(&external, b"{}").unwrap();
        let custom = crate::config::Config::load(Some(&external)).unwrap();
        assert_eq!(
            super::firmware_repository(&custom, &layout),
            std::path::Path::new("data/firmware")
        );
    }
    use super::*;
    #[tokio::test]
    async fn consumed_runtime_retirement_waits_for_independent_store_clones() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        let database = dir.path().join("memory.db");
        std::fs::write(&config, serde_json::to_vec(&json!({
            "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false}},
            "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
        })).unwrap()).unwrap();
        let runtime = Runtime::initialize(Config::load(Some(&config)).unwrap(), &database, false)
            .await
            .unwrap();
        let held = runtime.engine.store.clone();
        let weak = Arc::downgrade(&runtime);
        let retire = tokio::spawn(runtime.retire(Duration::from_secs(3)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while weak.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !retire.is_finished(),
            "runtime drop alone is not database retirement"
        );
        assert!(crate::jobs::JobStore::open(&database, 500, 64 * 1024 * 1024).is_err());
        drop(held);
        retire.await.unwrap().unwrap();
        let replacement =
            Runtime::initialize(Config::load(Some(&config)).unwrap(), &database, false)
                .await
                .unwrap();
        replacement.retire(Duration::from_secs(3)).await.unwrap();
    }
    use eefn::{
        NodeCrypto,
        protocol::{build_auth, read_message, write_message},
    };
    use tokio::{
        io::BufReader,
        net::{
            TcpStream,
            tcp::{OwnedReadHalf, OwnedWriteHalf},
        },
        time::timeout,
    };

    async fn connect(
        runtime: &Runtime,
        crypto: &NodeCrypto,
        id: &str,
    ) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
        let stream = TcpStream::connect(("127.0.0.1", runtime.node_server.port()))
            .await
            .unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        write_message(crypto, &mut writer, &build_auth(crypto, id, eefn::VERSION))
            .await
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(5), read_message(crypto, &mut reader))
                .await
                .unwrap()
                .unwrap()
                .unwrap()["type"],
            "ok"
        );
        (reader, writer)
    }
    async fn wait_event(
        events: &mut tokio::sync::broadcast::Receiver<NodeEvent>,
        matches: impl Fn(&NodeEvent) -> bool,
    ) {
        timeout(Duration::from_secs(5), async {
            loop {
                match events.recv().await {
                    Ok(event) if matches(&event) => break,
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => panic!("{error}"),
                }
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn credential_only_runtime_initializes_without_legacy_secret_or_implicit_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, serde_json::to_vec(&json!({
            "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false}},
            "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
        })).unwrap()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let runtime = Runtime::initialize(
            Config::load(Some(&path)).unwrap(),
            dir.path().join("memory.db"),
            false,
        )
        .await
        .unwrap();
        let policy = runtime.node_server.authentication_policy();
        assert!(!policy.allow_legacy);
        assert!(policy.credentials.is_empty());
        assert!(policy.tls.is_none());
        assert_eq!(runtime.config.string("node.psk", "missing"), "");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "no dummy key, TLS identity or credentials may be generated by this change"
        );
        runtime.shutdown().await;
        for invalid in ["", "short", "change-me-eef", "prefix_CHANGE_ME_suffix"] {
            assert!(validate_secret(invalid).is_err());
        }
        assert!(validate_secret("fixture-legacy-secret").is_ok());
    }
    #[tokio::test]
    async fn cached_state_reconciliation_does_not_renew_scheduler_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "node":{"psk":"isolated-test-secret","host":"127.0.0.1","port":0},
                "update":{"policy":"off"},"python":{"plugins":[]},
                "firmware":{"repository":dir.path().join("firmware")}
            }))
            .unwrap(),
        )
        .unwrap();
        let runtime = Runtime::initialize(
            Config::load(Some(&path)).unwrap(),
            dir.path().join("memory.db"),
            false,
        )
        .await
        .unwrap();
        let received = eefn::protocol::now_ms() - 60_000;
        let snapshot = eefn::server::NodeStateSnapshot {
            registration: json!({"node_id":"fixture","name":"Fixture","capabilities":["system.info"],
                "gateway_received_ms":received}),
            heartbeat: Some(json!({"node_id":"fixture","gateway_received_ms":received+1,
                "load":{"cpu":0.3},"latency_ms":5})),
        };
        for _ in 0..3 {
            runtime.apply_node_state(snapshot.clone(), false).await;
        }
        let provider = runtime.registry.find("system.info", "run")[0].clone();
        assert_eq!(provider.last_heartbeat_ms, received + 1);
        assert!(
            runtime
                .registry
                .find_best("system.info", "run", &json!({}))
                .is_none()
        );
        assert!(
            runtime
                .registry
                .reserve_best_with_context("system.info", "run", &json!({}), None)
                .is_none()
        );
        runtime
            .on_node_heartbeat(
                json!({"node_id":"fixture","gateway_received_ms":eefn::protocol::now_ms()}),
                false,
            )
            .await;
        assert!(
            runtime
                .registry
                .find_best("system.info", "run", &json!({}))
                .is_some()
        );
        assert!(runtime.engine.list_plans().unwrap().is_empty());
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn lagged_gateway_events_recover_live_providers_models_and_metrics_without_replay() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let config = json!({"node":{"psk":"isolated-test-secret","host":"127.0.0.1","port":0},
            "update":{"policy":"off"},"python":{"plugins":[]},
            "firmware":{"repository":dir.path().join("firmware")}});
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let config = Config::load(Some(&config_path)).unwrap();
        let secret = std::env::var("EEF_NODE_PSK")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "isolated-test-secret".into());
        let crypto = NodeCrypto::new(secret).unwrap();
        let runtime = Runtime::initialize(config, dir.path().join("memory.db"), false)
            .await
            .unwrap();
        let mut observer = runtime.node_server.subscribe();
        let (a_reader, mut a_writer) = connect(&runtime, &crypto, "node-a").await;
        write_message(
            &crypto,
            &mut a_writer,
            &json!({"type":"register","node_id":"node-a","capabilities":["system.info"],
            "models":[{"model_id":"old-model","modality":"text","backend":"ollama"}]}),
        )
        .await
        .unwrap();
        wait_event(
            &mut observer,
            |e| matches!(e, NodeEvent::Registered(v) if v["node_id"] == "node-a"),
        )
        .await;
        timeout(Duration::from_secs(5), async {
            while !runtime
                .registry
                .all_providers()
                .iter()
                .any(|p| p.node_id == "node-a")
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let listener = runtime.background.lock().await.remove(2);
        listener.abort();
        let _ = listener.await;
        let queued = runtime.node_server.subscribe();
        runtime.registry.register(CapabilityProvider::local(
            "fixture.local",
            "read",
            json!({}),
        ));
        runtime
            .model_registry
            .update_metrics("metrics-only-disconnected", 0.4, 12.0);
        let (mut b_reader, mut b_writer) = connect(&runtime, &crypto, "node-b").await;
        write_message(
            &crypto,
            &mut b_writer,
            &json!({"type":"register","node_id":"node-b","capabilities":["system.info"],
            "models":[{"model_id":"new-model","modality":"text","backend":"ollama"}]}),
        )
        .await
        .unwrap();
        wait_event(
            &mut observer,
            |e| matches!(e, NodeEvent::Registered(v) if v["node_id"] == "node-b"),
        )
        .await;
        drop(a_reader);
        drop(a_writer);
        wait_event(
            &mut observer,
            |e| matches!(e, NodeEvent::Down(id) if id == "node-a"),
        )
        .await;
        // These submissions are deliberately lost, and must never be recovered
        // from state snapshots. No command execution or automatic replay.
        write_message(&crypto, &mut b_writer, &json!({"type":"submit","node_id":"node-b","id":"lost-request","kind":"jobs.list","payload":{}})).await.unwrap();
        for sequence in 0..300 {
            write_message(
                &crypto,
                &mut b_writer,
                &json!({"type":"heartbeat","node_id":"node-b","sequence":sequence,
                "load":{"cpu":0.73,"ram":0.21},"latency_ms":9}),
            )
            .await
            .unwrap();
        }
        wait_event(
            &mut observer,
            |e| matches!(e, NodeEvent::Heartbeat(v) if v["sequence"] == 299),
        )
        .await;
        let reconciliations_before = runtime
            .bus
            .history(500)
            .await
            .iter()
            .filter(|event| event.topic == "node.state_reconciled")
            .count();
        let listener = runtime.node_event_listener(queued);
        runtime.background.lock().await.push(listener);
        let recovered = timeout(Duration::from_secs(3), async {
            loop {
                let providers = runtime.registry.all_providers();
                let models = runtime.model_registry.list();
                if !providers.iter().any(|p| p.node_id == "node-a")
                    && providers
                        .iter()
                        .any(|p| p.node_id == "node-b" && p.load == 0.73 && p.latency_ms == 9.0)
                    && !models.iter().any(|m| m.node_id == "node-a")
                    && models
                        .iter()
                        .any(|m| m.node_id == "node-b" && m.model_id == "new-model")
                    && !runtime
                        .model_registry
                        .node_ids()
                        .contains("metrics-only-disconnected")
                    && runtime
                        .bus
                        .history(500)
                        .await
                        .iter()
                        .filter(|event| event.topic == "node.state_reconciled")
                        .count()
                        >= reconciliations_before + 2
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        let world = runtime.world.summary().await;
        assert!(
            world["devices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["node_id"] == "node-a" && node["connected"] == false)
        );
        assert!(
            world["devices"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["node_id"] == "node-b" && node["connected"] == true)
        );
        assert!(
            !runtime
                .bus
                .history(500)
                .await
                .iter()
                .any(|event| event.topic == "node.connected" && event.data["node_id"] == "node-b"),
            "repair replayed a connection trigger"
        );
        assert!(
            runtime
                .registry
                .all_providers()
                .iter()
                .any(|p| p.node_id == "local" && p.capability == "fixture.local")
        );
        let mut provider = runtime
            .registry
            .all_providers()
            .into_iter()
            .find(|p| p.node_id == "node-b")
            .unwrap();
        provider.in_flight = 2;
        provider.max_concurrent = 4;
        provider.priority = 7;
        runtime.registry.register(provider);
        // An obsolete disconnect signal consults current state, rather than
        // deleting a newly registered session. Scheduler-owned counts survive.
        runtime.sync_node_state("node-b", true).await;
        let provider = runtime
            .registry
            .all_providers()
            .into_iter()
            .find(|p| p.node_id == "node-b")
            .unwrap();
        assert_eq!(
            (
                provider.in_flight,
                provider.max_concurrent,
                provider.priority
            ),
            (2, 4, 7)
        );
        let replay = timeout(
            Duration::from_millis(50),
            read_message(&crypto, &mut b_reader),
        )
        .await;
        drop(b_reader);
        drop(b_writer);
        runtime.shutdown().await;
        assert!(
            recovered.is_ok(),
            "event loss left stale/missing scheduler state"
        );
        assert!(runtime.node_event_loss_count.load(Ordering::Relaxed) > 0);
        assert!(
            runtime
                .node_state_reconciliation_count
                .load(Ordering::Relaxed)
                >= 3
        );
        assert!(
            replay.is_err(),
            "lost submission was automatically replayed"
        );
    }
}
