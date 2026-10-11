use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;
use tracing::{info, warn};
use uuid::Uuid;

use crate::NodeCrypto;
use crate::protocol::{PROTOCOL_VERSION, now_ms, read_message, write_message};

pub const DEFAULT_PORT: u16 = 51335;
const AUTH_TIMEOUT: Duration = Duration::from_secs(15);
const AUTH_CLOCK_SKEW_MS: i64 = 5 * 60 * 1000;
const MAX_AUTH_NONCES: usize = 8192;

#[derive(Debug, Error)]
#[error("node '{0}' is not connected")]
pub struct NodeOfflineError(pub String);

#[derive(Debug, Error)]
#[error("node '{0}' outbound queue is full; response was not queued")]
pub struct NodeQueueFullError(pub String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub node_id: String,
    pub source_ip: String,
    pub source_port: u16,
    pub connected_since_ms: u64,
}

/// One live gateway snapshot, without node identities or addresses.
/// Registration here means wire validation, not scheduler/provider readiness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionCounts {
    pub connected_node_count: usize,
    pub registered_node_count: usize,
    pub registration_pending_node_count: usize,
}

/// Latest accepted state for a live registered session. Internal reconciliation
/// data, never a request journal or execution permission. No adapter bindings.
#[derive(Clone, Debug)]
pub struct NodeStateSnapshot {
    pub registration: Value,
    pub heartbeat: Option<Value>,
}

#[derive(Clone, Debug)]
pub enum NodeEvent {
    Registered(Value),
    Heartbeat(Value),
    Submission(Value),
    Down(String),
    GrantAccepted(Value),
}

#[derive(Clone)]
pub struct NodeServer {
    inner: Arc<Inner>,
}

struct Inner {
    host: String,
    port: AtomicU16,
    crypto: Option<NodeCrypto>,
    authentication: crate::authentication::AuthenticationPolicy,
    tls_config: Option<Arc<tokio_rustls::rustls::ServerConfig>>,
    connections: RwLock<HashMap<String, Connection>>,
    routes: RwLock<HashMap<String, ConnectionInfo>>,
    pending: Mutex<HashMap<String, Pending>>,
    nonces: Mutex<HashMap<(String, String), Instant>>,
    events: broadcast::Sender<NodeEvent>,
    accept_task: Mutex<Option<GatewayTask>>,
    active_ids: std::sync::Mutex<HashSet<String>>,
}

struct GatewayTask {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

// Reserve identity atomically through authentication and connection cleanup.
// Two concurrent handshakes must never replace each other's routing entry.
struct IdentityLease {
    inner: Arc<Inner>,
    node_id: String,
}
impl Drop for IdentityLease {
    fn drop(&mut self) {
        self.inner
            .active_ids
            .lock()
            .expect("active identities")
            .remove(&self.node_id);
    }
}

#[derive(Clone)]
struct Connection {
    credential_transport: bool,
    session_id: Uuid,
    info: ConnectionInfo,
    sender: mpsc::Sender<Value>,
    advertisement: Option<crate::network::PeerRecord>,
    state: Option<NodeStateSnapshot>,
    last_seen: Instant,
}

struct Pending {
    observer: Option<Arc<dyn crate::grants::GrantObserver>>,
    grant: Option<crate::grants::ExecutionGrant>,
    acknowledged: bool,
    lease_deadline: Option<Instant>,
    lease_probe_id: Option<String>,
    node_id: String,
    session_id: Uuid,
    sender: oneshot::Sender<Value>,
}

// Command callers may stop waiting before the inner response timeout. Always
// release their correlation entry, including cancellation during a queued send.
struct PendingCleanup {
    inner: Arc<Inner>,
    request_id: String,
}
impl Drop for PendingCleanup {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.inner.pending.try_lock() {
            pending.remove(&self.request_id);
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let inner = self.inner.clone();
            let id = self.request_id.clone();
            runtime.spawn(async move {
                inner.pending.lock().await.remove(&id);
            });
        }
    }
}

impl NodeServer {
    pub fn new(psk: &str, host: impl Into<String>, port: u16) -> Result<Self> {
        Self::with_authentication(psk, host, port, Default::default())
    }

    pub fn with_authentication(
        psk: &str,
        host: impl Into<String>,
        port: u16,
        authentication: crate::authentication::AuthenticationPolicy,
    ) -> Result<Self> {
        authentication.validate()?;
        authentication.validate_legacy_key(psk)?;
        let (events, _) = broadcast::channel(256);
        Ok(Self {
            inner: Arc::new(Inner {
                host: host.into(),
                port: AtomicU16::new(port),
                crypto: if authentication.allow_legacy {
                    Some(NodeCrypto::new(psk)?)
                } else {
                    None
                },
                tls_config: authentication
                    .tls
                    .as_ref()
                    .map(|v| v.server_config())
                    .transpose()?,
                authentication,
                connections: RwLock::new(HashMap::new()),
                routes: RwLock::new(HashMap::new()),
                pending: Mutex::new(HashMap::new()),
                nonces: Mutex::new(HashMap::new()),
                events,
                accept_task: Mutex::new(None),
                active_ids: std::sync::Mutex::new(HashSet::new()),
            }),
        })
    }

    pub fn port(&self) -> u16 {
        self.inner.port.load(Ordering::Relaxed)
    }
    pub fn authentication_policy(&self) -> &crate::authentication::AuthenticationPolicy {
        &self.inner.authentication
    }
    pub fn subscribe(&self) -> broadcast::Receiver<NodeEvent> {
        self.inner.events.subscribe()
    }

