use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sysinfo::{Disks, System};
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use uuid::Uuid;

use crate::NodeCrypto;
use crate::engine::NodeEngine;
use crate::protocol::{
    build_auth, build_heartbeat, build_register, error_response, read_message, success_response,
    write_message,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedModel {
    pub model_id: String,
    #[serde(default = "text_modality")]
    pub modality: String,
    #[serde(
        default,
        rename = "selection",
        skip_serializing_if = "crate::model_selection::ModelHints::is_empty"
    )]
    pub hints: crate::model_selection::ModelHints,
}

impl SelectedModel {
    pub fn validate(&self) -> Result<()> {
        crate::model_selection::validate_id(&self.model_id)?;
        if !matches!(self.modality.as_str(), "text" | "vlm") {
            bail!("model modality must be text or vlm")
        }
        self.selection_metadata()?;
        Ok(())
    }
    pub fn selection_metadata(&self) -> Result<crate::model_metadata::ModelMetadata> {
        self.hints.metadata(&self.modality)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoordinatorEndpoint {
    pub address: String,
    pub priority: i32,
}

impl CoordinatorEndpoint {
    pub fn new(address: impl Into<String>, priority: i32) -> Self {
        Self {
            address: address.into(),
            priority,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CoordinatorEndpointDef {
    Address(String),
    Detailed {
        #[serde(alias = "endpoint", alias = "url")]
        address: String,
        #[serde(default)]
        priority: i32,
    },
}

impl<'de> Deserialize<'de> for CoordinatorEndpoint {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match CoordinatorEndpointDef::deserialize(deserializer)? {
            CoordinatorEndpointDef::Address(address) => Self::new(address, 0),
            CoordinatorEndpointDef::Detailed { address, priority } => Self::new(address, priority),
        })
    }
}

fn text_modality() -> String {
    "text".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeClientConfig {
    #[serde(default)]
    pub network: crate::network::NetworkAdvertisement,
    #[serde(default)]
    pub metadata: crate::context::NodeMetadata,
    pub endpoints: Vec<CoordinatorEndpoint>,
    pub node_id: String,
    #[serde(default)]
    pub name: String,
    pub psk: String,
    #[serde(default = "default_heartbeat")]
    pub heartbeat_seconds: f64,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
    #[serde(default)]
    pub python: Option<PathBuf>,
    #[serde(default)]
    pub python_plugins: Vec<PathBuf>,
    #[serde(default)]
    pub update_manifest: Option<String>,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default)]
    pub ollama_models: Vec<SelectedModel>,
}

fn default_heartbeat() -> f64 {
    3.0
}

fn default_ollama_url() -> String {
    "http://127.0.0.1:11434".into()
}

pub struct NodeClient {
    config: NodeClientConfig,
    crypto: NodeCrypto,
    engine: Arc<NodeEngine>,
    http: reqwest::Client,
    status: Arc<std::sync::Mutex<Value>>,
    submissions: Arc<crate::submission::SubmissionMailbox>,
}

struct AbortTask(tokio::task::JoinHandle<()>);
impl Drop for AbortTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl NodeClient {
    pub fn new(mut config: NodeClientConfig, engine: Arc<NodeEngine>) -> Result<Self> {
        config.metadata.validate()?;
        config.network.validate()?;
        if config.endpoints.is_empty() {
            bail!("at least one coordinator endpoint is required")
        }
        if config.node_id.trim().is_empty() {
            bail!("node_id must not be empty")
        }
        for endpoint in &config.endpoints {
            parse_endpoint(&endpoint.address)?;
        }
        config.endpoints.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.address.cmp(&right.address))
        });
        let mut model_ids = std::collections::HashSet::new();
        for model in &config.ollama_models {
            model.validate()?;
            if model.model_id.trim().is_empty() {
                bail!("selected model_id must not be empty")
            }
            if !matches!(model.modality.as_str(), "text" | "vlm") {
                bail!(
                    "model '{}' modality must be 'text' or 'vlm'",
                    model.model_id
                )
            }
            if !model_ids.insert(model.model_id.as_str()) {
                bail!("model '{}' is selected more than once", model.model_id)
            }
        }
        Ok(Self {
            crypto: NodeCrypto::new(&config.psk)?,
            config,
            engine,
            http: reqwest::Client::new(),
            status: Arc::new(std::sync::Mutex::new(json!({}))),
            submissions: Arc::new(crate::submission::SubmissionMailbox::default()),
        })
    }

    pub fn with_status(mut self, status: Arc<std::sync::Mutex<Value>>) -> Self {
        self.status = status;
        self
    }

    pub fn with_submissions(mut self, mailbox: Arc<crate::submission::SubmissionMailbox>) -> Self {
        self.submissions = mailbox;
        self
    }

    fn connection_status(&self, state: &str, address: &str, error: Option<String>, retry: f64) {
        let mut status = self.status.lock().expect("connection status");
        let counter = match state {
            "checking" => Some("connection_checks"),
            "connected" => Some("successful_connections"),
            "disconnected" => Some("failed_connection_attempts"),
            _ => None,
        };
        if let Some(key) = counter {
            status[key] = json!(status[key].as_u64().unwrap_or(0).saturating_add(1));
        }
        status["connection"] =
            json!({"state":state,"address":address,"last_error":error,"retry_seconds":retry});
    }

    pub async fn run(&self) -> Result<()> {
        let mut backoff = 1.0_f64;
        loop {
            match self.connect_once().await {
                Ok(()) => backoff = 1.0,
                Err(error) => {
                    warn!(%error, backoff, "node disconnected");
                    {
                        let mut state = self.status.lock().expect("connection status");
                        state["connection"]["state"] = json!("retrying");
                        state["connection"]["retry_seconds"] = json!(backoff);
                    }
                    sleep(Duration::from_secs_f64(backoff)).await;
                    backoff = (backoff * 2.0).min(30.0) + rand::random::<f64>();
                }
            }
        }
    }

    pub async fn connect_once(&self) -> Result<()> {
        let mut last = None;
        for endpoint in &self.config.endpoints {
            self.connection_status("checking", &endpoint.address, None, 0.0);
            match self.serve_endpoint(endpoint).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    self.connection_status(
                        "disconnected",
                        &endpoint.address,
                        Some(format!("{error:#}")),
                        0.0,
                    );
                    warn!(endpoint = %endpoint.address, priority = endpoint.priority, %error, "node endpoint failed");
                    last = Some(error);
                }
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("all coordinator endpoints failed")))
    }

    fn registration(
        &self,
        node_id: &str,
        name: &str,
        version: &str,
        capabilities: &[String],
        specs: &Value,
        models: &[Value],
    ) -> Value {
        let mut message = build_register(node_id, name, version, capabilities, specs, models);
        message["metadata"] = json!(self.config.metadata.advertised(capabilities));
        message["network"] = json!(self.config.network);
        message
    }

    pub async fn submit_message(&self, text: &str, wait: Duration) -> Result<Value> {
        if text.trim().is_empty() {
            bail!("message must not be empty")
        }
        let mut last = None;
        for endpoint in &self.config.endpoints {
            match self.probe_endpoint(endpoint).await {
                Ok(stream) => return timeout(wait, self.submit_to_endpoint(stream, text, wait)).await
                    .context("request timed out; delivery may have occurred and was not automatically retried")?,
                Err(error) => {
                    warn!(endpoint = %endpoint.address, priority = endpoint.priority, %error, "message submission failed");
                    last = Some(error);
                }
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("all coordinator endpoints failed")))
    }

    async fn submit_to_endpoint(
        &self,
        stream: TcpStream,
        text: &str,
        wait: Duration,
    ) -> Result<Value> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        write_message(
            &self.crypto,
            &mut writer,
            &build_auth(&self.crypto, &self.config.node_id, crate::VERSION),
        )
        .await?;
        let reply = read_message(&self.crypto, &mut reader)
            .await?
            .context("coordinator closed during auth")?;
        if reply.get("type").and_then(Value::as_str) != Some("ok") {
            bail!("coordinator denied auth: {reply}")
        }
        let mut models = discover_models(
            &self.http,
            &self.config.ollama_url,
            &self.config.ollama_models,
        )
        .await;
        if let Some(server) = &self.engine.model_server {
            models.extend(server.models()?)
        }
        let capabilities = advertised_capabilities(&self.engine.capabilities(), &models);
        write_message(
            &self.crypto,
            &mut writer,
            &self.registration(
                &self.config.node_id,
                if self.config.name.is_empty() {
                    &self.config.node_id
                } else {
                    &self.config.name
                },
                crate::VERSION,
                &capabilities,
                &system_specs(),
                &models,
            ),
        )
        .await?;
        let id = Uuid::new_v4().simple().to_string();
        write_message(
            &self.crypto,
            &mut writer,
            &json!({
                "type": "submit", "id": id, "node_id": self.config.node_id,
                "kind": "message", "payload": {"text": text}
            }),
        )
        .await?;
        timeout(wait, async {
            while let Some(message) = read_message(&self.crypto, &mut reader).await? {
                match message.get("type").and_then(Value::as_str) {
                    Some("submission_result")
                        if message.get("id").and_then(Value::as_str) == Some(id.as_str()) =>
                    {
                        if message.get("success").and_then(Value::as_bool) == Some(true) {
                            return Ok(message.get("data").cloned().unwrap_or(Value::Null));
                        }
                        bail!(
                            "{}",
                            message
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("submission failed")
                        );
                    }
                    Some("request") => {
                        let request_id = message.get("id").and_then(Value::as_str).unwrap_or("");
                        let capability = message
                            .get("capability")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let action = message
                            .get("action")
                            .and_then(Value::as_str)
                            .unwrap_or("run");
                        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                        let response = match self.engine.execute(capability, action, params).await {
                            Ok(data) => success_response(request_id, data),
                            Err(error) => error_response(request_id, error),
                        };
                        write_message(&self.crypto, &mut writer, &response).await?;
                    }
                    _ => {}
                }
            }
            bail!("coordinator disconnected before returning a result")
        })
        .await
        .context("coordinator did not return the message result in time")?
    }

    /// Probe the encrypted EEF service before authenticating/registering this node.
    /// Reuse the probed connection. ICMP reachability is neither required nor trusted.
    async fn probe_endpoint(&self, endpoint: &CoordinatorEndpoint) -> Result<TcpStream> {
        self.connection_status("checking", &endpoint.address, None, 0.0);
        let (host, port) = parse_endpoint(&endpoint.address)?;
        let stream = timeout(Duration::from_secs(5), async {
            let stream = TcpStream::connect((host.as_str(), port)).await?;
            let mut connection = BufReader::new(stream);
            let nonce = Uuid::new_v4().simple().to_string();
            write_message(
                &self.crypto,
                connection.get_mut(),
                &json!({"type":"probe","nonce":nonce}),
            )
            .await?;
            let reply = read_message(&self.crypto, &mut connection)
                .await?
                .context("EEF closed during the reachability check")?;
            if reply["type"] == "pong"
                && reply["nonce"] == nonce
                && reply["protocol"] == crate::PROTOCOL_VERSION
            {
                return Ok(connection.into_inner());
            }
            // 0.3.x answers this encrypted probe with an auth denial and closes.
            // This proves service reachability only. Normal authentication remains
            // mandatory on a fresh socket; no registration was sent by the probe.
            if reply["type"] == "denied" && reply["error"] == "auth failed" {
                return TcpStream::connect((host.as_str(), port))
                    .await
                    .map_err(Into::into);
            }
            bail!("EEF returned an invalid reachability response")
        })
        .await
        .context("EEF unavailable: reachability check timed out after five seconds")??;
        self.connection_status("connecting", &endpoint.address, None, 0.0);
        Ok(stream)
    }

    async fn serve_endpoint(&self, endpoint: &CoordinatorEndpoint) -> Result<()> {
        let (host, port) = parse_endpoint(&endpoint.address)?;
        let stream = self.probe_endpoint(endpoint).await?;
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let writer = Arc::new(Mutex::new(writer));

        {
            let mut output = writer.lock().await;
            write_message(
                &self.crypto,
                &mut *output,
                &build_auth(&self.crypto, &self.config.node_id, crate::VERSION),
            )
            .await?;
        }
        let reply = timeout(
            Duration::from_secs(10),
            read_message(&self.crypto, &mut reader),
        )
        .await
        .context("EEF did not finish signing this device in")??
        .context("coordinator closed during auth")?;
        if reply.get("type").and_then(Value::as_str) != Some("ok") {
            bail!("coordinator denied auth: {reply}");
        }
        let specs = system_specs();
        let mut models = discover_models(
            &self.http,
            &self.config.ollama_url,
            &self.config.ollama_models,
        )
        .await;
        if let Some(server) = &self.engine.model_server {
            models.extend(server.models()?)
        }
        let capabilities = advertised_capabilities(&self.engine.capabilities(), &models);
        {
            let mut output = writer.lock().await;
            write_message(
                &self.crypto,
                &mut *output,
                &self.registration(
                    &self.config.node_id,
                    if self.config.name.is_empty() {
                        &self.config.node_id
                    } else {
                        &self.config.name
                    },
                    crate::VERSION,
                    &capabilities,
                    &specs,
                    &models,
                ),
            )
            .await?;
        }
        info!(host, port, node_id = %self.config.node_id, "node authenticated and registered");
        self.connection_status("connected", &endpoint.address, None, 0.0);
        {
            let mut status = self.status.lock().expect("status");
            status["models"] = json!(models);
            status["capabilities"] = json!(capabilities);
        }

        let heartbeat_writer = writer.clone();
        let heartbeat_crypto = self.crypto.clone();
        let heartbeat_id = self.config.node_id.clone();
        let heartbeat_interval = Duration::from_secs_f64(self.config.heartbeat_seconds.max(0.2));
        let heartbeat = AbortTask(tokio::spawn(async move {
            loop {
                let started = Instant::now();
                let load = current_load().await;
                let latency = started.elapsed().as_secs_f64() * 1000.0;
                let message = build_heartbeat(&heartbeat_id, &load, latency, &system_specs());
                if write_message(
                    &heartbeat_crypto,
                    &mut *heartbeat_writer.lock().await,
                    &message,
                )
                .await
                .is_err()
                {
                    break;
                }
                sleep(heartbeat_interval).await;
            }
        }));

        let mut tasks = tokio::task::JoinSet::new();
        let mut local = self.submissions.connect();
        let mut replies: std::collections::HashMap<
            String,
            tokio::sync::oneshot::Sender<Result<Value>>,
        > = std::collections::HashMap::new();
        loop {
            // Keep partial frame reads alive when local input arrives. Dropping a
            // read_message future mid-frame would corrupt the next message.
            let incoming = read_message(&self.crypto, &mut reader);
            tokio::pin!(incoming);
            let message = loop {
                tokio::select! {
                    message = &mut incoming => break message?,
                    Some(command) = local.recv() => {
                        replies.retain(|_, reply| !reply.is_closed());
                        if command.reply.is_closed() { continue; }
                        if replies.len() >= 8 {
                            let _ = command.reply.send(Err(crate::submission::SubmissionFailure::NotSent("Node is busy. This message was not sent.".into()).into()));
                            continue;
                        }
                        let value = json!({"type":"submit","id":command.id,"node_id":self.config.node_id,
                            "kind":command.kind,"payload":command.payload,"operation_id":command.operation_id});
                        // Store before writing: a failed/partial write is unknown,
                        // not permission to automatically execute the request twice.
                        replies.insert(command.id, command.reply);
                        timeout(Duration::from_secs(5), async {
                            write_message(&self.crypto, &mut *writer.lock().await, &value).await
                        }).await.context("message send timed out; delivery is unknown")??;
                    }
                }
            };
            let Some(message) = message else {
                break;
            };
            while tasks.try_join_next().is_some() {}
            if message["type"] == "submission_result" {
                if let Some(reply) = message["id"].as_str().and_then(|id| replies.remove(id)) {
                    let result = if message["success"] == true {
                        Ok(message["data"].clone())
                    } else {
                        Err(crate::submission::SubmissionFailure::Rejected(
                            message["error"]
                                .as_str()
                                .unwrap_or("Request failed")
                                .to_owned(),
                        )
                        .into())
                    };
                    let _ = reply.send(result);
                }
                continue;
            }
            if message.get("type").and_then(Value::as_str) != Some("request") {
                continue;
            }
            let request_id = message
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let capability = message
                .get("capability")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let action = message
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("run")
                .to_owned();
            let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
            let context = message
                .get("request_context")
                .cloned()
                .unwrap_or(Value::Null);
            let engine = self.engine.clone();
            let writer = writer.clone();
            let crypto = self.crypto.clone();
            let status = self.status.clone();
            tasks.spawn(async move {
                {
                    let mut s = status.lock().expect("status");
                    s["last_activity"] = json!(capability);
                    s["last_request_context"] = context;
                    s["last_resource_id"] =
                        params.get("resource_id").cloned().unwrap_or(Value::Null);
                }
                let response = match engine.execute(&capability, &action, params).await {
                    Ok(data) => success_response(&request_id, data),
                    Err(error) => error_response(&request_id, error),
                };
                let _ = write_message(&crypto, &mut *writer.lock().await, &response).await;
            });
        }
        drop(heartbeat);
        bail!("EEF closed the connection. Reconnecting automatically.")
    }
}

