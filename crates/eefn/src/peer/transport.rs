//! Mutually pinned TLS plus existing encrypted newline framing. No socket bind/dial.
use super::{
    credentials::{Material, der, name},
    scope::{Role, Scope},
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf},
    time::Sleep,
};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        self, ClientConfig, RootCertStore, ServerConfig,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    },
};
const ALPN: &[u8] = b"eef-peer-tls/1";

pub struct PeerEndpoint {
    scope: Scope,
    material: Material,
    started: Instant,
    wall: u64,
}
impl std::fmt::Debug for PeerEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerEndpoint")
            .field("session_id", &self.scope.session_id)
            .field("role", &self.material.role)
            .finish_non_exhaustive()
    }
}
impl PeerEndpoint {
    pub(super) fn new(
        scope: Scope,
        material: Material,
        started: Instant,
        wall: u64,
    ) -> Result<Self> {
        // Validate key/cert material before making an endpoint usable.
        let endpoint = Self {
            scope,
            material,
            started,
            wall,
        };
        match endpoint.material.role {
            Role::Source => {
                endpoint.client_config()?;
            }
            Role::Destination => {
                endpoint.server_config()?;
            }
        }
        Ok(endpoint)
    }
    fn roots(&self) -> Result<RootCertStore> {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(der(&self.material.root_certificate)?))?;
        Ok(roots)
    }
    fn client_config(&self) -> Result<Arc<ClientConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut cfg = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(self.roots()?)
            .with_client_auth_cert(
                vec![CertificateDer::from(der(&self.material.certificate)?)],
                PrivatePkcs8KeyDer::from(der(&self.material.private_key)?).into(),
            )?;
        cfg.alpn_protocols = vec![ALPN.to_vec()];
        cfg.enable_early_data = false;
        cfg.resumption = rustls::client::Resumption::disabled();
        Ok(Arc::new(cfg))
    }
    fn server_config(&self) -> Result<Arc<ServerConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(self.roots()?),
            provider.clone(),
        )
        .build()?;
        let mut cfg = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![CertificateDer::from(der(&self.material.certificate)?)],
                PrivatePkcs8KeyDer::from(der(&self.material.private_key)?).into(),
            )?;
        cfg.alpn_protocols = vec![ALPN.to_vec()];
        cfg.max_early_data_size = 0;
        cfg.send_tls13_tickets = 0;
        cfg.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        Ok(Arc::new(cfg))
    }
    fn deadline(&self) -> Instant {
        self.started + Duration::from_millis(self.scope.expires_at_ms - self.wall)
    }
    fn check(&self) -> Result<()> {
        check_clock(&self.scope, self.started, self.wall).map_err(Into::into)
    }
    pub async fn connect<S: crate::tls::NodeIo + 'static>(self, stream: S) -> Result<PeerChannel> {
        if self.material.role != Role::Source {
            bail!("Only the scoped source may initiate this peer path")
        }
        self.check()?;
        let connector = TlsConnector::from(self.client_config()?);
        let secured = tokio::time::timeout_at(
            self.deadline().into(),
            connector.connect(
                ServerName::try_from(name(&self.material.scope_sha256, Role::Destination))?,
                stream,
            ),
        )
        .await
        .context("Peer TLS deadline expired")?
        .map_err(|_| anyhow::anyhow!("Peer TLS source verification failed"))?;
        self.check()?;
        verify(
            secured.get_ref().1.protocol_version(),
            secured.get_ref().1.alpn_protocol(),
            secured.get_ref().1.peer_certificates(),
            &self.material.peer_certificate,
        )?;
        self.channel(Box::new(secured))
    }
    pub async fn accept<S: crate::tls::NodeIo + 'static>(self, stream: S) -> Result<PeerChannel> {
        if self.material.role != Role::Destination {
            bail!("Only the scoped destination may accept this peer path")
        }
        self.check()?;
        let acceptor = TlsAcceptor::from(self.server_config()?);
        let secured = tokio::time::timeout_at(self.deadline().into(), acceptor.accept(stream))
            .await
            .context("Peer TLS deadline expired")?
            .map_err(|_| anyhow::anyhow!("Peer TLS destination verification failed"))?;
        self.check()?;
        verify(
            secured.get_ref().1.protocol_version(),
            secured.get_ref().1.alpn_protocol(),
            secured.get_ref().1.peer_certificates(),
            &self.material.peer_certificate,
        )?;
        self.channel(Box::new(secured))
    }
    fn channel(self, stream: crate::tls::NodeStream) -> Result<PeerChannel> {
        let crypto = crate::NodeCrypto::new(&self.material.wire_key)?;
        let role = self.material.role;
        let hash = self.material.scope_sha256;
        let deadline = self.started + Duration::from_millis(self.scope.expires_at_ms - self.wall);
        let io = PeerIo {
            stream,
            scope: self.scope.clone(),
            started: self.started,
            wall: self.wall,
            deadline: Box::pin(tokio::time::sleep_until(deadline.into())),
            used: 0,
            tokens: f64::from(self.scope.limits.max_bytes_per_second),
            refilled: Instant::now(),
            rate_wait: Box::pin(tokio::time::sleep(Duration::ZERO)),
            scratch: vec![0u8; 16384],
        };
        Ok(PeerChannel {
            io: BufReader::new(io),
            crypto,
            scope: self.scope,
            role,
            hash,
            sent: 0,
            received: 0,
            sent_bytes: 0,
            received_bytes: 0,
            ack_sent: 0,
            ack_received: 0,
            last_ack_sent: 0,
            unacked: VecDeque::new(),
            failed: false,
        })
    }
}
fn verify(
    version: Option<rustls::ProtocolVersion>,
    alpn: Option<&[u8]>,
    certs: Option<&[CertificateDer<'_>]>,
    expected: &str,
) -> Result<()> {
    if version != Some(rustls::ProtocolVersion::TLSv1_3)
        || alpn != Some(ALPN)
        || certs.and_then(|c| c.first()).map(|c| c.as_ref()) != Some(der(expected)?.as_slice())
    {
        bail!("Peer TLS role, exact certificate or protocol differs")
    }
    Ok(())
}
fn check_clock(scope: &Scope, started: Instant, wall: u64) -> io::Result<()> {
    let now = crate::protocol::now_ms();
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if now < scope.issued_at_ms
        || now >= scope.expires_at_ms
        || now.abs_diff(wall.saturating_add(elapsed)) > 1000
        || elapsed >= scope.expires_at_ms - wall
    {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Peer lease expired or clock changed",
        ));
    }
    Ok(())
}
struct PeerIo {
    stream: crate::tls::NodeStream,
    scope: Scope,
    started: Instant,
    wall: u64,
    deadline: Pin<Box<Sleep>>,
    used: u64,
    tokens: f64,
    refilled: Instant,
    rate_wait: Pin<Box<Sleep>>,
    scratch: Vec<u8>,
}
impl PeerIo {
    fn budget(&mut self, cx: &mut TaskContext<'_>, wanted: usize) -> Poll<io::Result<usize>> {
        if let Err(error) = check_clock(&self.scope, self.started, self.wall) {
            return Poll::Ready(Err(error));
        }
        if self.deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Peer lease expired",
            )));
        }
        if self.used >= self.scope.limits.max_wire_bytes {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Peer wire-byte budget exhausted",
            )));
        }
        let now = Instant::now();
        let rate = f64::from(self.scope.limits.max_bytes_per_second);
        self.tokens =
            (self.tokens + now.duration_since(self.refilled).as_secs_f64() * rate).min(rate);
        self.refilled = now;
        if self.tokens < 1.0 {
            self.rate_wait
                .as_mut()
                .reset((now + Duration::from_secs_f64((1.0 - self.tokens) / rate)).into());
            if self.rate_wait.as_mut().poll(cx).is_ready() {
                cx.waker().wake_by_ref();
            }
            return Poll::Pending;
        }
        Poll::Ready(Ok(wanted
            .min(16384)
            .min((self.scope.limits.max_wire_bytes - self.used) as usize)
            .min(self.tokens as usize)))
    }
    fn charged(&mut self, n: usize) {
        self.used += n as u64;
        self.tokens -= n as f64;
    }
}
impl AsyncRead for PeerIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let n = match this.budget(cx, buf.remaining()) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(n)) => n,
        };
        let mut read = ReadBuf::new(&mut this.scratch[..n]);
        match Pin::new(&mut this.stream).poll_read(cx, &mut read) {
            Poll::Ready(Ok(())) => {
                let n = read.filled().len();
                buf.put_slice(read.filled());
                this.charged(n);
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}
impl AsyncWrite for PeerIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let n = match this.budget(cx, bytes.len()) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok(n)) => n,
        };
        match Pin::new(&mut this.stream).poll_write(cx, &bytes[..n]) {
            Poll::Ready(Ok(n)) => {
                this.charged(n);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = check_clock(&this.scope, this.started, this.wall) {
            return Poll::Ready(Err(error));
        }
        Pin::new(&mut this.stream).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    pub received_frames: u32,
    pub received_payload_bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Body {
    Data {
        bytes: String,
    },
    Acknowledgement {
        received_frames: u32,
        received_payload_bytes: u64,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    schema_version: u32,
    scope_sha256: String,
    role: Role,
    sequence: u32,
    body: Body,
}
/// Bounded transport only. An acknowledgement is endpoint evidence, not a grant,
/// job completion, sink-device playback or proof of stopped external effects.
pub struct PeerChannel {
    io: BufReader<PeerIo>,
    crypto: crate::NodeCrypto,
    scope: Scope,
    role: Role,
    hash: String,
    sent: u32,
    received: u32,
    sent_bytes: u64,
    received_bytes: u64,
    ack_sent: u32,
    ack_received: u32,
    last_ack_sent: u32,
    unacked: VecDeque<(u32, u64)>,
    failed: bool,
}
impl PeerChannel {
    fn check_lease(&self) -> io::Result<()> {
        check_clock(
            &self.scope,
            self.io.get_ref().started,
            self.io.get_ref().wall,
        )
    }
    fn check(&mut self) -> Result<()> {
        if self.failed {
            bail!("Peer channel is fenced after failure")
        };
        let checked = self.check_lease();
        if checked.is_err() {
            self.failed = true;
        }
        checked?;
        Ok(())
    }
    async fn write(&mut self, role: Role, sequence: u32, body: Body) -> Result<()> {
        self.check()?;
        let frame = Frame {
            schema_version: 1,
            scope_sha256: self.hash.clone(),
            role,
            sequence,
            body,
        };
        let line = crate::protocol::encode_line(&self.crypto, &serde_json::to_value(frame)?)?;
        if line.len() as u64 > self.scope.limits.max_wire_bytes - self.io.get_ref().used {
            self.failed = true;
            bail!("Peer frame exceeds remaining wire budget")
        }
        // A dropped future can leave a partial frame on the stream. Fence before
        // the first suspension, and reopen only after a complete successful IO.
        // Cancellation is not authority to retry the same sequence or grant.
        self.failed = true;
        let result = async {
            self.io.write_all(&line).await?;
            self.io.flush().await?;
            self.check_lease()?;
            Ok(())
        }
        .await;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    async fn read(&mut self, role: Role, sequence: u32) -> Result<Body> {
        self.check()?;
        let limit = 2 * self.scope.limits.max_frame_bytes as usize + 4096;
        // read_wire_line's partial local buffer is not retained on cancellation.
        // Do not interpret a suffix as a new frame on a later call.
        self.failed = true;
        let result = async {
            let line = crate::protocol::read_wire_line(&mut self.io, limit)
                .await?
                .context("Peer closed without a complete frame")?;
            if line.last() != Some(&b'\n') {
                bail!("Peer closed before the frame delimiter")
            }
            let value = crate::protocol::decode_line(&self.crypto, &line)
                .map_err(|_| anyhow::anyhow!("Peer frame verification failed"))?;
            let frame: Frame =
                serde_json::from_value(value).map_err(|_| anyhow::anyhow!("Invalid peer frame"))?;
            if frame.schema_version != 1
                || frame.scope_sha256 != self.hash
                || frame.role != role
                || frame.sequence != sequence
            {
                bail!("Peer frame direction, scope or sequence differs")
            }
            self.check_lease()?;
            Ok(frame.body)
        }
        .await;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    pub async fn send_data(&mut self, bytes: &[u8]) -> Result<()> {
        if self.role != Role::Source
            || bytes.is_empty()
            || bytes.len() > self.scope.limits.max_frame_bytes as usize
            || self.sent >= self.scope.limits.max_frames
        {
            bail!("Peer data direction, frame size or count denied")
        }
        if self.unacked.len() >= usize::from(self.scope.limits.max_inflight_frames) {
            bail!("Peer send window full; await a valid acknowledgement")
        }
        self.write(
            Role::Source,
            self.sent + 1,
            Body::Data {
                bytes: STANDARD.encode(bytes),
            },
        )
        .await?;
        self.sent += 1;
        self.sent_bytes += bytes.len() as u64;
        self.unacked.push_back((self.sent, self.sent_bytes));
        Ok(())
    }
    pub async fn receive_data(&mut self) -> Result<Vec<u8>> {
        if self.role != Role::Destination
            || self.received >= self.scope.limits.max_frames
            || self.received - self.last_ack_sent
                >= u32::from(self.scope.limits.max_inflight_frames)
        {
            bail!("Peer receive direction, frame count or window denied")
        }
        let Body::Data { bytes } = self.read(Role::Source, self.received + 1).await? else {
            self.failed = true;
            bail!("Expected peer data, not control")
        };
        let value = STANDARD
            .decode(bytes)
            .ok()
            .filter(|v| !v.is_empty() && v.len() <= self.scope.limits.max_frame_bytes as usize);
        let Some(value) = value else {
            self.failed = true;
            bail!("Invalid peer payload size or encoding")
        };
        self.received += 1;
        self.received_bytes += value.len() as u64;
        Ok(value)
    }
    pub async fn acknowledge(&mut self) -> Result<()> {
        if self.role != Role::Destination || self.received <= self.last_ack_sent {
            bail!("No new peer data to acknowledge")
        }
        self.write(
            Role::Destination,
            self.ack_sent + 1,
            Body::Acknowledgement {
                received_frames: self.received,
                received_payload_bytes: self.received_bytes,
            },
        )
        .await?;
        self.ack_sent += 1;
        self.last_ack_sent = self.received;
        Ok(())
    }
    pub async fn receive_acknowledgement(&mut self) -> Result<Acknowledgement> {
        if self.role != Role::Source || self.unacked.is_empty() {
            bail!("No outstanding peer data")
        }
        let Body::Acknowledgement {
            received_frames,
            received_payload_bytes,
        } = self.read(Role::Destination, self.ack_received + 1).await?
        else {
            self.failed = true;
            bail!("Expected peer acknowledgement, not data")
        };
        let Some(at) = self
            .unacked
            .iter()
            .position(|entry| *entry == (received_frames, received_payload_bytes))
        else {
            self.failed = true;
            bail!("Peer acknowledgement does not match an exact sent prefix")
        };
        self.unacked.drain(..=at);
        self.ack_received += 1;
        Ok(Acknowledgement {
            received_frames,
            received_payload_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::tests::Fixture;

    #[test]
    fn tls_uses_mutual_authentication_no_resumption_and_a_distinct_protocol() {
        let f = Fixture::new();
        let source = f.endpoint(Role::Source);
        let destination = f.endpoint(Role::Destination);
        let client = source.client_config().unwrap();
        let server = destination.server_config().unwrap();
        assert_eq!(client.alpn_protocols, vec![ALPN.to_vec()]);
        assert!(!client.enable_early_data);
        assert_eq!(server.max_early_data_size, 0);
        assert_eq!(server.send_tls13_tickets, 0);
        assert!(server.session_storage.get(b"not-retained").is_none());
        assert!(
            verify(
                Some(rustls::ProtocolVersion::TLSv1_3),
                Some(ALPN),
                Some(&[CertificateDer::from(
                    der(&source.material.certificate).unwrap()
                )]),
                &source.material.peer_certificate
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn a_client_without_its_scoped_certificate_is_not_accepted() {
        let f = Fixture::new();
        let source = f.endpoint(Role::Source);
        let destination = f.endpoint(Role::Destination);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_root_certificates(source.roots().unwrap())
            .with_no_client_auth();
        config.alpn_protocols = vec![ALPN.to_vec()];
        let (a, b) = tokio::io::duplex(1024);
        let connector = TlsConnector::from(Arc::new(config));
        let client = async {
            connector
                .connect(
                    ServerName::try_from(name(&source.material.scope_sha256, Role::Destination))
                        .unwrap(),
                    a,
                )
                .await
        };
        let (_, server) = tokio::join!(client, destination.accept(b));
        assert!(server.is_err());
    }
    #[tokio::test]
    async fn plaintext_and_control_protocol_alpn_are_not_peer_paths() {
        let f = Fixture::new();
        let (mut a, b) = tokio::io::duplex(1024);
        let raw = async {
            a.write_all(b"not a TLS peer frame\n").await.unwrap();
            a.shutdown().await.unwrap();
        };
        let (_, accepted) = tokio::join!(raw, f.endpoint(Role::Destination).accept(b));
        assert!(accepted.is_err());
        let source = f.endpoint(Role::Source);
        let destination = f.endpoint(Role::Destination);
        let mut cfg = source.client_config().unwrap().as_ref().clone();
        cfg.alpn_protocols = vec![b"eef-node-tls/1".to_vec()];
        let (a, b) = tokio::io::duplex(1024);
        let connector = TlsConnector::from(Arc::new(cfg));
        let client = async {
            connector
                .connect(
                    ServerName::try_from(name(&source.material.scope_sha256, Role::Destination))
                        .unwrap(),
                    a,
                )
                .await
        };
        let (_, accepted) = tokio::join!(client, destination.accept(b));
        assert!(accepted.is_err());
    }

    struct Capture<T> {
        inner: T,
        writes: Arc<std::sync::Mutex<Vec<u8>>>,
    }
    impl<T: AsyncRead + Unpin> AsyncRead for Capture<T> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
        }
    }
    impl<T: AsyncWrite + Unpin> AsyncWrite for Capture<T> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            match Pin::new(&mut this.inner).poll_write(cx, bytes) {
                Poll::Ready(Ok(n)) => {
                    let mut captured = this.writes.lock().unwrap();
                    assert!(captured.len() + n <= 1024 * 1024);
                    captured.extend_from_slice(&bytes[..n]);
                    Poll::Ready(Ok(n))
                }
                other => other,
            }
        }
        fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }
        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
    }
    #[tokio::test]
    async fn observed_tls_wire_does_not_contain_plaintext_or_base64_payload() {
        let f = Fixture::new();
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (a, b) = tokio::io::duplex(1024);
        let a = Capture {
            inner: a,
            writes: captured.clone(),
        };
        let (source, destination) = tokio::join!(
            f.endpoint(Role::Source).connect(a),
            f.endpoint(Role::Destination).accept(b)
        );
        let mut source = source.unwrap();
        let mut destination = destination.unwrap();
        let payload = b"synthetic private content must remain encrypted on this wire";
        source.send_data(payload).await.unwrap();
        assert_eq!(destination.receive_data().await.unwrap(), payload);
        destination.acknowledge().await.unwrap();
        source.receive_acknowledgement().await.unwrap();
        let captured = captured.lock().unwrap();
        assert!(!captured.is_empty());
        assert!(!captured.windows(payload.len()).any(|w| w == payload));
        let base64 = STANDARD.encode(payload);
        assert!(
            !captured
                .windows(base64.len())
                .any(|w| w == base64.as_bytes())
        );
    }
    #[tokio::test]
    async fn reordered_wrong_direction_and_foreign_scope_frames_fence_the_channel() {
        for fault in ["sequence", "direction", "scope", "unknown_field"] {
            let f = Fixture::new();
            let (mut source, mut destination) = f.channels().await;
            let mut frame = serde_json::json!({"schema_version":1,"scope_sha256":source.hash,"role":"source","sequence":1,"body":{"kind":"data","bytes":STANDARD.encode(b"private")}});
            match fault {
                "sequence" => frame["sequence"] = serde_json::json!(2),
                "direction" => frame["role"] = serde_json::json!("destination"),
                "scope" => frame["scope_sha256"] = serde_json::json!("b".repeat(64)),
                "unknown_field" => frame["authority"] = serde_json::json!(true),
                _ => unreachable!(),
            }
            crate::protocol::write_message(&source.crypto, &mut source.io, &frame)
                .await
                .unwrap();
            assert!(destination.receive_data().await.is_err(), "{fault}");
            assert!(destination.receive_data().await.is_err(), "fenced {fault}");
        }
    }
    #[tokio::test]
    async fn repeated_data_and_false_acknowledgements_cannot_claim_new_delivery() {
        let f = Fixture::new();
        let (mut source, mut destination) = f.channels().await;
        let frame = serde_json::json!({"schema_version":1,"scope_sha256":source.hash,"role":"source","sequence":1,"body":{"kind":"data","bytes":STANDARD.encode(b"one")}});
        let line = crate::protocol::encode_line(&source.crypto, &frame).unwrap();
        source.io.write_all(&line).await.unwrap();
        source.io.flush().await.unwrap();
        assert_eq!(destination.receive_data().await.unwrap(), b"one");
        source.io.write_all(&line).await.unwrap();
        source.io.flush().await.unwrap();
        assert!(destination.receive_data().await.is_err());
        let (mut source, mut destination) = f.channels().await;
        source.send_data(b"one").await.unwrap();
        destination.receive_data().await.unwrap();
        destination
            .write(
                Role::Destination,
                1,
                Body::Acknowledgement {
                    received_frames: 1,
                    received_payload_bytes: 4,
                },
            )
            .await
            .unwrap();
        assert!(source.receive_acknowledgement().await.is_err());
        assert!(source.send_data(b"no continuation").await.is_err());
    }
    #[tokio::test]
    async fn a_clock_discontinuity_permanently_fences_the_existing_channel() {
        let f = Fixture::new();
        let (mut source, _destination) = f.channels().await;
        let wall = source.io.get_ref().wall;
        source.io.get_mut().wall = wall - 5000;
        assert!(source.send_data(b"never transmitted").await.is_err());
        source.io.get_mut().wall = wall;
        assert!(
            source
                .send_data(b"not revived by a repaired clock")
                .await
                .is_err()
        );
        assert_eq!(source.io.get_ref().used, 0);
    }

    #[tokio::test]
    async fn cancelled_partial_write_cannot_reuse_the_channel_or_claim_delivery() {
        let mut f = Fixture::new();
        f.scope.limits.max_bytes_per_second = 100;
        f.reseal();
        let (mut source, _destination) = f.channels().await;
        let mut sending = Box::pin(source.send_data(&[7; 64]));
        std::future::poll_fn(|cx| {
            assert!(sending.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(sending);
        let written = source.io.get_ref().used;
        assert!(
            written > 0,
            "test must cancel after accepting partial bytes"
        );
        assert!(source.failed);
        assert_eq!(source.sent, 0);
        assert!(source.unacked.is_empty());
        assert!(source.send_data(b"no retry").await.is_err());
        assert_eq!(source.io.get_ref().used, written);
    }

    #[tokio::test]
    async fn cancelled_partial_read_cannot_reinterpret_the_remaining_suffix() {
        let f = Fixture::new();
        let (mut source, mut destination) = f.channels().await;
        let frame = serde_json::json!({"schema_version":1,"scope_sha256":source.hash,"role":"source","sequence":1,"body":{"kind":"data","bytes":STANDARD.encode(b"private")}});
        let line = crate::protocol::encode_line(&source.crypto, &frame).unwrap();
        let split = line.len() / 2;
        source.io.write_all(&line[..split]).await.unwrap();
        source.io.flush().await.unwrap();
        let mut receiving = Box::pin(destination.receive_data());
        std::future::poll_fn(|cx| {
            assert!(receiving.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(receiving);
        let read = destination.io.get_ref().used;
        assert!(read > 0, "test must cancel after consuming partial bytes");
        source.io.write_all(&line[split..]).await.unwrap();
        source.io.flush().await.unwrap();
        assert!(destination.failed);
        assert!(destination.receive_data().await.is_err());
        assert!(destination.acknowledge().await.is_err());
        assert_eq!(destination.received, 0);
        assert_eq!(destination.io.get_ref().used, read);
    }

    #[tokio::test]
    async fn eof_without_frame_delimiter_is_not_received_data() {
        let f = Fixture::new();
        let (mut source, mut destination) = f.channels().await;
        let frame = serde_json::json!({"schema_version":1,"scope_sha256":source.hash,"role":"source","sequence":1,"body":{"kind":"data","bytes":STANDARD.encode(b"private")}});
        let mut line = crate::protocol::encode_line(&source.crypto, &frame).unwrap();
        assert_eq!(line.pop(), Some(b'\n'));
        source.io.write_all(&line).await.unwrap();
        source.io.flush().await.unwrap();
        source.io.shutdown().await.unwrap();
        assert!(destination.receive_data().await.is_err());
        assert!(destination.failed);
        assert_eq!(destination.received, 0);
        assert!(destination.acknowledge().await.is_err());
    }
}
