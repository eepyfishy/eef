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
    pub auth_mode: crate::authentication::AuthMode,
    #[serde(default)]
    pub coordinator_certificate: Option<String>,
    #[serde(default)]
    pub request_limits: crate::request_limits::RequestLimits,
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
    grant_ledger: Option<Arc<crate::grants::GrantLedger>>,
    tls_config: Option<Arc<tokio_rustls::rustls::ClientConfig>>,
    request_budget: crate::request_limits::RequestBudget,
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
        crate::network::validate_remote_node_id(&config.node_id)?;
        let tls_config = if config.auth_mode == crate::authentication::AuthMode::NodeCredential {
            Some(crate::tls::client_config(config.coordinator_certificate.as_deref()
                .context("credential connections require an owner-paired coordinator certificate; pair again")?)?)
        } else {
            None
        };
        if config.auth_mode == crate::authentication::AuthMode::NodeCredential {
            crate::authentication::validate_credential(&config.psk)?;
        }
        config.metadata.validate()?;
        config.network.validate()?;
        if config.endpoints.is_empty() {
            bail!("at least one coordinator endpoint is required")
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
            grant_ledger: None,
            tls_config,
            request_budget: crate::request_limits::RequestBudget::new(&config.request_limits)?,
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
        self.status.lock().expect("status")["execution_grants"] = json!({"schema_version":1,"available":self.grant_ledger.is_some() && self.tls_config.is_some(),
            "limits":self.grant_ledger.as_ref().map(|l|l.policy()),"deadline_guarantee":"rpc_future_only","timing_mode":crate::grant_time::MODE,
            "retained_results":self.grant_ledger.is_some() && self.tls_config.is_some()});
        self
    }
    pub fn with_grants(
        mut self,
        path: &std::path::Path,
        policy: crate::grants::GrantPolicy,
    ) -> Self {
        match crate::grants::GrantLedger::open(path, policy, &self.config.node_id) {
            Ok(ledger) => self.grant_ledger = Some(ledger),
            Err(_) => warn!(
                "Execution grant journal unavailable; retained for recovery; grant execution disabled"
            ),
        }
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
        status["connection"] = json!({"state":state,"address":address,"last_error":error,"retry_seconds":retry,
                "auth_mode":self.config.auth_mode,"transport":if self.tls_config.is_some(){"pinned_tls13"}else{"legacy_shared_key"}});
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
        message["execution_grants"] = json!({"schema_version":1,"available":self.grant_ledger.is_some() && self.tls_config.is_some(),
            "limits":self.grant_ledger.as_ref().map(|l|l.policy()),"deadline_guarantee":"rpc_future_only","timing_mode":crate::grant_time::MODE,
            "retained_results":self.grant_ledger.is_some() && self.tls_config.is_some()});
        message
    }

    /// Retired one-shot input: use the running node's request service instead.
    /// This must not register a second runtime or execute ordinary inbound RPC.
    pub async fn submit_message(&self, _text: &str, _wait: Duration) -> Result<Value> {
        bail!(
            "Use the already running node: eefn --ask or requests open. Standalone message submission is retired; no connection or inference was attempted."
        )
    }

    /// Probe the encrypted EEF service before authenticating/registering this node.
    /// Reuse the probed connection. ICMP reachability is neither required nor trusted.
    async fn probe_endpoint(
        &self,
        endpoint: &CoordinatorEndpoint,
    ) -> Result<crate::tls::NodeStream> {
        self.connection_status("checking", &endpoint.address, None, 0.0);
        let (host, port) = parse_endpoint(&endpoint.address)?;
        let stream = timeout(Duration::from_secs(5), async {
            let mut stream = TcpStream::connect((host.as_str(), port)).await?;
            let stream: crate::tls::NodeStream = if let Some(config) = &self.tls_config {
                crate::authentication::write_selector(&mut stream, &self.config.node_id).await?;
                crate::tls::connect(
                    stream,
                    self.config
                        .coordinator_certificate
                        .as_deref()
                        .context("paired certificate missing")?,
                    config.clone(),
                )
                .await?
            } else {
                Box::new(stream)
            };
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
                // Preserve any read-ahead bytes across probe/authentication.
                return Ok(Box::new(connection) as crate::tls::NodeStream);
            }
            // 0.3.x answers this encrypted probe with an auth denial and closes.
            // This proves service reachability only. Normal authentication remains
            // mandatory on a fresh socket; no registration was sent by the probe.
            if self.config.auth_mode == crate::authentication::AuthMode::Legacy
                && reply["type"] == "denied"
                && reply["error"] == "auth failed"
            {
                return Ok(Box::new(TcpStream::connect((host.as_str(), port)).await?)
                    as crate::tls::NodeStream);
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
        let (reader, writer) = tokio::io::split(stream);
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
        let grant_session = reply["grant_session_id"]
            .as_str()
            .filter(|s| crate::job_commands::validate_operation_id(s).is_ok())
            .map(str::to_owned);
        let grant_issuer = if self.tls_config.is_some() {
            self.config
                .coordinator_certificate
                .as_deref()
                .map(crate::tls::certificate_fingerprint)
                .transpose()?
        } else {
            None
        };
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
            status["request_limits"] = json!(self.config.request_limits);
        }

        // Subscribe before the first refresh. A startup transition that happened
        // during authentication must not be lost between snapshot and subscription.
        let mut model_changes = self.engine.model_server.as_ref().map(|s| s.subscribe());
        if let Some(changes) = &mut model_changes {
            changes.mark_changed();
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
        let lease_requests = Arc::new(crate::grant_time::LeaseRequests::default());
        let mut local = self.submissions.connect();
        // A timed-out partial response must retire this framed connection.
        let (receipt_write_failed, mut receipt_write_failure) = tokio::sync::watch::channel(false);
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
                    _ = receipt_write_failure.changed() => bail!("Receipt response delivery failed; reconnect required"),
                    message = &mut incoming => break message?,
                    _ = async {
                        if let Some(changes) = &mut model_changes { let _ = changes.changed().await; }
                        else { std::future::pending::<()>().await; }
                    } => {
                        // Refresh in place while retaining the in-progress frame.
                        if let Some(changes) = &mut model_changes { changes.borrow_and_update(); }
                        let mut updated = models.iter().filter(|m| m["backend"] != "llamacpp")
                            .cloned().collect::<Vec<_>>();
                        if let Some(server) = &self.engine.model_server { updated.extend(server.models()?); }
                        if updated == models { continue; }
                        let capabilities = advertised_capabilities(&self.engine.capabilities(), &updated);
                        let registration = self.registration(&self.config.node_id, &self.config.name,
                            crate::VERSION, &capabilities, &specs, &updated);
                        timeout(Duration::from_secs(5), async {
                            write_message(&self.crypto, &mut *writer.lock().await, &registration).await
                        }).await.context("model registration refresh timed out")??;
                        models = updated;
                        let mut status = self.status.lock().expect("status");
                        status["models"] = json!(models);
                        status["capabilities"] = json!(capabilities);
                    }
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
            if message["type"] == "grant_lease" {
                lease_requests.complete(message);
                continue;
            }
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
            if message["type"] == "grant_receipt" || message["type"] == "grant_result" {
                let result_query = message["type"] == "grant_result";
                let request_id = message["id"].as_str().unwrap_or("").to_owned();
                let query = if result_query {
                    serde_json::from_value::<crate::request_result::WireRequest>(message).and_then(
                        |q| {
                            let digest = q.expected_grant_sha256.clone();
                            q.receipt_query()
                                .map(|query| (query, Some(digest)))
                                .map_err(serde::de::Error::custom)
                        },
                    )
                } else {
                    serde_json::from_value::<crate::request_receipt::WireRequest>(message)
                        .map(|q| (q, None))
                };
                let prepared = match (
                    query,
                    self.request_budget.admit_receipt(),
                    self.grant_ledger.clone(),
                    grant_issuer.clone(),
                ) {
                    (Ok((query, digest)), Some(permit), Some(ledger), Some(issuer))
                        if query.validate(&self.config.node_id, &issuer).is_ok()
                            && digest
                                .as_deref()
                                .is_none_or(crate::request_result::valid_digest) =>
                    {
                        Some((query, digest, permit, ledger, issuer))
                    }
                    _ => None,
                };
                let Some((query, digest, permit, ledger, issuer)) = prepared else {
                    let response = json!({"type":"response","id":request_id,"success":false,"error_type":"GrantReceiptUnavailable","error":"Authenticated receipt inspection unavailable; no execution or recovery performed"});
                    timeout(Duration::from_secs(5), async {
                        write_message(&self.crypto, &mut *writer.lock().await, &response).await
                    })
                    .await
                    .context("receipt rejection write timed out")??;
                    continue;
                };
                let writer = writer.clone();
                let crypto = self.crypto.clone();
                let write_failed = receipt_write_failed.clone();
                tasks.spawn(async move {
                    let read = tokio::task::spawn_blocking(move || {
                        let result = if let Some(digest)=digest {
                            ledger.result(&issuer,&query.grant_id,&digest)
                        } else {ledger.receipt(&issuer, &query.grant_id)};
                        (result, permit)
                    }).await;
                    let (result, _permit) = match read {
                        Ok((result, permit)) => (result, Some(permit)),
                        Err(_) => (Err(anyhow::anyhow!("Receipt reader unavailable")), None),
                    };
                    let response = match result {
                        Ok(data) => success_response(&request_id, data),
                        Err(_) => json!({"type":"response","id":request_id,"success":false,"error_type":"GrantReceiptUnavailable","error":"Consumed receipt unavailable; no execution, fallback or recovery performed"}),
                    };
                    if !matches!(timeout(Duration::from_secs(5), async {
                        write_message(&crypto, &mut *writer.lock().await, &response).await
                    }).await, Ok(Ok(()))) { let _ = write_failed.send(true); }
                });
                continue;
            }
            if message["type"] == "execute_grant" {
                let request_id = message["id"].as_str().unwrap_or("").to_owned();
                let parsed = serde_json::from_value::<crate::grants::GrantRequest>(message.clone())
                    .and_then(|r| {
                        if r.kind != "execute_grant"
                            || crate::request_drafts::validate_id(&r.id).is_err()
                        {
                            Err(serde::de::Error::custom("Invalid grant envelope"))
                        } else {
                            Ok(r.grant)
                        }
                    });
                let (Ok(grant), Some(ledger), Some(issuer), Some(session)) = (
                    parsed,
                    self.grant_ledger.clone(),
                    grant_issuer.as_deref(),
                    grant_session.as_deref(),
                ) else {
                    let response = json!({"type":"response","id":request_id,"success":false,"error_type":"GrantDenied","not_started":true,"error":"Grant execution requires a valid scope, receipt journal and pinned issuer session"});
                    timeout(Duration::from_secs(5), async {
                        write_message(&self.crypto, &mut *writer.lock().await, &response).await
                    })
                    .await
                    .context("grant rejection write timed out")??;
                    continue;
                };
                let admission = self.request_budget.admit(&grant.capability, &grant.action);
                let prepared = if admission.is_some() {
                    grant
                        .validate()
                        .map_err(|_| crate::grants::GrantDenied("invalid_grant_scope").into())
                        .and_then(|_| {
                            if grant.matches_session(issuer, session, &self.config.node_id) {
                                self.engine.check_grant_policy(&grant).map_err(|_| {
                                    crate::grants::GrantDenied("local_policy_denied_or_changed")
                                        .into()
                                })
                            } else {
                                Err(crate::grants::GrantDenied("grant_identity_changed").into())
                            }
                        })
                } else {
                    Err(crate::grants::GrantDenied("node_grant_capacity_full").into())
                };
                match prepared {
                    Err(error) => {
                        let report = crate::grants::GrantReport::new(
                            &grant,
                            crate::grants::ReceiptState::Rejected,
                            Value::Null,
                            Some(crate::grants::denial_code(&error)),
                        );
                        let response = json!({"type":"response","id":request_id,"success":false,"grant":report});
                        timeout(Duration::from_secs(5), async {
                            write_message(&self.crypto, &mut *writer.lock().await, &response).await
                        })
                        .await
                        .context("grant rejection write timed out")??;
                    }
                    Ok(()) => {
                        let engine = self.engine.clone();
                        let writer = writer.clone();
                        let crypto = self.crypto.clone();
                        let status = self.status.clone();
                        let issuer = issuer.to_owned();
                        let session = session.to_owned();
                        let node_id = self.config.node_id.clone();
                        let leases = lease_requests.clone();
                        let write_failed = receipt_write_failed.clone();
                        tasks.spawn(async move {
                            let _permit=admission;
                            let lease = leases.request(&grant, &request_id, |query| {
                                let writer=&writer;
                                let crypto=&crypto;
                                let write_failed=&write_failed;
                                async move {
                                let mut output=writer.lock().await;
                                let mut guard=crate::grant_time::WriteGuard::new(write_failed.clone());
                                let result=write_message(&crypto,&mut *output,&query).await;
                                if result.is_ok(){guard.complete()}
                                result
                            }}).await;
                            let prepared=lease.map_err(|_| crate::grants::GrantDenied("grant_lease_unavailable").into())
                                .and_then(|lease| crate::grants::AcceptedGrant::prepare_with_lease(
                                    grant.clone(),ledger,&engine,&issuer,&session,&node_id,lease));
                            let report=match prepared {
                                Err(error)=>crate::grants::GrantReport::new(&grant,crate::grants::ReceiptState::Rejected,Value::Null,Some(crate::grants::denial_code(&error))),
                                Ok(accepted)=>{
                                    // Claim is durable. Failed/cancelled ACK writes
                                    // retire the connection and retain uncertainty;
                                    // no effects start and no rejected receipt is invented.
                                    let ack=json!({"type":"grant_ack","id":request_id,"grant":accepted.acknowledgement()});
                                    if !matches!(timeout(Duration::from_secs(5),async {
                                        let mut output=writer.lock().await;
                                        let mut guard=crate::grant_time::WriteGuard::new(write_failed.clone());
                                        let result=write_message(&crypto,&mut *output,&ack).await;
                                        if result.is_ok(){guard.complete()}
                                        result
                                    }).await,Ok(Ok(()))){let _=write_failed.send(true);return}
                                    {
                                        let mut live=status.lock().expect("status");
                                        live["last_activity"]=json!(grant.capability);
                                        live["last_request_context"]=json!(grant.request_context);
                                        live["last_resource_id"]=json!(grant.resource_id);
                                        // Set only after durable admission and a successful ACK
                                        // write, immediately before the executor. Control polling
                                        // may change last_activity, but cannot erase this scope.
                                        live["last_workload"]=json!({"schema_version":1,
                                            "grant_id":grant.grant_id,"capability":grant.capability,
                                            "request_context":grant.request_context,"resource_id":grant.resource_id,
                                            "model_id":grant.model_id,"backend":grant.backend,
                                            "execution_may_have_started":true});
                                    }
                                    accepted.run(&engine).await
                                }
                            };
                            status.lock().expect("status")["last_grant"]=json!({"grant_id":report.grant_id,"state":report.state,"persisted":report.persisted,"success":report.success});
                            let response=json!({"type":"response","id":request_id,"success":report.success,"grant":report});
                            if !matches!(timeout(Duration::from_secs(5),async {
                                let mut output=writer.lock().await;
                                let mut guard=crate::grant_time::WriteGuard::new(write_failed.clone());
                                let result=write_message(&crypto,&mut *output,&response).await;
                                if result.is_ok(){guard.complete()}
                                result
                            }).await,Ok(Ok(()))){let _=write_failed.send(true);}
                        });
                    }
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
            if !crate::execution_boundary::ordinary_control(&capability) {
                let response = json!({"type":"response","id":request_id,"success":false,
                    "error":"Workload execution requires a reviewed execution grant; no fallback was attempted",
                    "error_type":"ExecutionGrantRequired","error_code":crate::execution_boundary::ERROR_CODE,
                    "not_started":true});
                timeout(Duration::from_secs(5), async {
                    write_message(&self.crypto, &mut *writer.lock().await, &response).await
                })
                .await
                .context("grant-required response timed out; reconnect required")??;
                continue;
            }
            let Some(permit) = self.request_budget.admit(&capability, &action) else {
                {
                    let mut status = self.status.lock().expect("status");
                    status["busy_rejections"] = json!(
                        status["busy_rejections"]
                            .as_u64()
                            .unwrap_or(0)
                            .saturating_add(1)
                    );
                }
                let response = json!({"type":"response","id":request_id,"success":false,
                    "error":"Node request capacity is full; this request was not started",
                    "error_type":"NodeBusy","not_started":true});
                // A timed-out partial write ends this connection; never continue
                // its framing or automatically replay the rejected operation.
                timeout(Duration::from_secs(5), async {
                    write_message(&self.crypto, &mut *writer.lock().await, &response).await
                })
                .await
                .context("busy response timed out; reconnect required")??;
                continue;
            };
            let context = message
                .get("request_context")
                .cloned()
                .unwrap_or(Value::Null);
            let engine = self.engine.clone();
            let writer = writer.clone();
            let crypto = self.crypto.clone();
            let status = self.status.clone();
            tasks.spawn(async move {
                let _permit = permit;
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
        metadata.is_some_and(|m| m.supports(capability) && m.routable())
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

    #[test]
    fn client_identity_validation_is_identical_for_legacy_and_credential_modes() {
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let engine = Arc::new(NodeEngine::new(false, vec![], std::env::temp_dir()).unwrap());
        for mode in ["legacy", "node_credential"] {
            for id in ["local", "*", " node", "node ", "bad\0id"] {
                let config = serde_json::from_value(json!({
                    "endpoints":["127.0.0.1:51335"],"node_id":id,"psk":"a".repeat(64),
                    "auth_mode":mode,"coordinator_certificate":identity.certificate,
                }))
                .unwrap();
                assert!(
                    NodeClient::new(config, engine.clone()).is_err(),
                    "mode={mode} id={id:?}"
                );
            }
        }
    }

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
    async fn pinned_grants_acknowledge_and_fence_effects_across_node_restart() {
        let dir = tempfile::tempdir().unwrap();
        let config_file = dir.path().join("node.json");
        let receipts = crate::grants::journal_path(&config_file);
        let policy: crate::NodePolicy = serde_json::from_value(
            json!({"filesystem":{"write":true},"full_access":["filesystem.write"]}),
        )
        .unwrap();
        std::fs::write(
            &config_file,
            serde_json::to_vec(&json!({"node_id":"grant-node","permissions":policy})).unwrap(),
        )
        .unwrap();
        let service = crate::NodeService::new(config_file, "grant-node".into());
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_service(service),
        );
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let key = crate::authentication::generate_credential();
        let server = crate::NodeServer::with_authentication(
            "",
            "127.0.0.1",
            0,
            crate::authentication::AuthenticationPolicy {
                allow_legacy: false,
                credentials: std::collections::BTreeMap::from([(
                    "grant-node".into(),
                    Some(key.clone()),
                )]),
                tls: Some(identity.clone()),
            },
        )
        .unwrap();
        server.start().await.unwrap();
        let mut events = server.subscribe();
        let live = Arc::new(std::sync::Mutex::new(json!({})));
        let config:NodeClientConfig=serde_json::from_value(json!({"node_id":"grant-node","psk":key,"auth_mode":"node_credential",
            "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",server.port())],"heartbeat_seconds":0.2})).unwrap();
        let start = || {
            let client = NodeClient::new(config.clone(), engine.clone())
                .unwrap()
                .with_grants(&receipts, Default::default())
                .with_status(live.clone());
            let endpoint = client.config.endpoints[0].clone();
            tokio::spawn(async move { client.serve_endpoint(&endpoint).await })
        };
        let wait_ready = || async {
            timeout(Duration::from_secs(10), async {
                loop {
                    if let Ok(s) = server.grant_session("grant-node").await {
                        break s;
                    };
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap()
        };
        let node = start();
        let session = wait_ready().await;
        let now = crate::protocol::now_ms();
        let out = dir.path().join("effect.txt");
        let g:crate::grants::ExecutionGrant=serde_json::from_value(json!({"schema_version":1,"grant_id":Uuid::new_v4().to_string(),
            "issuer_id":session["issuer_id"],"session_id":session["session_id"],"executor_node":"grant-node","job_id":Uuid::new_v4().simple().to_string(),
            "task_id":Uuid::new_v4().simple().to_string(),"attempt":1,"plan_sha256":"b".repeat(64),
            "request_context":{"request_id":Uuid::new_v4().to_string(),"origin_node":"origin","origin_area":["Room"]},
            "capability":"filesystem","action":"write","params":{"path":out,"content":"first effect"},"resource_id":null,"model_id":null,"backend":null,"model_role":null,"priority":0,
            "limits":{"max_execution_ms":5000,"max_result_bytes":65536},"reporting":"acceptance_and_completion","issued_at_ms":now-1,"expires_at_ms":now+10_000})).unwrap();
        let response = server
            .invoke_grant(g.clone(), Duration::from_secs(10))
            .await
            .unwrap();
        let result: crate::grants::GrantReport =
            serde_json::from_value(response["grant"].clone()).unwrap();
        assert!(result.confirms(&g));
        assert!(result.success);
        let workload = live.lock().unwrap()["last_workload"].clone();
        assert_eq!(workload["grant_id"], g.grant_id);
        assert_eq!(workload["request_context"], json!(g.request_context));
        assert_eq!(workload["execution_may_have_started"], true);
        assert!(workload.get("params").is_none());
        server
            .invoke_remote(
                "grant-node",
                "system.ping",
                "run",
                json!({}),
                Duration::from_secs(3),
            )
            .await
            .unwrap();
        assert_eq!(
            live.lock().unwrap()["last_workload"],
            workload,
            "control polling cannot erase workload scope"
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "first effect");
        let ack = timeout(Duration::from_secs(3), async {
            loop {
                if let crate::server::NodeEvent::GrantAccepted(r) = events.recv().await.unwrap() {
                    break r;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(ack["grant_id"], g.grant_id);
        assert_eq!(ack["persisted"], true);
        assert_eq!(ack["execution_may_have_started"], false);
        let inspected = server.inspect_grant_receipt(&g).await.unwrap();
        assert_eq!(inspected.state, "completed");
        assert!(inspected.confirms(&g));
        let retained = server.inspect_grant_result(&g).await.unwrap();
        assert!(retained.confirms(&g));
        assert_eq!(json!(retained.report), json!(Some(result.clone())));
        let mut wrong = g.clone();
        wrong.params["content"] = json!("forged different scope");
        assert!(server.inspect_grant_receipt(&wrong).await.is_err());
        assert!(server.inspect_grant_result(&wrong).await.is_err());
        let mut wrong = g.clone();
        wrong.issuer_id = "d".repeat(64);
        assert!(server.inspect_grant_receipt(&wrong).await.is_err());
        assert!(server.inspect_grant_result(&wrong).await.is_err());
        // Node checkpoints output before a failing coordinator observer loses
        // completion. Recovery reads the same old issuance without re-executing.
        struct LoseCompletion;
        impl crate::grants::GrantObserver for LoseCompletion {
            fn record(&self, r: &crate::grants::GrantReport) -> Result<()> {
                if r.state == crate::grants::ReceiptState::Completed {
                    bail!("fixture lost coordinator checkpoint")
                }
                Ok(())
            }
        }
        let mut lost = g.clone();
        lost.grant_id = Uuid::new_v4().to_string();
        lost.job_id = Uuid::new_v4().simple().to_string();
        lost.task_id = Uuid::new_v4().simple().to_string();
        lost.params["content"] = json!("effect with lost completion");
        lost.issued_at_ms = crate::protocol::now_ms();
        lost.expires_at_ms = lost.issued_at_ms + 10_000;
        let unconfirmed = server
            .invoke_grant_observed(
                lost.clone(),
                Duration::from_secs(10),
                Arc::new(LoseCompletion),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(unconfirmed["error_type"], "GrantJournalError");
        let recovered = server.inspect_grant_result(&lost).await.unwrap();
        assert!(recovered.confirms(&lost));
        assert!(recovered.report.as_ref().unwrap().success);
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "effect with lost completion"
        );
        std::fs::write(&out, "owner marker").unwrap();
        let duplicate = server
            .invoke_grant(g.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(duplicate["grant"]["state"], "rejected");
        assert_eq!(
            live.lock().unwrap()["last_workload"]["grant_id"],
            lost.grant_id,
            "rejection cannot claim a new workload started"
        );
        assert_eq!(
            duplicate["grant"]["error_code"],
            "grant_attempt_replayed_or_unresolved"
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "owner marker");
        node.abort();
        let _ = node.await;
        timeout(Duration::from_secs(3), async {
            while server.connected_nodes().await.contains_key("grant-node") {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let node = start();
        let replacement = wait_ready().await;
        assert_ne!(replacement["session_id"], session["session_id"]);
        assert_eq!(
            server.inspect_grant_receipt(&g).await.unwrap(),
            inspected,
            "read-only evidence survives session replacement without reviving the old lease"
        );
        assert_eq!(
            json!(server.inspect_grant_result(&g).await.unwrap()),
            json!(retained)
        );
        assert_eq!(
            json!(server.inspect_grant_result(&lost).await.unwrap()),
            json!(recovered)
        );
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "owner marker");
        assert!(
            server
                .invoke_grant(g.clone(), Duration::from_secs(1))
                .await
                .is_err()
        );
        let mut replay = g.clone();
        replay.session_id = replacement["session_id"].as_str().unwrap().into();
        replay.grant_id = Uuid::new_v4().to_string();
        let replayed = server
            .invoke_grant(replay, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(replayed["grant"]["state"], "rejected");
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "owner marker");
        node.abort();
        let _ = node.await;
        server.stop().await;
        let ledger =
            crate::grants::GrantLedger::open(&receipts, Default::default(), "grant-node").unwrap();
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "completed"
        );
    }

    #[tokio::test]
    async fn credential_probe_requires_owner_pin_and_rejects_coordinator_replacement() {
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let key = crate::authentication::generate_credential();
        let policy = crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([(
                "probe-test".into(),
                Some(key.clone()),
            )]),
            tls: Some(identity.clone()),
        };
        let server =
            crate::NodeServer::with_authentication("test-secret", "127.0.0.1", 0, policy).unwrap();
        server.start().await.unwrap();
        let mut config: NodeClientConfig = serde_json::from_value(json!({
            "endpoints":[format!("127.0.0.1:{}",server.port())],"node_id":"probe-test","psk":key,
            "auth_mode":"node_credential"
        }))
        .unwrap();
        let engine = Arc::new(NodeEngine::new(false, vec![], std::env::temp_dir()).unwrap());
        assert!(NodeClient::new(config.clone(), engine.clone()).is_err());
        for valid in [false, true] {
            config.coordinator_certificate = Some(if valid {
                identity.certificate.clone()
            } else {
                crate::tls::CoordinatorIdentity::generate()
                    .unwrap()
                    .certificate
            });
            let client = NodeClient::new(config.clone(), engine.clone()).unwrap();
            let result = client.probe_endpoint(&client.config.endpoints[0]).await;
            assert_eq!(result.is_ok(), valid);
            drop(result);
            assert!(server.connected_nodes().await.is_empty());
            assert!(server.registered_nodes().await.is_empty());
        }
        server.stop().await;
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
                auth_mode: Default::default(),
                coordinator_certificate: None,
                request_limits: Default::default(),
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