    pub async fn start(&self) -> Result<()> {
        let mut task = self.inner.accept_task.lock().await;
        if let Some(gateway) = task.as_ref() {
            if gateway.stop.is_none() {
                bail!("node gateway is stopping; finish stop before restarting")
            }
            return Ok(());
        }
        let listener = TcpListener::bind((self.inner.host.as_str(), self.port()))
            .await
            .with_context(|| format!("bind node gateway {}:{}", self.inner.host, self.port()))?;
        self.inner
            .port
            .store(listener.local_addr()?.port(), Ordering::Relaxed);
        info!(host = %self.inner.host, port = self.port(), "node gateway listening");
        let server = self.clone();
        let (stop, mut stopped) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let mut sessions = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stopped => break,
                    _ = sessions.join_next(), if !sessions.is_empty() => {},
                    accepted = listener.accept() => match accepted {
                        Ok((stream, peer)) => {
                            let server = server.clone();
                            sessions.spawn(async move {
                                if let Err(error) = server.handle_connection(stream, peer).await {
                                    warn!(%peer, %error, "node connection ended");
                                }
                            });
                        }
                        Err(error) => {
                            warn!(%error, "node accept failed");
                            break;
                        }
                    }
                };
            }
            drop(listener);
            // Own and await every session, including incomplete authentication.
            // Aborting a handler drops both socket halves and its identity lease.
            sessions.shutdown().await;
        });
        *task = Some(GatewayTask {
            stop: Some(stop),
            task: Some(handle),
        });
        Ok(())
    }

    pub async fn stop(&self) {
        // Serialize stop/cleanup with start. Retain the handle if this future is
        // cancelled so a subsequent stop can still await the same generation.
        let mut gateway = self.inner.accept_task.lock().await;
        if let Some(task) = gateway.as_mut() {
            if let Some(stop) = task.stop.take() {
                let _ = stop.send(());
            }
            if let Some(handle) = task.task.as_mut() {
                let _ = handle.await;
            }
            task.task = None;
        }
        let disconnected = self
            .inner
            .connections
            .write()
            .await
            .drain()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        for id in disconnected {
            let _ = self.inner.events.send(NodeEvent::Down(id));
        }
        let mut pending = self.inner.pending.lock().await;
        for (id, request) in pending.drain() {
            let _ = request.sender.send(json!({"type": "response", "id": id, "success": false, "error": "node gateway stopped"}));
        }
        *gateway = None;
    }

    pub async fn connected_nodes(&self) -> HashMap<String, ConnectionInfo> {
        self.inner
            .connections
            .read()
            .await
            .iter()
            .map(|(id, connection)| (id.clone(), connection.info.clone()))
            .collect()
    }

    pub async fn registered_nodes(&self) -> HashMap<String, ConnectionInfo> {
        self.inner.routes.read().await.clone()
    }

    pub async fn connection_counts(&self) -> ConnectionCounts {
        let connections = self.inner.connections.read().await;
        let registered = connections
            .values()
            .filter(|connection| connection.advertisement.is_some())
            .count();
        ConnectionCounts {
            connected_node_count: connections.len(),
            registered_node_count: registered,
            registration_pending_node_count: connections.len() - registered,
        }
    }

    pub async fn node_state(&self, node_id: &str) -> Option<NodeStateSnapshot> {
        self.inner
            .connections
            .read()
            .await
            .get(node_id)?
            .state
            .clone()
    }

    /// Heartbeats need only metrics, without cloning/revalidating all models.
    pub async fn node_heartbeat(&self, node_id: &str) -> Option<Value> {
        self.inner
            .connections
            .read()
            .await
            .get(node_id)?
            .state
            .as_ref()?
            .heartbeat
            .clone()
    }

    pub async fn node_states(&self) -> std::collections::BTreeMap<String, NodeStateSnapshot> {
        self.inner
            .connections
            .read()
            .await
            .iter()
            .filter_map(|(id, connection)| Some((id.clone(), connection.state.clone()?)))
            .collect()
    }

    /// Returns only explicitly selected IDs; callers must authorize them first.
    /// Uses live connections, never the unbounded historical route list.
    pub async fn peer_records(
        &self,
        ids: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<(crate::network::PeerRecord, Duration)>> {
        if ids.len() > 257 {
            bail!("peer selection exceeds bounds")
        }
        let connections = self.inner.connections.read().await;
        Ok(ids
            .iter()
            .filter_map(|id| {
                let connection = connections.get(id)?;
                Some((
                    connection.advertisement.clone()?,
                    connection.last_seen.elapsed(),
                ))
            })
            .collect())
    }

    pub async fn invoke_remote(
        &self,
        node_id: &str,
        capability: &str,
        action: &str,
        params: Value,
        wait: Duration,
    ) -> Result<Value> {
        self.invoke_remote_with_context(node_id, capability, action, params, wait, None)
            .await
    }

    pub async fn invoke_remote_with_context(
        &self,
        node_id: &str,
        capability: &str,
        action: &str,
        params: Value,
        wait: Duration,
        context: Option<&crate::context::RequestContext>,
    ) -> Result<Value> {
        crate::execution_boundary::require_control(capability)?;
        self.invoke_message(node_id,json!({"type":"request","capability":capability,"action":action,"params":params,"request_context":context}),wait,None,None,None).await
    }

    /// Issuer/session selection only, never an execution approval. The caller
    /// must persist its reviewed approval and exact issuance before dispatch.
    pub async fn grant_session(&self, node_id: &str) -> Result<Value> {
        let connections = self.inner.connections.read().await;
        let c = connections
            .get(node_id)
            .context("Grant node is not connected")?;
        self.check_grant_connection(c)?;
        self.check_lease_connection(c)?;
        let issuer = self
            .inner
            .authentication
            .tls
            .as_ref()
            .context("Grant issuer TLS unavailable")?
            .fingerprint()?;
        Ok(
            json!({"issuer_id":issuer,"session_id":c.session_id.to_string(),"executor_node":node_id,
            "limits":c.state.as_ref().unwrap().registration["execution_grants"]["limits"],"timing_mode":crate::grant_time::MODE}),
        )
    }
    fn check_grant_connection(&self, c: &Connection) -> Result<()> {
        if !c.credential_transport
            || c.state.as_ref().is_none_or(|s| {
                s.registration["execution_grants"]["schema_version"] != 1
                    || s.registration["execution_grants"]["available"] != true
            })
        {
            bail!(
                "Node requires an independent credential, pinned TLS and available grant receipt journal"
            )
        }
        Ok(())
    }
    fn check_lease_connection(&self, c: &Connection) -> Result<()> {
        if c.state.as_ref().is_none_or(|s| {
            s.registration["execution_grants"]["timing_mode"] != crate::grant_time::MODE
        }) {
            bail!("Node requires fresh issuer lease challenges; no wall-clock fallback")
        }
        Ok(())
    }
    pub async fn invoke_grant(
        &self,
        grant: crate::grants::ExecutionGrant,
        wait: Duration,
    ) -> Result<Value> {
        grant.validate()?;
        let node = grant.executor_node.clone();
        self.invoke_message(
            &node,
            json!({"type":"execute_grant","grant":grant}),
            wait,
            Some(grant),
            None,
            None,
        )
        .await
    }
    pub async fn invoke_grant_observed(
        &self,
        grant: crate::grants::ExecutionGrant,
        wait: Duration,
        observer: Arc<dyn crate::grants::GrantObserver>,
        lease_deadline: Instant,
    ) -> Result<Value> {
        grant.validate()?;
        let node = grant.executor_node.clone();
        self.invoke_message(
            &node,
            json!({"type":"execute_grant","grant":grant}),
            wait,
            Some(grant),
            Some(observer),
            Some(lease_deadline),
        )
        .await
    }
    /// Read the current authenticated executor's journal for a previously issued grant.
    /// Older sessions/issuers and missing or mismatched receipts never imply effects did not occur.
    pub async fn inspect_grant_receipt(
        &self,
        grant: &crate::grants::ExecutionGrant,
    ) -> Result<crate::request_receipt::ConsumedReceipt> {
        grant.validate()?;
        let response = self.invoke_message(&grant.executor_node,
            json!({"type":"grant_receipt","grant_id":grant.grant_id,"expected_node_id":grant.executor_node,"expected_issuer_id":grant.issuer_id}),
            Duration::from_secs(10), None, None, None).await?;
        if response["success"] != true {
            bail!("Consumed receipt unavailable; effects remain unresolved")
        }
        let receipt: crate::request_receipt::ConsumedReceipt =
            serde_json::from_value(response["data"].clone())?;
        if !receipt.confirms(grant) {
            bail!("Receipt does not match the exact issued grant")
        }
        Ok(receipt)
    }
    /// Explicit output evidence read, never execution/reconciliation or lease renewal.
    pub async fn inspect_grant_result(
        &self,
        grant: &crate::grants::ExecutionGrant,
    ) -> Result<crate::request_result::RetainedResult> {
        grant.validate()?;
        let response = self.invoke_message(&grant.executor_node,
            json!({"type":"grant_result","grant_id":grant.grant_id,"expected_node_id":grant.executor_node,
                "expected_issuer_id":grant.issuer_id,"expected_grant_sha256":crate::grants::definition_digest(&json!(grant))?}),
            Duration::from_secs(10),None,None,None).await?;
        if response["success"] != true {
            bail!("Retained result unavailable; no execution or fallback")
        }
        let result: crate::request_result::RetainedResult =
            serde_json::from_value(response["data"].clone())?;
        result.validate()?;
        if !result.confirms(grant) {
            bail!("Retained result does not match exact issuance")
        }
        Ok(result)
    }
    async fn invoke_message(
        &self,
        node_id: &str,
        mut message: Value,
        wait: Duration,
        grant: Option<crate::grants::ExecutionGrant>,
        observer: Option<Arc<dyn crate::grants::GrantObserver>>,
        lease_deadline: Option<Instant>,
    ) -> Result<Value> {
        // Start before routing/queue waits. The reviewed driver also supplies
        // its mint-time monotonic deadline, before durable issuance journal I/O.
        let lease_deadline = if let Some(g) = &grant {
            let started = Instant::now();
            let now = now_ms();
            if now < g.issued_at_ms || now >= g.expires_at_ms {
                bail!("Issuer grant clock or expiry invalid")
            }
            let local = started
                + Duration::from_millis(g.limits.max_execution_ms.min(g.expires_at_ms - now));
            let deadline = lease_deadline.unwrap_or(local).min(local);
            if deadline <= Instant::now() {
                bail!("Issued monotonic lease already expired")
            }
            Some(deadline)
        } else {
            None
        };
        // Hold the routing read guard until the waiter is registered. Session
        // cleanup removes the route before draining that session's waiters.
        let connections = self.inner.connections.read().await;
        let connection = connections
            .get(node_id)
            .cloned()
            .ok_or_else(|| NodeOfflineError(node_id.into()))?;
        if message["type"] == "grant_receipt" || message["type"] == "grant_result" {
            self.check_grant_connection(&connection)?;
            if message["type"] == "grant_result" {
                let support: crate::grants::GrantAdvertisement = serde_json::from_value(
                    connection
                        .state
                        .as_ref()
                        .context("Result registration unavailable")?
                        .registration["execution_grants"]
                        .clone(),
                )?;
                support.validate()?;
                if !support.retained_results {
                    bail!("Executor does not advertise retained results; no fallback")
                }
            }
            let issuer = self
                .inner
                .authentication
                .tls
                .as_ref()
                .context("Receipt issuer unavailable")?
                .fingerprint()?;
            if message["expected_issuer_id"] != issuer || message["expected_node_id"] != node_id {
                bail!("Receipt issuer or node changed; use local owner inspection")
            }
        }
        if let Some(g) = &grant {
            self.check_grant_connection(&connection)?;
            self.check_lease_connection(&connection)?;
            let issuer = self
                .inner
                .authentication
                .tls
                .as_ref()
                .context("Grant issuer unavailable")?
                .fingerprint()?;
            if !g.matches_session(&issuer, &connection.session_id.to_string(), node_id) {
                bail!("Grant issuer/session changed before dispatch")
            }
        }
        let request_id = Uuid::new_v4().simple().to_string();
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().await.insert(
            request_id.clone(),
            Pending {
                observer,
                grant,
                acknowledged: false,
                lease_deadline,
                lease_probe_id: None,
                node_id: node_id.into(),
                session_id: connection.session_id,
                sender,
            },
        );
        drop(connections);
        let _cleanup = PendingCleanup {
            inner: self.inner.clone(),
            request_id: request_id.clone(),
        };
        message["id"] = json!(request_id);
        // The same budget covers a saturated outbound queue and the reply.
        match timeout(wait, async {
            connection
                .sender
                .send(message)
                .await
                .map_err(|_| NodeOfflineError(node_id.into()))?;
            receiver
                .await
                .with_context(|| format!("node '{node_id}' disconnected before responding"))
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                bail!(
                    "node '{node_id}' did not answer within {}s",
                    wait.as_secs_f64()
                )
            }
        }
    }

    /// Best-effort response admission, not delivery acknowledgement. Never wait
    /// behind a saturated node: this is also called by the shared event listener.
    pub async fn send_to_node(&self, node_id: &str, message: Value) -> Result<()> {
        let connection = self
            .inner
            .connections
            .read()
            .await
            .get(node_id)
            .cloned()
            .ok_or_else(|| NodeOfflineError(node_id.into()))?;
        connection
            .sender
            .try_send(message)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => NodeQueueFullError(node_id.into()).into(),
                mpsc::error::TrySendError::Closed(_) => NodeOfflineError(node_id.into()).into(),
            })
    }

    /// Reply only to the authenticated session that originally submitted input.
    /// A reconnect with the same node ID is not the same response destination.
    pub async fn send_submission_reply(&self, submission: &Value, response: Value) -> Result<()> {
        if submission["type"] != "submit"
            || response["type"] != "submission_result"
            || submission["id"]
                .as_str()
                .is_none_or(|id| id.is_empty() || id.len() > 128)
            || response["id"] != submission["id"]
        {
            bail!("Submission reply does not match its input")
        }
        let node_id = submission["node_id"]
            .as_str()
            .context("Missing submission origin")?;
        let session_id = submission["gateway_session_id"]
            .as_str()
            .context("Missing gateway session")?;
        let parsed = Uuid::parse_str(session_id)?;
        if parsed.is_nil() || parsed.to_string() != session_id {
            bail!("Invalid gateway session")
        }
        let session_id = parsed;
        let connections = self.inner.connections.read().await;
        let connection = connections
            .get(node_id)
            .filter(|c| c.session_id == session_id)
            .ok_or_else(|| NodeOfflineError(node_id.into()))?;
        connection
            .sender
            .try_send(response)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => NodeQueueFullError(node_id.into()).into(),
                mpsc::error::TrySendError::Closed(_) => NodeOfflineError(node_id.into()).into(),
            })
    }

    async fn handle_connection(&self, stream: TcpStream, peer: SocketAddr) -> Result<()> {
        let mut reader = BufReader::new(stream);
        // The selector is untrusted and public. It chooses an exact key only;
        // it never reserves an identity or permits fallback to the network key.
        let first = timeout(
            AUTH_TIMEOUT,
            crate::protocol::read_wire_line(&mut reader, 8192),
        )
        .await
        .context("auth timed out")??
        .context("connection closed before auth")?;
        let (crypto, selected_id, stream, initial_auth) = if first.first() == Some(&b'{') {
            if first.len() > 1024 {
                bail!("credential selector exceeds bounds")
            }
            let selector: crate::authentication::CredentialSelector =
                serde_json::from_slice(&first)
                    .map_err(|_| anyhow::anyhow!("invalid credential selector"))?;
            if selector.r#type != "node_credential" || selector.schema_version != 2 {
                bail!("invalid credential selector")
            }
            crate::network::validate_remote_node_id(&selector.node_id)?;
            let secret = self
                .inner
                .authentication
                .credentials
                .get(&selector.node_id)
                .and_then(Option::as_ref)
                .context("node credential unavailable")?;
            let crypto = NodeCrypto::new(secret)?;
            let config = self
                .inner
                .tls_config
                .as_ref()
                .context("credential TLS identity unavailable; owner provisioning required")?;
            // Keep the buffered reader: selector and TLS ClientHello may arrive
            // together, and dropping prefetched bytes would break the upgrade.
            let stream = timeout(AUTH_TIMEOUT, crate::tls::accept(reader, config.clone()))
                .await
                .context("credential TLS timed out")??;
            (crypto, Some(selector.node_id), stream, None)
        } else {
            if !self.inner.authentication.allow_legacy {
                bail!("legacy authentication disabled")
            }
            let crypto = self
                .inner
                .crypto
                .as_ref()
                .context("Legacy authentication key unavailable")?;
            (
                crypto.clone(),
                None,
                Box::new(reader) as crate::tls::NodeStream,
                Some(crate::protocol::decode_line(crypto, &first)?),
            )
        };
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);
        let mut auth = if let Some(auth) = initial_auth {
            auth
        } else {
            timeout(AUTH_TIMEOUT, read_message(&crypto, &mut reader))
                .await
                .context("credential auth timed out")??
                .context("connection closed before credential auth")?
        };
        // A bounded, encrypted service probe does not register or replace a node.
        if auth["type"] == "probe" {
            let nonce = auth["nonce"]
                .as_str()
                .filter(|s| s.len() == 32)
                .context("invalid probe challenge")?;
            timeout(
                Duration::from_secs(5),
                write_message(
                    &crypto,
                    &mut writer,
                    &json!({"type":"pong","nonce":nonce,"protocol":PROTOCOL_VERSION}),
                ),
            )
            .await
            .context("probe reply timed out")??;
            auth = timeout(AUTH_TIMEOUT, read_message(&crypto, &mut reader))
                .await
                .context("auth after probe timed out")??
                .context("connection closed after probe")?;
        }
        let verification = async {
            let claimed = auth["node_id"].as_str().context("auth failed")?;
            // Validate before consulting credentials/tombstones, and verify the
            // same exact ID below. Trimming only after the lookup permits
            // whitespace aliases to bypass protected/revoked legacy policy.
            crate::network::validate_remote_node_id(claimed)?;
            match selected_id.as_deref() {
                Some(id) if id != claimed => bail!("credential identity mismatch"),
                None if self.inner.authentication.credentials.contains_key(claimed) => {
                    bail!("identity requires its node credential")
                }
                _ => {}
            }
            self.verify_auth_with(&crypto, &auth).await
        }
        .await;
        let node_id = match verification {
            Ok(node_id) => node_id,
            Err(error) => {
                let _ = write_message(
                    &crypto,
                    &mut writer,
                    &json!({"type": "denied", "error": error.to_string()}),
                )
                .await;
                return Err(error);
            }
        };
        let reserved = self
            .inner
            .active_ids
            .lock()
            .expect("active identities")
            .insert(node_id.clone());
        if !reserved {
            let denied = json!({"type": "denied", "error": "duplicate node id already connected"});
            write_message(&crypto, &mut writer, &denied).await?;
            bail!("duplicate node id '{node_id}'");
        }
        let _identity = IdentityLease {
            inner: self.inner.clone(),
            node_id: node_id.clone(),
        };
        let session_id = Uuid::new_v4();
        write_message(
            &crypto,
            &mut writer,
            &json!({"type": "ok", "protocol": PROTOCOL_VERSION,"grant_session_id":session_id.to_string()}),
        )
        .await?;

        let (send, mut receive) = mpsc::channel::<Value>(64);
        let info = ConnectionInfo {
            node_id: node_id.clone(),
            source_ip: peer.ip().to_string(),
            source_port: peer.port(),
            connected_since_ms: now_ms(),
        };
        self.inner
            .routes
            .write()
            .await
            .insert(node_id.clone(), info.clone());
        self.inner.connections.write().await.insert(
            node_id.clone(),
            Connection {
                credential_transport: selected_id.is_some(),
                session_id,
                info,
                sender: send,
                advertisement: None,
                state: None,
                last_seen: Instant::now(),
            },
        );

        // Neither direction is detached: shutdown/cancellation of this session
        // must release both halves, including a stalled outbound write.
        let result = tokio::select! {
            result = self.serve_session(&node_id, session_id, &crypto, &mut reader) => result,
            result = async {
                while let Some(message) = receive.recv().await {
                    write_message(&crypto, &mut writer, &message).await?;
                }
                Ok(())
            } => result,
        };
        self.inner.connections.write().await.remove(&node_id);
        self.fail_pending_for(&node_id, session_id).await;
        let _ = self.inner.events.send(NodeEvent::Down(node_id));
        result
    }

    #[cfg(test)]
    async fn serve<R>(&self, node_id: &str, crypto: &NodeCrypto, reader: &mut R) -> Result<()>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        self.serve_session(node_id, Uuid::nil(), crypto, reader)
            .await
    }

    async fn serve_session<R>(
        &self,
        authenticated_id: &str,
        session_id: Uuid,
        crypto: &NodeCrypto,
        reader: &mut R,
    ) -> Result<()>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        let mut metadata = crate::context::NodeMetadata::default();
        let mut interpretation_capabilities = Vec::<String>::new();
        let mut registered = false;
        while let Some(mut message) = read_message(crypto, reader).await? {
            match message.get("type").and_then(Value::as_str) {
                Some("register") => {
                    require_node_id(&message, authenticated_id)?;
                    metadata = serde_json::from_value(
                        message
                            .get("metadata")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    )?;
                    metadata.validate()?;
                    let network: crate::network::NetworkAdvertisement = serde_json::from_value(
                        message.get("network").cloned().unwrap_or_else(|| json!({})),
                    )?;
                    network.validate()?;
                    message["network"] = json!(network);
                    let capabilities: Vec<String> = serde_json::from_value(
                        message
                            .get("capabilities")
                            .cloned()
                            .unwrap_or_else(|| json!([])),
                    )?;
                    metadata = metadata.advertised(&capabilities);
                    message["metadata"] = json!(metadata);
                    if let Some(advertisement) = message.get("execution_grants") {
                        let a: crate::grants::GrantAdvertisement =
                            serde_json::from_value(advertisement.clone())?;
                        a.validate()?;
                    }
                    let advertisement = crate::network::PeerRecord::from_registration(&message)?;
                    interpretation_capabilities = capabilities;
                    let received_at_ms = now_ms();
                    message["gateway_received_ms"] = json!(received_at_ms);
                    if let Some(connection) = self
                        .inner
                        .connections
                        .write()
                        .await
                        .get_mut(authenticated_id)
                    {
                        connection.advertisement = Some(advertisement);
                        connection.state = Some(NodeStateSnapshot {
                            registration: state_fields(
                                &message,
                                &[
                                    "node_id",
                                    "name",
                                    "version",
                                    "specs",
                                    "capabilities",
                                    "models",
                                    "metadata",
                                    "network",
                                    "execution_grants",
                                ],
                                received_at_ms,
                            ),
                            // Old specs/metrics must not overwrite a refreshed
                            // registration until this generation has a heartbeat.
                            heartbeat: None,
                        });
                        connection.last_seen = Instant::now();
                    }
                    registered = true;
                    message["node_id"] = Value::String(authenticated_id.into());
                    let _ = self.inner.events.send(NodeEvent::Registered(message));
                }
                Some("heartbeat") => {
                    require_node_id(&message, authenticated_id)?;
                    if !registered {
                        bail!("register before sending heartbeats")
                    }
                    let received_at_ms = now_ms();
                    message["gateway_received_ms"] = json!(received_at_ms);
                    if let Some(connection) = self
                        .inner
                        .connections
                        .write()
                        .await
                        .get_mut(authenticated_id)
                    {
                        connection.last_seen = Instant::now();
                        if let Some(state) = &mut connection.state {
                            state.heartbeat = Some(state_fields(
                                &message,
                                &["node_id", "load", "latency_ms", "specs"],
                                received_at_ms,
                            ));
                        }
                    }
                    message["node_id"] = Value::String(authenticated_id.into());
                    let _ = self.inner.events.send(NodeEvent::Heartbeat(message));
                }
                Some("submit") => {
                    require_node_id(&message, authenticated_id)?;
                    if !registered {
                        bail!("register before submitting work")
                    }
                    let id = message["id"]
                        .as_str()
                        .context("submission requires an id")?;
                    let request_id =
                        match message.get("operation_id").filter(|value| !value.is_null()) {
                            Some(value) => {
                                let id = value
                                    .as_str()
                                    .context("operation ID must be a UUID string")?;
                                crate::job_commands::validate_operation_id(id)?;
                                id
                            }
                            None => id,
                        };
                    let origin = crate::context::RequestContext::new(
                        request_id.into(),
                        authenticated_id.into(),
                        metadata.area.clone(),
                    )?;
                    message["request_context"] = json!(origin);
                    // Overwrite caller-supplied context with the same accepted
                    // registration snapshot as this session's origin area.
                    message["interpretation_capabilities"] = json!(interpretation_capabilities);
                    message["node_id"] = Value::String(authenticated_id.into());
                    message["gateway_session_id"] = json!(session_id.to_string());
                    let _ = self.inner.events.send(NodeEvent::Submission(message));
                }
                Some("grant_ack") => {
                    self.resolve_grant_ack(authenticated_id, session_id, message)
                        .await
                }
                Some("grant_lease_probe") => {
                    if !registered {
                        bail!("register before requesting a grant lease")
                    }
                    self.reply_grant_lease(authenticated_id, session_id, message)
                        .await?;
                }
                Some("response") => {
                    self.resolve_response(authenticated_id, session_id, message)
                        .await
                }
                Some("announce") => {}
                Some(kind) => warn!(
                    node_id = authenticated_id,
                    kind, "ignored unknown node message"
                ),
                None => warn!(node_id = authenticated_id, "ignored message without type"),
            }
        }
        Ok(())
    }

    async fn verify_auth_with(&self, crypto: &NodeCrypto, auth: &Value) -> Result<String> {
        if auth.get("type").and_then(Value::as_str) != Some("auth") {
            bail!("auth failed")
        }
        if auth.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
            bail!("unsupported protocol")
        }
        let text = |key| auth.get(key).and_then(Value::as_str).context("auth failed");
        let node_id = text("node_id")?;
        crate::network::validate_remote_node_id(node_id)?;
        let version = text("version")?;
        let nonce = text("nonce")?;
        let timestamp = text("timestamp")?;
        let signature = text("signature")?;
        if version.len() > 128
            || nonce.is_empty()
            || nonce.len() > 128
            || nonce.chars().any(char::is_control)
            || timestamp.len() > 20
            || signature.len() > 128
        {
            bail!("invalid node id")
        }
        let timestamp_ms: i64 = timestamp.parse().context("invalid auth timestamp")?;
        let local_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if local_ms.abs_diff(timestamp_ms) > AUTH_CLOCK_SKEW_MS as u64 {
            bail!("stale auth timestamp")
        }
        let raw = format!("{nonce}|{node_id}|{version}|{timestamp}");
        if !crypto.verify(raw.as_bytes(), signature) {
            bail!("auth failed")
        }
        let mut nonces = self.inner.nonces.lock().await;
        nonces.retain(|_, seen| seen.elapsed() < Duration::from_secs(10 * 60));
        let receipt = (node_id.to_owned(), nonce.to_owned());
        if nonces.contains_key(&receipt) {
            bail!("replayed auth")
        }
        if nonces.len() >= MAX_AUTH_NONCES {
            bail!("authentication replay cache at capacity")
        }
        nonces.insert(receipt, Instant::now());
        Ok(node_id.into())
    }

    async fn reply_grant_lease(&self, node: &str, session: Uuid, message: Value) -> Result<()> {
        let query: crate::grant_time::LeaseQuery = serde_json::from_value(message)?;
        query.validate()?;
        if query.node_id != node || query.session_id != session.to_string() {
            bail!("Lease query identity changed")
        }
        let connections = self.inner.connections.read().await;
        let connection = connections
            .get(node)
            .filter(|c| c.session_id == session)
            .context("Lease session retired")?;
        self.check_grant_connection(connection)?;
        self.check_lease_connection(connection)?;
        let mut pending = self.inner.pending.lock().await;
        let request = pending
            .get_mut(&query.request_id)
            .context("Lease issuance no longer pending")?;
        if request.node_id != node
            || request.session_id != session
            || request.acknowledged
            || request.lease_probe_id.is_some()
            || !request.grant.as_ref().is_some_and(|g| query.confirms(g))
        {
            bail!("Lease query mismatched or already used")
        }
        let deadline = request
            .lease_deadline
            .context("Pending request has no issued lease")?;
        let remaining_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(crate::grants::MAX_DURATION_MS as u128) as u64;
        let reply = crate::grant_time::LeaseReply::new(&query, remaining_ms);
        // One proof only, never renewal. Queue residence is included in the
        // node's complete measured round trip and cannot extend this deadline.
        request.lease_probe_id = Some(query.id.clone());
        connection
            .sender
            .try_send(json!(reply))
            .context("Lease reply queue unavailable")?;
        Ok(())
    }
    async fn resolve_response(&self, authenticated_id: &str, session_id: Uuid, message: Value) {
        let Some(id) = message.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return;
        };
        let mut requests = self.inner.pending.lock().await;
        if !requests.get(&id).is_some_and(|pending| {
            pending.node_id == authenticated_id
                && pending.session_id == session_id
                && pending.grant.as_ref().is_none_or(|g| {
                    serde_json::from_value::<crate::grants::GrantReport>(message["grant"].clone())
                        .is_ok_and(|r| {
                            r.confirms(g)
                                && r.state != crate::grants::ReceiptState::Accepted
                                && message["success"] == json!(r.success)
                                && if r.state == crate::grants::ReceiptState::Rejected {
                                    !pending.acknowledged
                                } else {
                                    pending.acknowledged
                                }
                        })
                })
        }) {
            return;
        }
        if let Some(pending) = requests.remove(&id) {
            if let Some(observer) = &pending.observer {
                let report =
                    serde_json::from_value::<crate::grants::GrantReport>(message["grant"].clone())
                        .expect("validated grant report");
                if observer.record(&report).is_err() {
                    let _ = pending.sender.send(json!({"success":false,"error_type":"GrantJournalError","error":"Grant completion checkpoint failed; effects may have happened"}));
                    return;
                }
            }
            let _ = pending.sender.send(message);
        }
    }
    async fn resolve_grant_ack(&self, node: &str, session: Uuid, message: Value) {
        let Some(id) = message["id"].as_str() else {
            return;
        };
        let mut pending = self.inner.pending.lock().await;
        let Some(p) = pending
            .get_mut(id)
            .filter(|p| p.node_id == node && p.session_id == session && !p.acknowledged)
        else {
            return;
        };
        let Some(g) = &p.grant else { return };
        let Ok(r) = serde_json::from_value::<crate::grants::GrantReport>(message["grant"].clone())
        else {
            return;
        };
        if r.state == crate::grants::ReceiptState::Accepted
            && r.confirms(g)
            && p.lease_probe_id.is_some()
            && p.lease_deadline
                .is_some_and(|deadline| Instant::now() < deadline)
        {
            if p.observer.as_ref().is_some_and(|o| o.record(&r).is_err()) {
                if let Some(p) = pending.remove(id) {
                    let _ = p.sender.send(json!({"success":false,"error_type":"GrantJournalError","error":"Grant acceptance checkpoint failed; effects may have happened"}));
                }
                return;
            }
            p.acknowledged = true;
            let _ = self.inner.events.send(NodeEvent::GrantAccepted(json!(r)));
        }
    }

    async fn fail_pending_for(&self, node_id: &str, session_id: Uuid) {
        let mut pending = self.inner.pending.lock().await;
        let ids = pending
            .iter()
            .filter(|(_, request)| request.node_id == node_id && request.session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(request) = pending.remove(&id) {
                let _ = request.sender.send(json!({"type": "response", "id": id, "success": false, "error": "node disconnected"}));
            }
        }
    }
}

