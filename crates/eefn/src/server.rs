use std::collections::HashMap;
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
}

#[derive(Clone)]
struct Connection {
    info: ConnectionInfo,
    sender: mpsc::Sender<Value>,
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

    pub async fn invoke_remote(
        &self,
        node_id: &str,
        capability: &str,
        action: &str,
        params: Value,
        wait: Duration,
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
            "action": action, "params": params,
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
        let auth = timeout(AUTH_TIMEOUT, read_message(&self.inner.crypto, &mut reader))
            .await
            .context("auth timed out")??
            .context("connection closed before auth")?;
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
        if self.inner.connections.read().await.contains_key(&node_id) {
            let denied = json!({"type": "denied", "error": "duplicate node id already connected"});
            write_message(&self.inner.crypto, &mut writer, &denied).await?;
            bail!("duplicate node id '{node_id}'");
        }
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
        self.inner
            .connections
            .write()
            .await
            .insert(node_id.clone(), Connection { info, sender: send });

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
        while let Some(mut message) = read_message(&self.inner.crypto, reader).await? {
            match message.get("type").and_then(Value::as_str) {
                Some("register") => {
                    require_node_id(&message, authenticated_id)?;
                    message["node_id"] = Value::String(authenticated_id.into());
                    let _ = self.inner.events.send(NodeEvent::Registered(message));
                }
                Some("heartbeat") => {
                    require_node_id(&message, authenticated_id)?;
                    message["node_id"] = Value::String(authenticated_id.into());
                    let _ = self.inner.events.send(NodeEvent::Heartbeat(message));
                }
                Some("submit") => {
                    require_node_id(&message, authenticated_id)?;
                    if message.get("id").and_then(Value::as_str).is_none() {
                        bail!("submission requires an id")
                    }
                    message["node_id"] = Value::String(authenticated_id.into());
                    let _ = self.inner.events.send(NodeEvent::Submission(message));
                }
                Some("response") => self.resolve_response(message).await,
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

    async fn resolve_response(&self, message: Value) {
        let Some(id) = message.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return;
        };
        if let Some(pending) = self.inner.pending.lock().await.remove(&id) {
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