fn parse_endpoint(raw: &str) -> Result<(String, u16)> {
    let raw = raw.trim().trim_start_matches("tcp://");
    if raw.is_empty() {
        bail!("empty endpoint")
    }
    if let Ok(address) = raw.parse::<std::net::SocketAddr>() {
        return Ok((address.ip().to_string(), address.port()));
    }
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
            Ok((host.into(), port.parse()?))
        }
        _ => Ok((raw.into(), crate::DEFAULT_PORT)),
    }
}

pub fn system_specs() -> Value {
    let system = System::new_all();
    let storage_bytes = Disks::new_with_refreshed_list()
        .iter()
        .map(|disk| disk.available_space())
        .sum::<u64>();
    json!({
        "hostname": System::host_name(),
        "cpu_name": system.cpus().first().map(|cpu| cpu.brand()).unwrap_or("Unknown CPU"),
        "cpu_cores": system.cpus().len(),
        "cpu_freq_mhz": system.cpus().first().map(|cpu| cpu.frequency()).unwrap_or(0),
        "ram_mb": system.total_memory() / 1024 / 1024,
        "gpu_model": "",
        "gpu_vram_mb": 0,
        "gpu_family": "",
        "storage_gb": storage_bytes / 1024 / 1024 / 1024,
        "platform": std::env::consts::OS,
    })
}

