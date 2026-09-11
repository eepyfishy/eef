use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    instance_id: String,
    pub restart: Arc<tokio::sync::Notify>,
    pub pending_restart: AtomicBool,
    initialized: AtomicBool,
    background: AsyncMutex<Vec<JoinHandle<()>>>,
    submissions: AsyncMutex<tokio::task::JoinSet<()>>,
}

impl Runtime {
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
        validate_secret(&psk)?;
        let host = config.string("node.host", "0.0.0.0");
        let port = u16::try_from(config.u64("node.port", eefn::DEFAULT_PORT.into()))
            .context("node.port exceeds 65535")?;
        let node_server = NodeServer::new(&psk, host, port)?;

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
        let registry = CapabilityRegistry::default();
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
        let firmware = FirmwareService::open(
            PathBuf::from(config.string("firmware.repository", "data/firmware")),
            node_server.clone(),
            firmware_defaults,
        )?;
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
            background: AsyncMutex::new(Vec::new()),
            instance_id: uuid::Uuid::new_v4().to_string(),
            submissions: AsyncMutex::new(tokio::task::JoinSet::new()),
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

    pub fn save_configuration(&self, mut value: Value) -> Result<()> {
        self.config.edit_for_restart(|saved| {
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
        self.bus
            .publish(
                "runtime.started",
                json!({"version": crate::VERSION, "node_port": self.node_server.port()}),
            )
            .await;
        info!(capabilities = ?self.registry.all_capabilities(), "EEF runtime initialized");
        Ok(())
    }

    fn start_node_event_listener(self: &Arc<Self>) -> JoinHandle<()> {
        let runtime = self.clone();
        let mut events = self.node_server.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(NodeEvent::Registered(message)) => runtime.on_node_register(message).await,
                    Ok(NodeEvent::Heartbeat(message)) => runtime.on_node_heartbeat(message).await,
                    Ok(NodeEvent::Submission(message)) => {
                        let mut submissions = runtime.submissions.lock().await;
                        while submissions.try_join_next().is_some() {}
                        if submissions.len() >= 128 {
                            if let Some(node_id) = message["node_id"].as_str() {
                                let _=runtime.node_server.send_to_node(node_id,json!({"type":"submission_result","id":message["id"],"success":false,"error":"Network is busy. This request was not started."})).await;
                            }
                            continue;
                        }
                        let worker = runtime.clone();
                        submissions.spawn(async move { worker.on_node_submission(message).await });
                    }
                    Ok(NodeEvent::Down(node_id)) => runtime.on_node_down(&node_id).await,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        warn!(count, "node event listener lagged")
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    async fn on_node_register(&self, message: Value) {
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
        self.registry.unregister_node(node_id);
        for value in &capabilities {
            let Some(capability) = value.as_str() else {
                continue;
            };
            self.registry.register(CapabilityProvider {
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
                last_heartbeat_ms: eefn::protocol::now_ms(),
            });
        }
        if let Some(models) = message.get("models").and_then(Value::as_array) {
            self.model_registry.register_remote(node_id, models);
        }
        self.bus
            .publish(
                "node.connected",
                json!({"node_id": node_id, "name": name, "capabilities": capabilities,"specs":message.get("specs"),"models":message.get("models"),"metadata":message.get("metadata"),"network":message.get("network")}),
            )
            .await;
    }

    async fn on_node_heartbeat(&self, message: Value) {
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
        self.registry
            .update_metrics(node_id, combined, latency, specs);
        self.model_registry
            .update_metrics(node_id, combined, latency);
        self.bus
            .publish(
                "node.updated",
                json!({"node_id": node_id, "load": combined, "latency_ms": latency}),
            )
            .await;
    }

    async fn on_node_down(&self, node_id: &str) {
        self.registry.unregister_node(node_id);
        self.model_registry.remove_node(node_id);
        self.bus
            .publish("node.disconnected", json!({"node_id": node_id}))
            .await;
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
        let result = match message
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("message")
        {
            "message" => {
                let text = message
                    .pointer("/payload/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.brain
                    .handle_message_with_context(text, Some(context.clone()))
                    .await
                    .map(|reply| json!({"reply": reply,"request_context":context}))
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
        if let Err(error) = self.node_server.send_to_node(&node_id, response).await {
            warn!(%node_id, %error, "could not return submission result to originating node");
        }
    }

    pub async fn handle_message(&self, message: &str) -> Result<String> {
        self.brain.handle_message(message).await
    }

    pub async fn diagnostics(&self) -> Value {
        json!({"schema_version":1,"success":true,"report_type":"coordinator_diagnostics",
            "version":crate::VERSION,"runtime_id":self.instance_id,
            "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,
            "connected_node_count":self.node_server.connected_nodes().await.len(),
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
        self.brain.stop().await;
        self.node_server.stop().await;
        for task in self.background.lock().await.drain(..) {
            task.abort();
            let _ = task.await;
        }
        self.submissions.lock().await.shutdown().await;
        self.engine.shutdown().await;
        self.adapters.shutdown().await;
        self.bus.publish("runtime.stopped", json!({})).await;
    }
}

fn validate_secret(psk: &str) -> Result<()> {
    if psk.len() < 12 || psk.eq_ignore_ascii_case("change-me-eef") || psk.contains("CHANGE_ME") {
        bail!("set EEF_NODE_PSK to a non-placeholder secret of at least 12 characters")
    }
    Ok(())
}

fn model_tiers(config: &Config) -> BTreeMap<String, Vec<String>> {
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
            "Hi! I'm EEF — I'm running and watching.",
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