fn state_fields(message: &Value, fields: &[&str], received_at_ms: u64) -> Value {
    let mut snapshot = serde_json::Map::new();
    for field in fields {
        if let Some(value) = message.get(*field) {
            snapshot.insert((*field).into(), value.clone());
        }
    }
    // Gateway receipt time, never a node-authored timestamp. Reconciliation
    // must preserve original freshness instead of making cached state new.
    snapshot.insert("gateway_received_ms".into(), json!(received_at_ms));
    Value::Object(snapshot)
}

fn require_node_id(message: &Value, authenticated_id: &str) -> Result<()> {
    if message.get("node_id").and_then(Value::as_str) != Some(authenticated_id) {
        bail!("node id does not match authenticated connection")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_legacy_transport_has_no_key_and_does_not_require_a_dummy_secret() {
        let policy = crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            ..Default::default()
        };
        for unused in ["", "CHANGE_ME", "short"] {
            let server =
                NodeServer::with_authentication(unused, "127.0.0.1", 0, policy.clone()).unwrap();
            assert!(server.inner.crypto.is_none());
        }
        assert!(NodeServer::new("", "127.0.0.1", 0).is_err());
        let mut invalid = policy;
        invalid
            .credentials
            .insert("owner".into(), Some("weak".into()));
        assert!(
            NodeServer::with_authentication("", "127.0.0.1", 0, invalid).is_err(),
            "disabled legacy is not a credential validation bypass"
        );
    }

    #[tokio::test]
    async fn legacy_auth_rejects_identity_aliases_before_policy_and_replay_receipts() {
        let policy = crate::authentication::AuthenticationPolicy {
            credentials: std::collections::BTreeMap::from([
                ("protected".into(), Some("a".repeat(64))),
                ("revoked".into(), None),
            ]),
            ..Default::default()
        };
        let server =
            NodeServer::with_authentication("legacy-secret", "127.0.0.1", 0, policy).unwrap();
        server.start().await.unwrap();
        for (signed, claimed) in [
            ("protected", " protected"),
            ("protected", "protected "),
            ("revoked", " revoked "),
            ("revoked", "\u{2003}revoked\u{2003}"),
            ("local", "local"),
            ("*", "*"),
            ("bad\0id", "bad\0id"),
        ] {
            let stream = TcpStream::connect(("127.0.0.1", server.port()))
                .await
                .unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let crypto = NodeCrypto::new("legacy-secret").unwrap();
            let mut auth = crate::protocol::build_auth(&crypto, signed, crate::VERSION);
            // Keep the signature over the canonical ID. The previous server
            // consulted policy before trimming, then verified the trimmed ID.
            auth["node_id"] = json!(claimed);
            write_message(&crypto, &mut writer, &auth).await.unwrap();
            let response = timeout(Duration::from_secs(3), read_message(&crypto, &mut reader))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(response["type"], "denied", "claimed={claimed:?}");
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts::default()
            );
            assert!(server.inner.active_ids.lock().unwrap().is_empty());
            assert!(server.inner.nonces.lock().await.is_empty());
        }
        server.stop().await;
    }

    #[tokio::test]
    async fn credential_transport_refuses_plaintext_downgrade_and_retires_partial_tls() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let key = "a".repeat(64);
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let policy = crate::authentication::AuthenticationPolicy {
            credentials: std::collections::BTreeMap::from([("a".into(), Some(key.clone()))]),
            tls: Some(identity),
            ..Default::default()
        };
        let server =
            NodeServer::with_authentication("legacy-secret", "127.0.0.1", 0, policy).unwrap();
        server.start().await.unwrap();
        for version in [1, 2] {
            let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
                .await
                .unwrap();
            let line = format!(
                "{{\"type\":\"node_credential\",\"schema_version\":{version},\"node_id\":\"a\"}}\n"
            );
            stream.write_all(line.as_bytes()).await.unwrap();
            // Static-key framing must not be accepted outside TLS, even though
            // the correct independent secret can authenticate that inner frame.
            let crypto = NodeCrypto::new(&key).unwrap();
            let _ = write_message(
                &crypto,
                &mut stream,
                &crate::protocol::build_auth(&crypto, "a", crate::VERSION),
            )
            .await;
            let mut response = Vec::new();
            let _ = timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
                .await
                .unwrap();
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts::default()
            );
            assert!(server.inner.nonces.lock().await.is_empty());
        }
        let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
            .await
            .unwrap();
        crate::authentication::write_selector(&mut stream, "a")
            .await
            .unwrap();
        // Shutdown owns a TLS handshake too; no detached task/identity may linger.
        timeout(Duration::from_secs(3), server.stop())
            .await
            .unwrap();
        let mut response = Vec::new();
        let _ = timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
            .await
            .unwrap();
        assert!(server.inner.active_ids.lock().unwrap().is_empty());
        assert_eq!(
            server.connection_counts().await,
            ConnectionCounts::default()
        );
    }

    #[tokio::test]
    async fn independent_keys_enforce_identity_and_never_downgrade() {
        let key_a = "a".repeat(64);
        let key_b = "b".repeat(64);
        for (selector, claimed, secret, allow_legacy, accepted) in [
            (Some("a"), "a", key_a.as_str(), true, true),
            (Some("b"), "b", key_b.as_str(), false, true),
            (Some("b"), "b", key_a.as_str(), true, false),
            (Some("a"), "b", key_a.as_str(), true, false),
            (Some("a"), "a", "legacy-secret", true, false),
            (Some("revoked"), "revoked", key_a.as_str(), true, false),
            (Some("unknown"), "unknown", key_a.as_str(), true, false),
            (None, "a", "legacy-secret", true, false),
            (None, "revoked", "legacy-secret", true, false),
            (None, "old-node", "legacy-secret", true, true),
            (None, "old-node", "legacy-secret", false, false),
        ] {
            let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
            let policy = crate::authentication::AuthenticationPolicy {
                allow_legacy,
                tls: Some(identity.clone()),
                credentials: std::collections::BTreeMap::from([
                    ("a".into(), Some(key_a.clone())),
                    ("b".into(), Some(key_b.clone())),
                    ("revoked".into(), None),
                ]),
            };
            let server =
                NodeServer::with_authentication("legacy-secret", "127.0.0.1", 0, policy).unwrap();
            let mut events = server.subscribe();
            server.start().await.unwrap();
            let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
                .await
                .unwrap();
            let stream: crate::tls::NodeStream = if let Some(id) = selector {
                crate::authentication::write_selector(&mut stream, id)
                    .await
                    .unwrap();
                let secured = crate::tls::connect(
                    stream,
                    &identity.certificate,
                    crate::tls::client_config(&identity.certificate).unwrap(),
                )
                .await;
                if matches!(id, "unknown" | "revoked") {
                    assert!(secured.is_err());
                    assert_eq!(
                        server.connection_counts().await,
                        ConnectionCounts::default()
                    );
                    assert!(server.inner.nonces.lock().await.is_empty());
                    server.stop().await;
                    continue;
                }
                secured.unwrap()
            } else {
                Box::new(stream)
            };
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let crypto = NodeCrypto::new(secret).unwrap();
            // Send auth without a probe to exercise both supported entry paths.
            let _ = write_message(
                &crypto,
                &mut writer,
                &crate::protocol::build_auth(&crypto, claimed, crate::VERSION),
            )
            .await;
            let reply = timeout(Duration::from_secs(3), read_message(&crypto, &mut reader))
                .await
                .unwrap();
            let ok = reply.ok().flatten().is_some_and(|v| v["type"] == "ok");
            assert_eq!(
                ok, accepted,
                "selector={selector:?} claimed={claimed} legacy={allow_legacy}"
            );
            if !accepted {
                assert_eq!(
                    server.connection_counts().await,
                    ConnectionCounts::default()
                );
                assert!(
                    server.inner.nonces.lock().await.is_empty(),
                    "unauthorized identity must not poison the replay cache"
                );
            } else {
                write_message(
                    &crypto,
                    &mut writer,
                    &json!({"type":"register","node_id":claimed}),
                )
                .await
                .unwrap();
                assert!(matches!(
                    timeout(Duration::from_secs(3), events.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                    NodeEvent::Registered(_)
                ));
                // Exercise the selected key in both directions after authentication.
                server
                    .send_to_node(claimed, json!({"type":"test","private":"pair-only"}))
                    .await
                    .unwrap();
                assert_eq!(
                    read_message(&crypto, &mut reader).await.unwrap().unwrap()["private"],
                    "pair-only"
                );
            }
            server.stop().await;
            assert!(server.inner.active_ids.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn extreme_auth_timestamps_fail_closed_without_panicking() {
        let server = NodeServer::new("legacy-secret", "127.0.0.1", 0).unwrap();
        for timestamp in [i64::MIN, i64::MAX] {
            let mut auth = crate::protocol::build_auth(
                server.inner.crypto.as_ref().unwrap(),
                "a",
                crate::VERSION,
            );
            auth["timestamp"] = json!(timestamp.to_string());
            assert!(
                server
                    .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn bounded_legacy_timestamp_nonces_remain_compatible() {
        let server = NodeServer::new("legacy-secret", "127.0.0.1", 0).unwrap();
        let mut auth =
            crate::protocol::build_auth(server.inner.crypto.as_ref().unwrap(), "old-node", "0.3.2");
        auth["nonce"] = json!(crate::protocol::now_ms().to_string());
        let raw = format!(
            "{}|old-node|0.3.2|{}",
            auth["nonce"].as_str().unwrap(),
            auth["timestamp"].as_str().unwrap()
        );
        auth["signature"] = json!(server.inner.crypto.as_ref().unwrap().sign(raw.as_bytes()));
        assert_eq!(
            server
                .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                .await
                .unwrap(),
            "old-node"
        );
        assert!(
            server
                .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn exact_identity_nonce_pairs_cannot_collide_through_delimiters() {
        let server = NodeServer::new("legacy-secret", "127.0.0.1", 0).unwrap();
        for (id, nonce) in [("a:b", "c"), ("a", "b:c")] {
            let mut auth = crate::protocol::build_auth(
                server.inner.crypto.as_ref().unwrap(),
                id,
                crate::VERSION,
            );
            auth["nonce"] = json!(nonce);
            let raw = format!(
                "{nonce}|{id}|{}|{}",
                crate::VERSION,
                auth["timestamp"].as_str().unwrap()
            );
            auth["signature"] = json!(server.inner.crypto.as_ref().unwrap().sign(raw.as_bytes()));
            assert_eq!(
                server
                    .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                    .await
                    .unwrap(),
                id
            );
            assert!(
                server
                    .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                    .await
                    .is_err()
            );
        }
        assert_eq!(server.inner.nonces.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn authentication_replay_cache_is_bounded_and_rejections_do_not_extend_receipts() {
        let server = NodeServer::new("legacy-secret", "127.0.0.1", 0).unwrap();
        let auth =
            crate::protocol::build_auth(server.inner.crypto.as_ref().unwrap(), "a", crate::VERSION);
        server
            .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
            .await
            .unwrap();
        let original = server.inner.nonces.lock().await.clone();
        assert!(
            server
                .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &auth)
                .await
                .is_err()
        );
        assert_eq!(*server.inner.nonces.lock().await, original);
        {
            let mut cache = server.inner.nonces.lock().await;
            for n in 1..MAX_AUTH_NONCES {
                cache.insert(("fixture".into(), n.to_string()), Instant::now());
            }
        }
        let fresh =
            crate::protocol::build_auth(server.inner.crypto.as_ref().unwrap(), "a", crate::VERSION);
        assert!(
            server
                .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &fresh)
                .await
                .is_err()
        );
        assert_eq!(server.inner.nonces.lock().await.len(), MAX_AUTH_NONCES);
        server
            .inner
            .nonces
            .lock()
            .await
            .values_mut()
            .for_each(|seen| *seen = Instant::now() - Duration::from_secs(601));
        server
            .verify_auth_with(server.inner.crypto.as_ref().unwrap(), &fresh)
            .await
            .unwrap();
        assert_eq!(server.inner.nonces.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn connection_counts_distinguish_auth_registration_and_disconnect() {
        for valid in [true, false] {
            let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let stream = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (accepted, peer) = listener.accept().await.unwrap();
            let handler = server.clone();
            let task = tokio::spawn(async move { handler.handle_connection(accepted, peer).await });
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut events = server.subscribe();
            let crypto = server.inner.crypto.as_ref().unwrap();
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts::default()
            );
            write_message(
                crypto,
                &mut writer,
                &crate::protocol::build_auth(crypto, "fixture", crate::VERSION),
            )
            .await
            .unwrap();
            let reply = timeout(Duration::from_secs(5), read_message(crypto, &mut reader))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(reply["type"], "ok");
            timeout(Duration::from_secs(5), async {
                while server.connection_counts().await.connected_node_count == 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts {
                    connected_node_count: 1,
                    registered_node_count: 0,
                    registration_pending_node_count: 1,
                }
            );
            let registration = if valid {
                json!({"type":"register","node_id":"fixture","capabilities":[]})
            } else {
                json!({"type":"register","node_id":"fixture","network":{"advertised_address":"http://invalid/path"}})
            };
            write_message(crypto, &mut writer, &registration)
                .await
                .unwrap();
            if valid {
                for refresh in [false, true] {
                    if refresh {
                        write_message(crypto, &mut writer, &registration)
                            .await
                            .unwrap();
                    }
                    assert!(matches!(
                        timeout(Duration::from_secs(5), events.recv())
                            .await
                            .unwrap()
                            .unwrap(),
                        NodeEvent::Registered(_)
                    ));
                    // Even a node advertising no capabilities is registered, not work-ready.
                    assert_eq!(
                        server.connection_counts().await,
                        ConnectionCounts {
                            connected_node_count: 1,
                            registered_node_count: 1,
                            registration_pending_node_count: 0,
                        }
                    );
                }
            }
            drop(writer);
            drop(reader);
            let result = timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.is_ok(), valid);
            assert!(matches!(
                timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
                NodeEvent::Down(_)
            ));
            assert_eq!(
                server.registered_nodes().await.len(),
                1,
                "historical routes survive disconnect"
            );
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts::default()
            );
            assert_eq!(
                serde_json::to_value(server.connection_counts().await).unwrap(),
                json!({
                    "connected_node_count":0,"registered_node_count":0,"registration_pending_node_count":0
                }),
                "no addresses or node identities in diagnostic counts"
            );
        }
    }

    #[tokio::test]
    async fn gateway_stop_clears_live_diagnostic_counts() {
        let (server, _receiver) = queued_test_server().await;
        assert_eq!(
            server
                .connection_counts()
                .await
                .registration_pending_node_count,
            1
        );
        server.stop().await;
        assert_eq!(
            server.connection_counts().await,
            ConnectionCounts::default()
        );
    }

    #[tokio::test]
    async fn gateway_stop_retires_sessions_before_restarting_same_identity() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let mut events = server.subscribe();
        for _ in 0..2 {
            server.start().await.unwrap();
            let stream = TcpStream::connect(("127.0.0.1", server.port()))
                .await
                .unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let crypto = server.inner.crypto.as_ref().unwrap();
            write_message(
                crypto,
                &mut writer,
                &crate::protocol::build_auth(crypto, "fixture", crate::VERSION),
            )
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
            write_message(
                crypto,
                &mut writer,
                &json!({"type":"register","node_id":"fixture"}),
            )
            .await
            .unwrap();
            assert!(matches!(
                timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
                NodeEvent::Registered(_)
            ));
            let caller = server.clone();
            let pending = tokio::spawn(async move {
                caller
                    .invoke_remote(
                        "fixture",
                        "system.ping",
                        "run",
                        json!({}),
                        Duration::from_secs(30),
                    )
                    .await
            });
            // Reading the request proves a waiter is present; deliberately do not reply.
            assert_eq!(
                timeout(Duration::from_secs(5), read_message(crypto, &mut reader))
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap()["type"],
                "request"
            );
            timeout(Duration::from_secs(5), server.stop())
                .await
                .unwrap();
            assert!(
                server.inner.active_ids.lock().unwrap().is_empty(),
                "Stop must await old session identity leases"
            );
            assert_eq!(
                server.connection_counts().await,
                ConnectionCounts::default()
            );
            let response = timeout(Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(response["success"], false);
            assert!(server.inner.pending.lock().await.is_empty());
            assert!(matches!(
                timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
                NodeEvent::Down(_)
            ));
            assert!(
                timeout(Duration::from_secs(5), read_message(crypto, &mut reader))
                    .await
                    .unwrap()
                    .unwrap()
                    .is_none()
            );
            drop(writer);
            drop(reader);
        }
    }

    #[tokio::test]
    async fn gateway_stop_closes_probe_sessions_waiting_for_auth() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        server.start().await.unwrap();
        let stream = TcpStream::connect(("127.0.0.1", server.port()))
            .await
            .unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let crypto = server.inner.crypto.as_ref().unwrap();
        write_message(
            crypto,
            &mut writer,
            &json!({"type":"probe","nonce":"0123456789abcdef0123456789abcdef"}),
        )
        .await
        .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(5), read_message(crypto, &mut reader))
                .await
                .unwrap()
                .unwrap()
                .unwrap()["type"],
            "pong"
        );
        timeout(Duration::from_secs(5), server.stop())
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_secs(1), read_message(crypto, &mut reader))
                .await
                .expect("Stopped gateway must close unauthenticated sessions")
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn cancelled_gateway_stop_can_finish_before_starting_new_generation() {
        let (server, _receiver) = queued_test_server().await;
        server.start().await.unwrap();
        // Hold cleanup at a known boundary after the accept task has exited.
        let pending = server.inner.pending.lock().await;
        let mut events = server.subscribe();
        let stopping = server.clone();
        let task = tokio::spawn(async move { stopping.stop().await });
        assert!(matches!(
            timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
            NodeEvent::Down(_)
        ));
        task.abort();
        let _ = task.await;
        assert!(
            server
                .inner
                .accept_task
                .lock()
                .await
                .as_ref()
                .unwrap()
                .task
                .is_none()
        );
        drop(pending);
        assert!(
            server
                .start()
                .await
                .unwrap_err()
                .to_string()
                .contains("stopping")
        );
        timeout(Duration::from_secs(5), server.stop())
            .await
            .unwrap();
        server.start().await.unwrap();
        server.stop().await;
    }

    #[tokio::test]
    async fn peer_records_use_current_connections_and_monotonic_heartbeat_age() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let (sender, _receiver) = mpsc::channel(1);
        let info = ConnectionInfo {
            node_id: "node-a".into(),
            source_ip: "private-source".into(),
            source_port: 50000,
            connected_since_ms: 0,
        };
        server.inner.connections.write().await.insert(
            "node-a".into(),
            Connection {
                session_id: Uuid::nil(),
                credential_transport: false,
                info: info.clone(),
                sender,
                advertisement: None,
                state: None,
                last_seen: Instant::now() - Duration::from_secs(90),
            },
        );
        let ids = std::collections::BTreeSet::from(["node-a".into()]);
        assert!(server.peer_records(&ids).await.unwrap().is_empty());
        let mut wire = Vec::new();
        write_message(server.inner.crypto.as_ref().unwrap(),&mut wire,&json!({"type":"register","node_id":"node-a","network":{"advertised_address":"26.1.2.3"}})).await.unwrap();
        write_message(server.inner.crypto.as_ref().unwrap(),&mut wire,&json!({"type":"heartbeat","node_id":"node-a","network":{"advertised_address":"attacker"}})).await.unwrap();
        server
            .serve(
                "node-a",
                server.inner.crypto.as_ref().unwrap(),
                &mut BufReader::new(wire.as_slice()),
            )
            .await
            .unwrap();
        let records = server.peer_records(&ids).await.unwrap();
        assert_eq!(
            records[0].0.network.advertised_address.as_deref(),
            Some("26.1.2.3")
        );
        assert!(records[0].1 < Duration::from_secs(10));
        server.inner.connections.write().await.remove("node-a");
        server
            .inner
            .routes
            .write()
            .await
            .insert("node-a".into(), info);
        assert!(server.peer_records(&ids).await.unwrap().is_empty());
        let mut wire = Vec::new();
        write_message(
            server.inner.crypto.as_ref().unwrap(),
            &mut wire,
            &json!({"type":"heartbeat","node_id":"node-a"}),
        )
        .await
        .unwrap();
        assert!(
            server
                .serve(
                    "node-a",
                    server.inner.crypto.as_ref().unwrap(),
                    &mut BufReader::new(wire.as_slice())
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn registration_keeps_advertisements_distinct_and_rejects_invalid_metadata() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let mut events = server.subscribe();
        for (network, accepted) in [
            (
                json!({"advertised_address":"26.1.2.3","coordinator":{"coordinator_id":"eef-owner","address":"26.1.2.3:51335","state":"standby"}}),
                true,
            ),
            (json!({"advertised_address":"http://arbitrary/path"}), false),
            (
                json!({"coordinator":{"coordinator_id":"eef-owner","address":"26.1.2.3"}}),
                false,
            ),
        ] {
            let message =
                json!({"type":"register","node_id":"stable","name":"Laptop","network":network});
            let mut wire = Vec::new();
            write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &message)
                .await
                .unwrap();
            let result = server
                .serve(
                    "stable",
                    server.inner.crypto.as_ref().unwrap(),
                    &mut BufReader::new(wire.as_slice()),
                )
                .await;
            assert_eq!(result.is_ok(), accepted);
            if accepted {
                let NodeEvent::Registered(actual) = events.recv().await.unwrap() else {
                    panic!()
                };
                assert_eq!(actual["network"], network);
                assert_eq!(actual["node_id"], "stable");
            } else {
                assert!(events.try_recv().is_err());
            }
        }
    }

    #[tokio::test]
    async fn gateway_stamps_origin_and_snapshots_area_without_leaking_bindings() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let mut events = server.subscribe();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let messages = [
            json!({"type":"register","node_id":"owner","capabilities":["camera.capture"],"metadata":{"area":["Home","Office"],"resources":[{"id":"camera","capability":"camera.capture","parameters":{"password":"private"}}]}}),
            json!({"type":"submit","node_id":"owner","id":"one","gateway_session_id":"forged","interpretation_capabilities":["shell.admin"],"request_context":{"origin_node":"forged","origin_area":["Other"]}}),
            json!({"type":"register","node_id":"owner"}),
            json!({"type":"submit","node_id":"owner","id":"two","operation_id":operation_id,"interpretation_capabilities":["shell.admin"],"request_context":{"origin_node":"forged"}}),
        ];
        let mut wire = Vec::new();
        for message in messages {
            write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &message)
                .await
                .unwrap();
        }
        server
            .serve(
                "owner",
                server.inner.crypto.as_ref().unwrap(),
                &mut BufReader::new(wire.as_slice()),
            )
            .await
            .unwrap();
        let NodeEvent::Registered(registration) = events.recv().await.unwrap() else {
            panic!()
        };
        assert!(!registration.to_string().contains("private"));
        let NodeEvent::Submission(first) = events.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(
            first["gateway_session_id"],
            Uuid::nil().to_string(),
            "the gateway replaces caller-authored routing generations"
        );
        assert_eq!(
            first["request_context"],
            json!({"request_id":"one","origin_node":"owner","origin_area":["Home","Office"]})
        );
        assert_eq!(
            first["interpretation_capabilities"],
            json!(["camera.capture"])
        );
        let NodeEvent::Registered(legacy) = events.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(legacy["metadata"]["area"], json!([]));
        assert!(legacy["network"]["advertised_address"].is_null());
        let NodeEvent::Submission(second) = events.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(second["request_context"]["origin_area"], json!([]));
        assert_eq!(second["interpretation_capabilities"], json!([]));
        assert_eq!(second["request_context"]["request_id"], operation_id);
        assert_eq!(second["request_context"]["origin_node"], "owner");
        assert_eq!(
            second["id"], "two",
            "reply routing retains the transport ID"
        );
        assert_eq!(
            first["request_context"]["origin_area"],
            json!(["Home", "Office"])
        );
        for invalid in [
            json!({"type":"submit","node_id":"owner","id":"early"}),
            json!({"type":"register","node_id":"forged"}),
        ] {
            let mut wire = Vec::new();
            write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &invalid)
                .await
                .unwrap();
            assert!(
                server
                    .serve(
                        "owner",
                        server.inner.crypto.as_ref().unwrap(),
                        &mut BufReader::new(wire.as_slice())
                    )
                    .await
                    .is_err()
            );
        }
        for invalid in [json!("bad"), json!(42), json!({"origin_node":"forged"})] {
            let mut wire = Vec::new();
            for message in [
                json!({"type":"register","node_id":"owner"}),
                json!({"type":"submit","node_id":"owner","id":"three","operation_id":invalid}),
            ] {
                write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &message)
                    .await
                    .unwrap();
            }
            assert!(
                server
                    .serve(
                        "owner",
                        server.inner.crypto.as_ref().unwrap(),
                        &mut BufReader::new(wire.as_slice())
                    )
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn a_different_node_cannot_complete_another_nodes_request() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let (sender, mut receiver) = oneshot::channel();
        server.inner.pending.lock().await.insert(
            "request".into(),
            Pending {
                node_id: "owner".into(),
                observer: None,
                grant: None,
                acknowledged: false,
                lease_deadline: None,
                lease_probe_id: None,
                session_id: Uuid::nil(),
                sender,
            },
        );
        let response = json!({"type":"response","id":"request","success":true});
        server
            .resolve_response("other", Uuid::nil(), response.clone())
            .await;
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(server.inner.pending.lock().await.contains_key("request"));
        server
            .resolve_response("owner", Uuid::nil(), response.clone())
            .await;
        assert_eq!(receiver.await.unwrap(), response);
    }
    #[tokio::test]
    async fn grants_require_exact_durable_ack_before_correlated_completion() {
        use crate::grants::{GrantReport, ReceiptState};
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let mut events = server.subscribe();
        let g = crate::grants::tests::grant();
        let session = Uuid::parse_str(&g.session_id).unwrap();
        let (sender, mut receiver) = oneshot::channel();
        server.inner.pending.lock().await.insert(
            "wire-id".into(),
            Pending {
                node_id: "owner".into(),
                observer: None,
                session_id: session,
                grant: Some(g.clone()),
                acknowledged: false,
                lease_deadline: Some(Instant::now() + Duration::from_secs(10)),
                lease_probe_id: Some(Uuid::new_v4().to_string()),
                sender,
            },
        );
        let complete = json!({"type":"response","id":"wire-id","success":true,"grant":GrantReport::new(&g,ReceiptState::Completed,json!({"ok":true}),None)});
        server
            .resolve_response("owner", session, complete.clone())
            .await;
        assert!(receiver.try_recv().is_err());
        let ack = json!({"type":"grant_ack","id":"wire-id","grant":GrantReport::new(&g,ReceiptState::Accepted,Value::Null,None)});
        server
            .resolve_grant_ack("other", session, ack.clone())
            .await;
        server
            .resolve_grant_ack("owner", Uuid::new_v4(), ack.clone())
            .await;
        let mut forged = ack.clone();
        forged["grant"]["persisted"] = json!(false);
        server.resolve_grant_ack("owner", session, forged).await;
        assert!(!server.inner.pending.lock().await["wire-id"].acknowledged);
        assert!(events.try_recv().is_err());
        server
            .resolve_grant_ack("owner", session, ack.clone())
            .await;
        assert!(matches!(
            events.try_recv().unwrap(),
            NodeEvent::GrantAccepted(_)
        ));
        server.resolve_grant_ack("owner", session, ack).await;
        assert!(events.try_recv().is_err());
        server
            .resolve_response(
                "owner",
                session,
                json!({"type":"response","id":"wire-id","success":true}),
            )
            .await;
        let rejected = json!({"type":"response","id":"wire-id","success":false,"grant":GrantReport::new(&g,ReceiptState::Rejected,Value::Null,Some("rejected"))});
        server.resolve_response("owner", session, rejected).await;
        assert!(receiver.try_recv().is_err());
        let mut bad = complete.clone();
        bad["grant"]["plan_sha256"] = json!("c".repeat(64));
        server.resolve_response("owner", session, bad).await;
        let mut bad = complete.clone();
        bad["grant"]["persisted"] = json!(false);
        server.resolve_response("owner", session, bad).await;
        assert!(receiver.try_recv().is_err());
        server
            .resolve_response("owner", session, complete.clone())
            .await;
        assert_eq!(receiver.await.unwrap(), complete);
    }
    #[tokio::test]
    async fn legacy_routes_cannot_send_a_grant_or_downgrade_it_into_rpc() {
        let (server, mut queue) = queued_test_server().await;
        for capability in [
            "llm.infer",
            "vlm.analyze",
            "filesystem",
            "http.request",
            "system.info",
            "fixture.work",
        ] {
            assert!(
                server
                    .invoke_remote(
                        "owner",
                        capability,
                        "run",
                        json!({}),
                        Duration::from_millis(10)
                    )
                    .await
                    .unwrap_err()
                    .is::<crate::execution_boundary::ExecutionGrantRequired>()
            );
        }
        assert!(server.grant_session("owner").await.is_err());
        assert!(
            server
                .invoke_grant(crate::grants::tests::grant(), Duration::from_millis(10))
                .await
                .is_err()
        );
        assert!(queue.try_recv().is_err());
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn node_rejects_forged_ordinary_work_even_when_feature_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("ordinary-must-not-write.txt");
        let policy: crate::NodePolicy = serde_json::from_value(json!({
            "filesystem":{"write":true},"full_access":["filesystem.write"]
        }))
        .unwrap();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap(),
        );
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        server.start().await.unwrap();
        let client = crate::NodeClient::new(serde_json::from_value(json!({
            "endpoints":[format!("127.0.0.1:{}",server.port())],"node_id":"boundary-node","psk":"test-secret"
        })).unwrap(), engine).unwrap();
        let running = tokio::spawn(async move { client.connect_once().await });
        timeout(Duration::from_secs(5), async {
            loop {
                if server
                    .registered_nodes()
                    .await
                    .contains_key("boundary-node")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // Deliberately bypass the sending API's guard to simulate an older or
        // malicious authenticated coordinator. The receiving node must fence it.
        for (capability, action, params) in [
            (
                "filesystem",
                "write",
                json!({"path":out,"content":"forged"}),
            ),
            ("llm.infer", "run", json!({"prompt":"must not run"})),
        ] {
            let response = server
                .invoke_message(
                    "boundary-node",
                    json!({"type":"request",
                "capability":capability,"action":action,"params":params}),
                    Duration::from_secs(3),
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();
            assert_eq!(response["success"], false);
            assert_eq!(response["not_started"], true);
            assert_eq!(response["error_code"], "execution_grant_required");
        }
        assert!(!out.exists());
        assert_eq!(
            server
                .invoke_remote(
                    "boundary-node",
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(3)
                )
                .await
                .unwrap()["success"],
            true
        );
        server.stop().await;
        running.abort();
        let _ = running.await;
    }
    #[tokio::test]
    async fn durable_observer_failures_never_confirm_ack_or_completion_via_broadcast() {
        use crate::grants::{GrantObserver, GrantReport, ReceiptState};
        struct Observer {
            fail: ReceiptState,
            saved: std::sync::Mutex<Vec<ReceiptState>>,
        }
        impl GrantObserver for Observer {
            fn record(&self, r: &GrantReport) -> Result<()> {
                if r.state == self.fail {
                    bail!("test storage is full")
                }
                self.saved.lock().unwrap().push(r.state);
                Ok(())
            }
        }
        for fail in [ReceiptState::Accepted, ReceiptState::Completed] {
            let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
            let mut events = server.subscribe();
            let g = crate::grants::tests::grant();
            let session = Uuid::parse_str(&g.session_id).unwrap();
            let observer = Arc::new(Observer {
                fail,
                saved: std::sync::Mutex::new(vec![]),
            });
            let (sender, mut receiver) = oneshot::channel();
            server.inner.pending.lock().await.insert(
                "wire".into(),
                Pending {
                    observer: Some(observer.clone()),
                    grant: Some(g.clone()),
                    acknowledged: false,
                    lease_deadline: Some(Instant::now() + Duration::from_secs(10)),
                    lease_probe_id: Some(Uuid::new_v4().to_string()),
                    node_id: "owner".into(),
                    session_id: session,
                    sender,
                },
            );
            let ack = json!({"type":"grant_ack","id":"wire","grant":GrantReport::new(&g,ReceiptState::Accepted,Value::Null,None)});
            server.resolve_grant_ack("owner", session, ack).await;
            if fail == ReceiptState::Completed {
                assert!(events.try_recv().is_ok());
                assert!(receiver.try_recv().is_err());
                server
                    .resolve_response(
                        "owner",
                        session,
                        json!({"type":"response","id":"wire","success":true,
                    "grant":GrantReport::new(&g,ReceiptState::Completed,json!({"ok":true}),None)}),
                    )
                    .await;
                assert_eq!(
                    *observer.saved.lock().unwrap(),
                    vec![ReceiptState::Accepted]
                );
            } else {
                assert!(events.try_recv().is_err());
                assert!(observer.saved.lock().unwrap().is_empty());
            }
            let value = receiver.await.unwrap();
            assert_eq!(value["success"], false);
            assert_eq!(value["error_type"], "GrantJournalError");
            assert!(!server.inner.pending.lock().await.contains_key("wire"));
        }
    }
    #[tokio::test]
    async fn observer_checkpoints_reports_before_the_waiter_is_confirmed_without_event_listeners() {
        use crate::grants::{GrantObserver, GrantReport, ReceiptState};
        struct Observer(std::sync::Mutex<Vec<ReceiptState>>);
        impl GrantObserver for Observer {
            fn record(&self, r: &GrantReport) -> Result<()> {
                self.0.lock().unwrap().push(r.state);
                Ok(())
            }
        }
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let g = crate::grants::tests::grant();
        let session = Uuid::parse_str(&g.session_id).unwrap();
        let observer = Arc::new(Observer(std::sync::Mutex::new(vec![])));
        let (sender, receiver) = oneshot::channel();
        server.inner.pending.lock().await.insert(
            "wire".into(),
            Pending {
                observer: Some(observer.clone()),
                grant: Some(g.clone()),
                acknowledged: false,
                lease_deadline: Some(Instant::now() + Duration::from_secs(10)),
                lease_probe_id: Some(Uuid::new_v4().to_string()),
                node_id: "owner".into(),
                session_id: session,
                sender,
            },
        );
        let ack = json!({"type":"grant_ack","id":"wire","grant":GrantReport::new(&g,ReceiptState::Accepted,Value::Null,None)});
        server
            .resolve_grant_ack("owner", session, ack.clone())
            .await;
        server.resolve_grant_ack("owner", session, ack).await;
        assert_eq!(*observer.0.lock().unwrap(), vec![ReceiptState::Accepted]);
        let reply = json!({"type":"response","id":"wire","success":true,"grant":GrantReport::new(&g,ReceiptState::Completed,json!({"ok":true}),None)});
        server
            .resolve_response("owner", session, reply.clone())
            .await;
        assert_eq!(receiver.await.unwrap(), reply);
        assert_eq!(
            *observer.0.lock().unwrap(),
            vec![ReceiptState::Accepted, ReceiptState::Completed]
        );
    }

    async fn queued_test_server() -> (NodeServer, mpsc::Receiver<Value>) {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let (sender, receiver) = mpsc::channel(1);
        server.inner.connections.write().await.insert(
            "owner".into(),
            Connection {
                session_id: Uuid::nil(),
                credential_transport: false,
                info: ConnectionInfo {
                    node_id: "owner".into(),
                    source_ip: "127.0.0.1".into(),
                    source_port: 1,
                    connected_since_ms: 0,
                },
                sender,
                advertisement: None,
                state: None,
                last_seen: Instant::now(),
            },
        );
        (server, receiver)
    }

    #[tokio::test]
    async fn fresh_lease_queries_require_live_exact_issuance_and_never_renew_or_accept_unproved_ack()
     {
        use crate::grant_time::{LeaseQuery, LeaseReply};
        let (server, mut queue) = queued_test_server().await;
        let g = crate::grants::tests::grant();
        let session = Uuid::parse_str(&g.session_id).unwrap();
        {
            let mut connections = server.inner.connections.write().await;
            let c = connections.get_mut("owner").unwrap();
            c.session_id = session;
            c.credential_transport = true;
            c.state = Some(NodeStateSnapshot {
                registration: json!({"execution_grants":{"schema_version":1,"available":true,"timing_mode":crate::grant_time::MODE}}),
                heartbeat: None,
            });
        }
        let wire = Uuid::new_v4().simple().to_string();
        let (sender, _receiver) = oneshot::channel();
        let deadline = Instant::now() + Duration::from_secs(1);
        server.inner.pending.lock().await.insert(
            wire.clone(),
            Pending {
                observer: None,
                grant: Some(g.clone()),
                acknowledged: false,
                lease_deadline: Some(deadline),
                lease_probe_id: None,
                node_id: "owner".into(),
                session_id: session,
                sender,
            },
        );
        let q = LeaseQuery {
            kind: "grant_lease_probe".into(),
            schema_version: 1,
            id: Uuid::new_v4().to_string(),
            request_id: wire.clone(),
            node_id: "owner".into(),
            session_id: g.session_id.clone(),
            grant_id: g.grant_id.clone(),
            grant_sha256: crate::grants::definition_digest(&json!(g)).unwrap(),
        };
        let ack = json!({"type":"grant_ack","id":wire,"grant":crate::grants::GrantReport::new(&g,crate::grants::ReceiptState::Accepted,Value::Null,None)});
        server
            .resolve_grant_ack("owner", session, ack.clone())
            .await;
        assert!(!server.inner.pending.lock().await[&wire].acknowledged);
        for key in [
            "node_id",
            "session_id",
            "grant_id",
            "grant_sha256",
            "request_id",
        ] {
            let mut bad = json!(q);
            bad[key] = json!("other");
            assert!(
                server
                    .reply_grant_lease("owner", session, bad)
                    .await
                    .is_err()
            );
            assert!(queue.try_recv().is_err());
            assert!(
                server.inner.pending.lock().await[&wire]
                    .lease_probe_id
                    .is_none()
            );
        }
        let mut extra = json!(q);
        extra["extend_ms"] = json!(30000);
        assert!(
            server
                .reply_grant_lease("owner", session, extra)
                .await
                .is_err()
        );
        server
            .reply_grant_lease("owner", session, json!(q))
            .await
            .unwrap();
        let reply: LeaseReply = serde_json::from_value(queue.recv().await.unwrap()).unwrap();
        assert!(reply.remaining_ms > 0 && reply.remaining_ms <= 1000);
        assert_eq!(reply.grant_sha256, q.grant_sha256);
        assert_eq!(
            server.inner.pending.lock().await[&wire].lease_deadline,
            Some(deadline)
        );
        assert!(
            server
                .reply_grant_lease("owner", session, json!(q))
                .await
                .is_err()
        );
        let mut renewal = q.clone();
        renewal.id = Uuid::new_v4().to_string();
        assert!(
            server
                .reply_grant_lease("owner", session, json!(renewal))
                .await
                .is_err()
        );
        assert!(queue.try_recv().is_err());
        server
            .resolve_grant_ack("owner", session, ack.clone())
            .await;
        assert!(server.inner.pending.lock().await[&wire].acknowledged);
        {
            let mut pending = server.inner.pending.lock().await;
            let p = pending.get_mut(&wire).unwrap();
            p.lease_deadline = Some(Instant::now() - Duration::from_secs(1));
            p.lease_probe_id = None;
            p.acknowledged = false;
        }
        server
            .reply_grant_lease("owner", session, json!(renewal))
            .await
            .unwrap();
        assert_eq!(queue.recv().await.unwrap()["remaining_ms"], 0);
        server.resolve_grant_ack("owner", session, ack).await;
        assert!(!server.inner.pending.lock().await[&wire].acknowledged);
        server
            .inner
            .connections
            .write()
            .await
            .get_mut("owner")
            .unwrap()
            .session_id = Uuid::new_v4();
        assert!(
            server
                .reply_grant_lease("owner", session, json!(q))
                .await
                .is_err()
        );
        let mut connections = server.inner.connections.write().await;
        let c = connections.get_mut("owner").unwrap();
        c.state.as_mut().unwrap().registration["execution_grants"]
            .as_object_mut()
            .unwrap()
            .remove("timing_mode");
        assert!(server.check_lease_connection(c).is_err());
    }

    #[tokio::test]
    async fn reconciliation_snapshots_are_live_sanitized_latest_and_independent() {
        let (server, _receiver) = queued_test_server().await;
        assert!(
            server.node_states().await.is_empty(),
            "unregistered sessions are excluded"
        );
        assert!(server.node_state("owner").await.is_none());
        let mut wire = Vec::new();
        for value in [
            json!({"type":"register","node_id":"owner","capabilities":["system.info"],"name":"First","psk":"PRIVATE-PSK","gateway_received_ms":1,
                "metadata":{"resources":[{"id":"info","capability":"system.info","parameters":{"password":"PRIVATE-BINDING"}}]}}),
            json!({"type":"heartbeat","node_id":"owner","load":{"cpu":0.2},"latency_ms":8,"command":"PRIVATE-COMMAND","gateway_received_ms":u64::MAX}),
        ] {
            write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &value)
                .await
                .unwrap();
        }
        server
            .serve(
                "owner",
                server.inner.crypto.as_ref().unwrap(),
                &mut BufReader::new(wire.as_slice()),
            )
            .await
            .unwrap();
        let old = server.node_state("owner").await.unwrap();
        assert_eq!(old.registration["name"], "First");
        assert_eq!(old.heartbeat.as_ref().unwrap()["load"]["cpu"], 0.2);
        let registration_time = old.registration["gateway_received_ms"].as_u64().unwrap();
        let heartbeat_time = old.heartbeat.as_ref().unwrap()["gateway_received_ms"]
            .as_u64()
            .unwrap();
        assert!(registration_time > 1);
        assert!(heartbeat_time >= registration_time && heartbeat_time <= now_ms());
        assert!(!format!("{old:?}").contains("PRIVATE"));
        let mut wire = Vec::new();
        for value in [
            json!({"type":"register","node_id":"owner","capabilities":[],"name":"Second"}),
            json!({"type":"heartbeat","node_id":"owner","load":{"cpu":0.8},"latency_ms":3}),
        ] {
            write_message(server.inner.crypto.as_ref().unwrap(), &mut wire, &value)
                .await
                .unwrap();
        }
        server
            .serve(
                "owner",
                server.inner.crypto.as_ref().unwrap(),
                &mut BufReader::new(wire.as_slice()),
            )
            .await
            .unwrap();
        let latest = server.node_states().await.remove("owner").unwrap();
        assert_eq!(latest.registration["name"], "Second");
        assert_eq!(latest.registration["capabilities"], json!([]));
        assert_eq!(old.registration["gateway_received_ms"], registration_time);
        assert_eq!(
            old.heartbeat.as_ref().unwrap()["gateway_received_ms"],
            heartbeat_time
        );
        assert_eq!(latest.heartbeat.unwrap()["load"]["cpu"], 0.8);
        assert_eq!(
            old.registration["name"], "First",
            "previous snapshots never mutate"
        );
        assert_eq!(old.heartbeat.unwrap()["load"]["cpu"], 0.2);
        server.inner.connections.write().await.remove("owner");
        assert!(server.node_state("owner").await.is_none());
        assert!(
            server.node_states().await.is_empty(),
            "historical routes are excluded"
        );
    }

    #[tokio::test]
    async fn cancelled_remote_wait_removes_pending_correlation() {
        let (server, mut receiver) = queued_test_server().await;
        let caller = server.clone();
        let task = tokio::spawn(async move {
            caller
                .invoke_remote(
                    "owner",
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(30),
                )
                .await
        });
        receiver.recv().await.unwrap();
        assert_eq!(server.inner.pending.lock().await.len(), 1);
        task.abort();
        let _ = task.await;
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn same_identity_different_session_cannot_confirm_or_fail_an_old_waiter() {
        let (server, mut receiver) = queued_test_server().await;
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        server
            .inner
            .connections
            .write()
            .await
            .get_mut("owner")
            .unwrap()
            .session_id = old;
        let caller = server.clone();
        let task = tokio::spawn(async move {
            caller
                .invoke_remote(
                    "owner",
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(3),
                )
                .await
        });
        let request = receiver.recv().await.unwrap();
        let id = request["id"].as_str().unwrap();
        let response = json!({"type":"response","id":id,"success":true});
        server
            .resolve_response("owner", new, response.clone())
            .await;
        server.fail_pending_for("owner", new).await;
        assert!(!task.is_finished());
        assert_eq!(server.inner.pending.lock().await[id].session_id, old);
        server
            .resolve_response("owner", old, response.clone())
            .await;
        assert_eq!(task.await.unwrap().unwrap(), response);
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn real_reconnect_cannot_receive_a_previous_submissions_reply_even_with_reused_wire_id() {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        server.start().await.unwrap();
        let mut events = server.subscribe();
        let mut previous: Option<Value> = None;
        for n in 0..2 {
            let stream = TcpStream::connect(("127.0.0.1", server.port()))
                .await
                .unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            write_message(
                server.inner.crypto.as_ref().unwrap(),
                &mut writer,
                &crate::protocol::build_auth(
                    server.inner.crypto.as_ref().unwrap(),
                    "owner",
                    crate::VERSION,
                ),
            )
            .await
            .unwrap();
            let reply = timeout(
                Duration::from_secs(3),
                read_message(server.inner.crypto.as_ref().unwrap(), &mut reader),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(reply["type"], "ok");
            write_message(
                server.inner.crypto.as_ref().unwrap(),
                &mut writer,
                &json!({"type":"register","node_id":"owner"}),
            )
            .await
            .unwrap();
            write_message(
                server.inner.crypto.as_ref().unwrap(),
                &mut writer,
                &json!({"type":"submit","node_id":"owner","id":"reused-wire-id","kind":"jobs.list",
                "payload":{},"gateway_session_id":"forged"}),
            )
            .await
            .unwrap();
            let current = timeout(Duration::from_secs(3), async {
                loop {
                    if let NodeEvent::Submission(value) = events.recv().await.unwrap() {
                        break value;
                    }
                }
            })
            .await
            .unwrap();
            let session = current["gateway_session_id"].as_str().unwrap();
            assert!(!Uuid::parse_str(session).unwrap().is_nil());
            if let Some(old) = &previous {
                assert_ne!(old["gateway_session_id"], current["gateway_session_id"]);
                assert!(server.send_submission_reply(old,json!({"type":"submission_result","id":"reused-wire-id","success":true,"data":"STALE"})).await.is_err());
            }
            server.send_submission_reply(&current,json!({"type":"submission_result","id":"reused-wire-id","success":true,"data":n})).await.unwrap();
            let reply = timeout(
                Duration::from_secs(3),
                read_message(server.inner.crypto.as_ref().unwrap(), &mut reader),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(
                reply["data"], n,
                "no stale reply is routed to the replacement connection"
            );
            previous = Some(current);
            drop(reader);
            drop(writer);
            timeout(Duration::from_secs(3), async {
                loop {
                    if matches!(events.recv().await.unwrap(), NodeEvent::Down(_)) {
                        break;
                    }
                }
            })
            .await
            .unwrap();
            timeout(Duration::from_secs(3), async {
                while server.inner.active_ids.lock().unwrap().contains("owner") {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        server.stop().await;
    }
    #[tokio::test]
    async fn old_cleanup_does_not_remove_replacement_waiters_or_deliver_old_submission_reply() {
        let (server, mut queue) = queued_test_server().await;
        let old = Uuid::new_v4();
        let current = Uuid::new_v4();
        server
            .inner
            .connections
            .write()
            .await
            .get_mut("owner")
            .unwrap()
            .session_id = current;
        let caller = server.clone();
        let task = tokio::spawn(async move {
            caller
                .invoke_remote(
                    "owner",
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(3),
                )
                .await
        });
        let request = queue.recv().await.unwrap();
        server.fail_pending_for("owner", old).await;
        assert!(!task.is_finished());
        let source = json!({"type":"submit","id":"old-reply","node_id":"owner","gateway_session_id":old.to_string()});
        let response = json!({"type":"submission_result","id":"old-reply"});
        assert!(
            server
                .send_submission_reply(&source, response.clone())
                .await
                .is_err()
        );
        assert!(queue.try_recv().is_err());
        for source in [
            json!({"type":"submit","id":"old-reply","node_id":"owner"}),
            json!({"type":"submit","id":"old-reply","node_id":"owner","gateway_session_id":"forged"}),
            json!({"type":"submit","id":"old-reply","node_id":"owner","gateway_session_id":Uuid::nil().to_string()}),
            json!({"type":"submit","id":"different","node_id":"owner","gateway_session_id":current.to_string()}),
        ] {
            assert!(
                server
                    .send_submission_reply(&source, response.clone())
                    .await
                    .is_err()
            );
            assert!(queue.try_recv().is_err());
        }
        server
            .send_submission_reply(
                &json!({"type":"submit","id":"old-reply","node_id":"owner","gateway_session_id":current.to_string()}),
                response.clone(),
            )
            .await
            .unwrap();
        assert_eq!(queue.recv().await.unwrap(), response);
        server.fail_pending_for("owner", current).await;
        let failed = task.await.unwrap().unwrap();
        assert_eq!(failed["id"], request["id"]);
        assert_eq!(failed["success"], false);
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn waiter_registration_cannot_run_after_its_session_cleanup_has_drained() {
        let (server, _queue) = queued_test_server().await;
        let generation = Uuid::new_v4();
        server
            .inner
            .connections
            .write()
            .await
            .get_mut("owner")
            .unwrap()
            .session_id = generation;
        let pending = server.inner.pending.lock().await;
        let caller = server.clone();
        let task = tokio::spawn(async move {
            caller
                .invoke_remote(
                    "owner",
                    "system.ping",
                    "run",
                    json!({}),
                    Duration::from_secs(3),
                )
                .await
        });
        timeout(Duration::from_secs(3), async {
            loop {
                if server.inner.connections.try_write().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let retired = server.clone();
        let cleanup = tokio::spawn(async move {
            retired.inner.connections.write().await.remove("owner");
            retired.fail_pending_for("owner", generation).await;
        });
        tokio::task::yield_now().await;
        assert!(!cleanup.is_finished());
        drop(pending);
        timeout(Duration::from_secs(3), cleanup)
            .await
            .unwrap()
            .unwrap();
        let result = timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err() || result.unwrap()["success"] == false);
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn outbound_queue_wait_is_bounded_and_does_not_leave_pending_requests() {
        let (server, _receiver) = queued_test_server().await;
        server.inner.connections.read().await["owner"]
            .sender
            .send(json!({"queued":true}))
            .await
            .unwrap();
        let result = timeout(
            Duration::from_secs(1),
            server.invoke_remote(
                "owner",
                "system.ping",
                "run",
                json!({}),
                Duration::from_millis(20),
            ),
        )
        .await;
        assert!(
            result
                .expect("outbound queue must honor command deadline")
                .is_err()
        );
        assert!(server.inner.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn submission_replies_fail_promptly_when_queue_is_full_without_late_delivery() {
        let (server, mut receiver) = queued_test_server().await;
        server
            .send_to_node("owner", json!({"id":"first"}))
            .await
            .unwrap();
        let result = timeout(
            Duration::from_millis(200),
            server.send_to_node("owner", json!({"id":"rejected"})),
        )
        .await
        .expect("A full response queue must not stall coordinator event processing");
        assert!(result.unwrap_err().to_string().contains("queue is full"));
        assert_eq!(receiver.recv().await.unwrap()["id"], "first");
        assert!(
            receiver.try_recv().is_err(),
            "rejected replies must not appear later"
        );
        server
            .send_to_node("owner", json!({"id":"next"}))
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap()["id"], "next");
        drop(receiver);
        assert!(
            server
                .send_to_node("owner", json!({}))
                .await
                .unwrap_err()
                .downcast_ref::<NodeOfflineError>()
                .is_some()
        );
        assert!(
            server
                .send_to_node("absent", json!({}))
                .await
                .unwrap_err()
                .downcast_ref::<NodeOfflineError>()
                .is_some()
        );
        assert!(server.inner.pending.lock().await.is_empty());
    }
}