pub async fn current_load() -> Value {
    tokio::task::spawn_blocking(|| {
        let mut system = System::new_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_cpu_usage();
        system.refresh_memory();
        let ram = if system.total_memory() == 0 {
            0.0
        } else {
            system.used_memory() as f64 / system.total_memory() as f64
        };
        json!({
            "cpu": f64::from(system.global_cpu_usage()) / 100.0,
            "ram": ram,
            "gpu": 0.0,
            "queue": 0,
        })
    })
    .await
    .unwrap_or_else(|_| json!({"cpu": 0.0, "ram": 0.0, "gpu": 0.0, "queue": 0}))
}

pub async fn discover_models(
    http: &reqwest::Client,
    base: &str,
    selected: &[SelectedModel],
) -> Vec<Value> {
    if selected.is_empty() {
        return Vec::new();
    }
    let Ok(installed) = installed_ollama_models(http, base).await else {
        return vec![];
    };
    let installed = installed
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    selected
        .iter()
        .filter(|model| installed.contains(model.model_id.as_str()))
        .filter_map(|model| {
            let metadata = model.selection_metadata().ok()?;
            Some(json!({"model_id": model.model_id, "modality": model.modality, "backend": "ollama", "capabilities":metadata.capabilities,"model_metadata":metadata}))
        })
        .collect()
}

