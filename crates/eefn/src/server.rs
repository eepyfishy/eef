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
}

#[derive(Clone)]
pub struct NodeServer {
    inner: Arc<Inner>,
}

struct Inner {
    host: String,
    port: AtomicU16,
    crypto: NodeCrypto,
    connections: RwLock<HashMap<String, Connection>>,
    routes: RwLock<HashMap<String, ConnectionInfo>>,
    pending: Mutex<HashMap<String, Pending>>,
    nonces: Mutex<HashMap<String, Instant>>,
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
    info: ConnectionInfo,
    sender: mpsc::Sender<Value>,
    advertisement: Option<crate::network::PeerRecord>,
    state: Option<NodeStateSnapshot>,
    last_seen: Instant,
}

struct Pending {
    node_id: String,
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
        let (events, _) = broadcast::channel(256);
        Ok(Self {
            inner: Arc::new(Inner {
                host: host.into(),
                port: AtomicU16::new(port),
                crypto: NodeCrypto::new(psk)?,
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
        let connection = self
            .inner
            .connections
            .read()
            .await
            .get(node_id)
            .cloned()
            .ok_or_else(|| NodeOfflineError(node_id.into()))?;
        let request_id = Uuid::new_v4().simple().to_string();
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().await.insert(
            request_id.clone(),
            Pending {
                node_id: node_id.into(),
                sender,
            },
        );
        let _cleanup = PendingCleanup {
            inner: self.inner.clone(),
            request_id: request_id.clone(),
        };
        let message = json!({
            "type": "request", "id": request_id, "capability": capability,
            "action": action, "params": params, "request_context":context,
        });
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

    async fn handle_connection(&self, stream: TcpStream, peer: SocketAddr) -> Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut auth = timeout(AUTH_TIMEOUT, read_message(&self.inner.crypto, &mut reader))
            .await
            .context("auth timed out")??
            .context("connection closed before auth")?;
        // A bounded, encrypted service probe does not register or replace a node.
        if auth["type"] == "probe" {
            let nonce = auth["nonce"]
                .as_str()
                .filter(|s| s.len() == 32)
                .context("invalid probe challenge")?;
            timeout(
                Duration::from_secs(5),
                write_message(
                    &self.inner.crypto,
                    &mut writer,
                    &json!({"type":"pong","nonce":nonce,"protocol":PROTOCOL_VERSION}),
                ),
            )
            .await
            .context("probe reply timed out")??;
            auth = timeout(AUTH_TIMEOUT, read_message(&self.inner.crypto, &mut reader))
                .await
                .context("auth after probe timed out")??
                .context("connection closed after probe")?;
        }
        let node_id = match self.verify_auth(&auth).await {
            Ok(node_id) => node_id,
            Err(error) => {
                let _ = write_message(
                    &self.inner.crypto,
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
            write_message(&self.inner.crypto, &mut writer, &denied).await?;
            bail!("duplicate node id '{node_id}'");
        }
        let _identity = IdentityLease {
            inner: self.inner.clone(),
            node_id: node_id.clone(),
        };
        write_message(
            &self.inner.crypto,
            &mut writer,
            &json!({"type": "ok", "protocol": PROTOCOL_VERSION}),
        )
        .await?;

        let (send, mut receive) = mpsc::channel::<Value>(64);
        let crypto = self.inner.crypto.clone();
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
            result = self.serve(&node_id, &mut reader) => result,
            result = async {
                while let Some(message) = receive.recv().await {
                    write_message(&crypto, &mut writer, &message).await?;
                }
                Ok(())
            } => result,
        };
        self.inner.connections.write().await.remove(&node_id);
        self.fail_pending_for(&node_id).await;
        let _ = self.inner.events.send(NodeEvent::Down(node_id));
        result
    }

    async fn serve<R>(&self, authenticated_id: &str, reader: &mut R) -> Result<()>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        let mut metadata = crate::context::NodeMetadata::default();
        let mut interpretation_capabilities = Vec::<String>::new();
        let mut registered = false;
        while let Some(mut message) = read_message(&self.inner.crypto, reader).await? {
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
                    let advertisement = crate::network::PeerRecord::from_registration(&message)?;
                    interpretation_capabilities = capabilities;
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
                                ],
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
                    let _ = self.inner.events.send(NodeEvent::Submission(message));
                }
                Some("response") => self.resolve_response(authenticated_id, message).await,
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

    async fn verify_auth(&self, auth: &Value) -> Result<String> {
        if auth.get("type").and_then(Value::as_str) != Some("auth") {
            bail!("auth failed")
        }
        if auth.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
            bail!("unsupported protocol")
        }
        let text = |key| auth.get(key).and_then(Value::as_str).context("auth failed");
        let node_id = text("node_id")?.trim();
        let version = text("version")?;
        let nonce = text("nonce")?;
        let timestamp = text("timestamp")?;
        let signature = text("signature")?;
        if node_id.is_empty() || node_id.len() > 128 {
            bail!("invalid node id")
        }
        let timestamp_ms: i64 = timestamp.parse().context("invalid auth timestamp")?;
        let local_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if (local_ms - timestamp_ms).abs() > AUTH_CLOCK_SKEW_MS {
            bail!("stale auth timestamp")
        }
        let raw = format!("{nonce}|{node_id}|{version}|{timestamp}");
        if !self.inner.crypto.verify(raw.as_bytes(), signature) {
            bail!("auth failed")
        }
        let mut nonces = self.inner.nonces.lock().await;
        nonces.retain(|_, seen| seen.elapsed() < Duration::from_secs(10 * 60));
        if nonces
            .insert(format!("{node_id}:{nonce}"), Instant::now())
            .is_some()
        {
            bail!("replayed auth")
        }
        Ok(node_id.into())
    }

    async fn resolve_response(&self, authenticated_id: &str, message: Value) {
        let Some(id) = message.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return;
        };
        let mut requests = self.inner.pending.lock().await;
        if !requests
            .get(&id)
            .is_some_and(|pending| pending.node_id == authenticated_id)
        {
            return;
        }
        if let Some(pending) = requests.remove(&id) {
            let _ = pending.sender.send(message);
        }
    }

    async fn fail_pending_for(&self, node_id: &str) {
        let mut pending = self.inner.pending.lock().await;
        let ids = pending
            .iter()
            .filter(|(_, request)| request.node_id == node_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(request) = pending.remove(&id) {
                let _ = request.sender.send(json!({"type": "response", "id": id, "success": false, "error": "node disconnected"}));
            }
        }
    }
}

fn state_fields(message: &Value, fields: &[&str]) -> Value {
    let mut snapshot = serde_json::Map::new();
    for field in fields {
        if let Some(value) = message.get(*field) {
            snapshot.insert((*field).into(), value.clone());
        }
    }
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
            let crypto = &server.inner.crypto;
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
            let crypto = &server.inner.crypto;
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
        let crypto = &server.inner.crypto;
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
        write_message(&server.inner.crypto,&mut wire,&json!({"type":"register","node_id":"node-a","network":{"advertised_address":"26.1.2.3"}})).await.unwrap();
        write_message(&server.inner.crypto,&mut wire,&json!({"type":"heartbeat","node_id":"node-a","network":{"advertised_address":"attacker"}})).await.unwrap();
        server
            .serve("node-a", &mut BufReader::new(wire.as_slice()))
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
            &server.inner.crypto,
            &mut wire,
            &json!({"type":"heartbeat","node_id":"node-a"}),
        )
        .await
        .unwrap();
        assert!(
            server
                .serve("node-a", &mut BufReader::new(wire.as_slice()))
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
            write_message(&server.inner.crypto, &mut wire, &message)
                .await
                .unwrap();
            let result = server
                .serve("stable", &mut BufReader::new(wire.as_slice()))
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
            json!({"type":"submit","node_id":"owner","id":"one","interpretation_capabilities":["shell.admin"],"request_context":{"origin_node":"forged","origin_area":["Other"]}}),
            json!({"type":"register","node_id":"owner"}),
            json!({"type":"submit","node_id":"owner","id":"two","operation_id":operation_id,"interpretation_capabilities":["shell.admin"],"request_context":{"origin_node":"forged"}}),
        ];
        let mut wire = Vec::new();
        for message in messages {
            write_message(&server.inner.crypto, &mut wire, &message)
                .await
                .unwrap();
        }
        server
            .serve("owner", &mut BufReader::new(wire.as_slice()))
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
            write_message(&server.inner.crypto, &mut wire, &invalid)
                .await
                .unwrap();
            assert!(
                server
                    .serve("owner", &mut BufReader::new(wire.as_slice()))
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
                write_message(&server.inner.crypto, &mut wire, &message)
                    .await
                    .unwrap();
            }
            assert!(
                server
                    .serve("owner", &mut BufReader::new(wire.as_slice()))
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
                sender,
            },
        );
        let response = json!({"type":"response","id":"request","success":true});
        server.resolve_response("other", response.clone()).await;
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(server.inner.pending.lock().await.contains_key("request"));
        server.resolve_response("owner", response.clone()).await;
        assert_eq!(receiver.await.unwrap(), response);
    }

    async fn queued_test_server() -> (NodeServer, mpsc::Receiver<Value>) {
        let server = NodeServer::new("test-secret", "127.0.0.1", 0).unwrap();
        let (sender, receiver) = mpsc::channel(1);
        server.inner.connections.write().await.insert(
            "owner".into(),
            Connection {
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
    async fn reconciliation_snapshots_are_live_sanitized_latest_and_independent() {
        let (server, _receiver) = queued_test_server().await;
        assert!(
            server.node_states().await.is_empty(),
            "unregistered sessions are excluded"
        );
        assert!(server.node_state("owner").await.is_none());
        let mut wire = Vec::new();
        for value in [
            json!({"type":"register","node_id":"owner","capabilities":["system.info"],"name":"First","psk":"PRIVATE-PSK",
                "metadata":{"resources":[{"id":"info","capability":"system.info","parameters":{"password":"PRIVATE-BINDING"}}]}}),
            json!({"type":"heartbeat","node_id":"owner","load":{"cpu":0.2},"latency_ms":8,"command":"PRIVATE-COMMAND"}),
        ] {
            write_message(&server.inner.crypto, &mut wire, &value)
                .await
                .unwrap();
        }
        server
            .serve("owner", &mut BufReader::new(wire.as_slice()))
            .await
            .unwrap();
        let old = server.node_state("owner").await.unwrap();
        assert_eq!(old.registration["name"], "First");
        assert_eq!(old.heartbeat.as_ref().unwrap()["load"]["cpu"], 0.2);
        assert!(!format!("{old:?}").contains("PRIVATE"));
        let mut wire = Vec::new();
        for value in [
            json!({"type":"register","node_id":"owner","capabilities":[],"name":"Second"}),
            json!({"type":"heartbeat","node_id":"owner","load":{"cpu":0.8},"latency_ms":3}),
        ] {
            write_message(&server.inner.crypto, &mut wire, &value)
                .await
                .unwrap();
        }
        server
            .serve("owner", &mut BufReader::new(wire.as_slice()))
            .await
            .unwrap();
        let latest = server.node_states().await.remove("owner").unwrap();
        assert_eq!(latest.registration["name"], "Second");
        assert_eq!(latest.registration["capabilities"], json!([]));
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
