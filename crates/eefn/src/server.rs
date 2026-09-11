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
use tokio::task::JoinHandle;
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub node_id: String,
    pub source_ip: String,
    pub source_port: u16,
    pub connected_since_ms: u64,
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
    accept_task: Mutex<Option<JoinHandle<()>>>,
    active_ids: std::sync::Mutex<HashSet<String>>,
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
    last_seen: Instant,
}

struct Pending {
    node_id: String,
    sender: oneshot::Sender<Value>,
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
        if task.is_some() {
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
        *task = Some(tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let server = server.clone();
                        tokio::spawn(async move {
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
            }
        }));
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(task) = self.inner.accept_task.lock().await.take() {
            task.abort()
        }
        self.inner.connections.write().await.clear();
        let mut pending = self.inner.pending.lock().await;
        for (id, request) in pending.drain() {
            let _ = request.sender.send(json!({"type": "response", "id": id, "success": false, "error": "node gateway stopped"}));
        }
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
        let message = json!({
            "type": "request", "id": request_id, "capability": capability,
            "action": action, "params": params, "request_context":context,
        });
        if connection.sender.send(message).await.is_err() {
            self.inner.pending.lock().await.remove(&request_id);
            return Err(NodeOfflineError(node_id.into()).into());
        }
        match timeout(wait, receiver).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => bail!("node '{node_id}' disconnected before responding"),
            Err(_) => {
                self.inner.pending.lock().await.remove(&request_id);
                bail!(
                    "node '{node_id}' did not answer within {}s",
                    wait.as_secs_f64()
                )
            }
        }
    }

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
            .send(message)
            .await
            .map_err(|_| NodeOfflineError(node_id.into()).into())
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
        let writer_task = tokio::spawn(async move {
            while let Some(message) = receive.recv().await {
                if write_message(&crypto, &mut writer, &message).await.is_err() {
                    break;
                }
            }
        });
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
                last_seen: Instant::now(),
            },
        );

        let result = self.serve(&node_id, &mut reader).await;
        self.inner.connections.write().await.remove(&node_id);
        writer_task.abort();
        self.fail_pending_for(&node_id).await;
        let _ = self.inner.events.send(NodeEvent::Down(node_id));
        result
    }

    async fn serve<R>(&self, authenticated_id: &str, reader: &mut R) -> Result<()>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        let mut metadata = crate::context::NodeMetadata::default();
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
                    if let Some(connection) = self
                        .inner
                        .connections
                        .write()
                        .await
                        .get_mut(authenticated_id)
                    {
                        connection.advertisement = Some(advertisement);
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
                    let origin = crate::context::RequestContext::new(
                        id.into(),
                        authenticated_id.into(),
                        metadata.area.clone(),
                    )?;
                    message["request_context"] = json!(origin);
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
        let messages = [
            json!({"type":"register","node_id":"owner","capabilities":["camera.capture"],"metadata":{"area":["Home","Office"],"resources":[{"id":"camera","capability":"camera.capture","parameters":{"password":"private"}}]}}),
            json!({"type":"submit","node_id":"owner","id":"one","request_context":{"origin_node":"forged","origin_area":["Other"]}}),
            json!({"type":"register","node_id":"owner"}),
            json!({"type":"submit","node_id":"owner","id":"two"}),
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
        let NodeEvent::Registered(legacy) = events.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(legacy["metadata"]["area"], json!([]));
        assert!(legacy["network"]["advertised_address"].is_null());
        let NodeEvent::Submission(second) = events.recv().await.unwrap() else {
            panic!()
        };
        assert_eq!(second["request_context"]["origin_area"], json!([]));
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
}