pub async fn installed_ollama_models(http: &reqwest::Client, base: &str) -> Result<Vec<String>> {
    let response = http
        .get(format!("{base}/api/tags"))
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?;
    let data = crate::model_manager::bounded_json(response).await?;
    let mut installed = data
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| model.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    installed.sort();
    installed.dedup();
    Ok(installed)
}

fn advertised_capabilities(configured: &[String], models: &[Value]) -> Vec<String> {
    let mut capabilities = configured
        .iter()
        .filter(|capability| !matches!(capability.as_str(), "llm.infer" | "vlm.analyze"))
        .cloned()
        .collect::<Vec<_>>();
    let supports = |model: &Value, capability: &str| {
        let metadata = if let Some(raw) = model.get("model_metadata") {
            serde_json::from_value::<crate::model_metadata::ModelMetadata>(raw.clone())
                .ok()
                .filter(|m| m.validate().is_ok())
        } else {
            Some(crate::model_metadata::ModelMetadata::from_legacy(
                model.get("modality").and_then(Value::as_str),
            ))
        };
        metadata.is_some_and(|m| m.supports(capability))
    };
    if models.iter().any(|model| supports(model, "llm.infer")) {
        capabilities.push("llm.infer".into());
    }
    if models.iter().any(|model| supports(model, "vlm.analyze")) {
        capabilities.push("vlm.analyze".into());
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_client(endpoint: String) -> NodeClient {
        let config: NodeClientConfig = serde_json::from_value(json!({
            "endpoints":[endpoint],"node_id":"probe-test","psk":"test-secret"
        }))
        .unwrap();
        NodeClient::new(
            config,
            Arc::new(NodeEngine::new(false, vec![], std::env::temp_dir()).unwrap()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn probe_succeeds_without_registering_a_node() {
        let server = crate::NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        server.start().await.unwrap();
        let client = probe_client(format!("127.0.0.1:{}", server.port()));
        let stream = client
            .probe_endpoint(&client.config.endpoints[0])
            .await
            .unwrap();
        assert!(server.connected_nodes().await.is_empty());
        assert!(server.registered_nodes().await.is_empty());
        drop(stream);
        server.stop().await;
    }

    #[tokio::test]
    async fn probe_rejects_wrong_challenge_and_closed_service() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let fake = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (input, mut output) = stream.into_split();
            let mut input = BufReader::new(input);
            let crypto = NodeCrypto::new("test-secret").unwrap();
            let probe = read_message(&crypto, &mut input).await.unwrap().unwrap();
            assert_eq!(probe["type"], "probe");
            write_message(
                &crypto,
                &mut output,
                &json!({"type":"pong","nonce":"wrong","protocol":crate::PROTOCOL_VERSION}),
            )
            .await
            .unwrap();
        });
        let client = probe_client(address);
        assert!(
            client
                .probe_endpoint(&client.config.endpoints[0])
                .await
                .is_err()
        );
        fake.await.unwrap();
        assert!(
            client
                .probe_endpoint(&client.config.endpoints[0])
                .await
                .is_err()
        );
    }

    #[test]
    fn endpoints_default_and_explicit_ports() {
        assert_eq!(
            parse_endpoint("node.example").unwrap(),
            ("node.example".into(), 51335)
        );
        assert_eq!(
            parse_endpoint("127.0.0.1:1234").unwrap(),
            ("127.0.0.1".into(), 1234)
        );
    }

    #[test]
    fn coordinator_endpoints_accept_strings_and_sort_by_priority() {
        let parsed: Vec<CoordinatorEndpoint> = serde_json::from_value(json!([
            "low.example:51335",
            {"address": "high.example:51335", "priority": 100}
        ]))
        .unwrap();
        assert_eq!(parsed[0].priority, 0);
        let engine =
            Arc::new(NodeEngine::new(false, vec![], std::env::current_dir().unwrap()).unwrap());
        let client = NodeClient::new(
            NodeClientConfig {
                network: Default::default(),
                metadata: Default::default(),
                endpoints: parsed,
                node_id: "node-test".into(),
                name: String::new(),
                psk: "a-valid-test-secret".into(),
                heartbeat_seconds: 3.0,
                allow_write: false,
                allowed_roots: vec![],
                python: None,
                python_plugins: vec![],
                update_manifest: None,
                ollama_url: default_ollama_url(),
                ollama_models: vec![],
            },
            engine,
        )
        .unwrap();
        assert_eq!(client.config.endpoints[0].priority, 100);
    }

    #[test]
    fn local_connection_does_not_replace_advertised_address() {
        let mut client = probe_client("127.0.0.1:51335".parse().unwrap());
        client.config.network.advertised_address = Some("26.1.2.3".into());
        let message = client.registration("stable", "Laptop", "test", &[], &json!({}), &[]);
        assert_eq!(message["network"]["advertised_address"], "26.1.2.3");
        assert_eq!(client.config.endpoints[0].address, "127.0.0.1:51335");
        assert_eq!(message["node_id"], "stable");
    }

    #[tokio::test]
    async fn no_models_are_discovered_without_owner_selection() {
        let models = discover_models(&reqwest::Client::new(), "http://127.0.0.1:1", &[]).await;
        assert!(models.is_empty());
    }

    #[test]
    fn model_capabilities_require_discovered_models() {
        let configured = vec!["system.ping".into(), "llm.infer".into()];
        assert_eq!(
            advertised_capabilities(&configured, &[]),
            vec!["system.ping"]
        );
        assert_eq!(
            advertised_capabilities(&configured, &[json!({"modality": "text"})]),
            vec!["llm.infer", "system.ping"]
        );
        assert_eq!(
            advertised_capabilities(&configured, &[json!({"modality":"vlm"})]),
            vec!["llm.infer", "system.ping", "vlm.analyze"]
        );
        let mut metadata = crate::model_metadata::ModelMetadata::from_legacy(Some("vlm"));
        metadata.capabilities = vec![];
        assert_eq!(
            advertised_capabilities(
                &configured,
                &[json!({"modality":"vlm","model_metadata":metadata})]
            ),
            vec!["system.ping"]
        );
    }
}
