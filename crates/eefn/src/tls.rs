//! Standard TLS 1.3 for independent node credentials. No insecure verifier,
//! public CA pool, early data, resumption, or automatic certificate replacement.
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore, ServerConfig};

// Protocol identity, not a routable host or a deployment-specific machine name.
const SERVER_NAME: &str = "eef-coordinator.invalid";
const ALPN: &[u8] = b"eef-node-tls/1";
const MAX_DER: usize = 4096;

pub trait NodeIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> NodeIo for T {}
pub type NodeStream = Box<dyn NodeIo>;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorIdentity {
    pub certificate: String,
    pub private_key: String,
}

fn decode_der(value: &str) -> Result<Vec<u8>> {
    if value.len() > 8192 {
        bail!("TLS material exceeds bounds")
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| anyhow::anyhow!("invalid TLS material encoding"))?;
    if bytes.is_empty() || bytes.len() > MAX_DER {
        bail!("TLS material exceeds bounds")
    }
    Ok(bytes)
}

impl CoordinatorIdentity {
    pub fn generate() -> Result<Self> {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_owned()])
                .map_err(|_| anyhow::anyhow!("coordinator certificate generation failed"))?;
        let identity = Self {
            certificate: STANDARD.encode(cert.der()),
            private_key: STANDARD.encode(signing_key.serialize_der()),
        };
        identity.server_config()?;
        Ok(identity)
    }
    pub fn fingerprint(&self) -> Result<String> {
        certificate_fingerprint(&self.certificate)
    }
    pub fn server_config(&self) -> Result<Arc<ServerConfig>> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(decode_der(&self.certificate)?)],
                PrivatePkcs8KeyDer::from(decode_der(&self.private_key)?).into(),
            )
            .map_err(|_| anyhow::anyhow!("invalid coordinator certificate or key"))?;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        Ok(Arc::new(config))
    }
}

pub fn certificate_fingerprint(certificate: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(decode_der(certificate)?)))
}

pub fn client_config(certificate: &str) -> Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(decode_der(certificate)?))
        .map_err(|_| anyhow::anyhow!("invalid paired coordinator certificate"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.enable_early_data = false;
    config.resumption = rustls::client::Resumption::disabled();
    Ok(Arc::new(config))
}

pub async fn connect<S: NodeIo + 'static>(
    stream: S,
    certificate: &str,
    config: Arc<ClientConfig>,
) -> Result<NodeStream> {
    let connector = tokio_rustls::TlsConnector::from(config);
    let secured = connector.connect(ServerName::try_from(SERVER_NAME)?, stream).await
        .map_err(|_| anyhow::anyhow!("coordinator TLS verification failed; inspect pairing and system clock, never bypass verification"))?;
    let state = &secured.get_ref().1;
    if state.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
        || state.alpn_protocol() != Some(ALPN)
        || state
            .peer_certificates()
            .and_then(|c| c.first())
            .map(|c| c.as_ref())
            != Some(decode_der(certificate)?.as_slice())
    {
        bail!("coordinator TLS identity or protocol mismatch")
    }
    Ok(Box::new(secured))
}

pub async fn accept<S: NodeIo + 'static>(
    stream: S,
    config: Arc<ServerConfig>,
) -> Result<NodeStream> {
    let acceptor = tokio_rustls::TlsAcceptor::from(config);
    let secured = acceptor
        .accept(stream)
        .await
        .map_err(|_| anyhow::anyhow!("node TLS handshake failed"))?;
    let state = &secured.get_ref().1;
    if state.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3)
        || state.alpn_protocol() != Some(ALPN)
    {
        bail!("node TLS protocol mismatch")
    }
    Ok(Box::new(secured))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    fn handshake(
        identity: &CoordinatorIdentity,
    ) -> (rustls::ClientConnection, rustls::ServerConnection) {
        let mut client = rustls::ClientConnection::new(
            client_config(&identity.certificate).unwrap(),
            ServerName::try_from(SERVER_NAME).unwrap(),
        )
        .unwrap();
        let mut server = rustls::ServerConnection::new(identity.server_config().unwrap()).unwrap();
        for _ in 0..16 {
            let mut bytes = Vec::new();
            client.write_tls(&mut bytes).unwrap();
            server.read_tls(&mut Cursor::new(bytes)).unwrap();
            server.process_new_packets().unwrap();
            let mut bytes = Vec::new();
            server.write_tls(&mut bytes).unwrap();
            client.read_tls(&mut Cursor::new(bytes)).unwrap();
            client.process_new_packets().unwrap();
            if !client.is_handshaking() && !server.is_handshaking() && !client.wants_write() {
                return (client, server);
            }
        }
        panic!("TLS handshake did not finish")
    }

    #[test]
    fn tls_records_cannot_repeat_or_cross_sessions() {
        let identity = CoordinatorIdentity::generate().unwrap();
        let (mut client, mut server) = handshake(&identity);
        assert_eq!(
            client.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        assert_eq!(client.alpn_protocol(), Some(ALPN));
        client.writer().write_all(b"private operation").unwrap();
        let mut captured = Vec::new();
        client.write_tls(&mut captured).unwrap();
        assert!(!captured.windows(17).any(|w| w == b"private operation"));
        server.read_tls(&mut Cursor::new(&captured)).unwrap();
        server.process_new_packets().unwrap();
        let mut received = vec![0; 17];
        server.reader().read_exact(&mut received).unwrap();
        assert_eq!(received, b"private operation");
        server.read_tls(&mut Cursor::new(&captured)).unwrap();
        assert!(
            server.process_new_packets().is_err(),
            "same-session record replay accepted"
        );
        let (_, mut new_session) = handshake(&identity);
        new_session.read_tls(&mut Cursor::new(captured)).unwrap();
        assert!(
            new_session.process_new_packets().is_err(),
            "cross-session record replay accepted"
        );
    }

    #[test]
    fn material_is_bounded_and_keys_must_match() {
        let first = CoordinatorIdentity::generate().unwrap();
        let second = CoordinatorIdentity::generate().unwrap();
        assert_ne!(first.fingerprint().unwrap(), second.fingerprint().unwrap());
        let mut bad = first.clone();
        bad.private_key = second.private_key;
        assert!(bad.server_config().is_err());
        for value in [
            "invalid".to_string(),
            STANDARD.encode([0; 32]),
            "A".repeat(8193),
        ] {
            assert!(client_config(&value).is_err());
        }
        let config = first.server_config().unwrap();
        assert_eq!(config.max_early_data_size, 0);
        assert_eq!(config.send_tls13_tickets, 0);
        assert!(!client_config(&first.certificate).unwrap().enable_early_data);
    }

    #[tokio::test]
    async fn exact_pinned_certificate_and_bounded_large_stream_work() {
        let identity = CoordinatorIdentity::generate().unwrap();
        for correct in [true, false] {
            let certificate = if correct {
                identity.certificate.clone()
            } else {
                CoordinatorIdentity::generate().unwrap().certificate
            };
            let (client_io, server_io) = tokio::io::duplex(1024);
            let config = identity.server_config().unwrap();
            let server = tokio::spawn(async move {
                let Ok(mut secured) = accept(server_io, config).await else {
                    return false;
                };
                let mut data = vec![0; 64 * 1024];
                if secured.read_exact(&mut data).await.is_err() {
                    return false;
                }
                assert!(data.iter().all(|b| *b == 42));
                secured.write_all(b"confirmed").await.unwrap();
                secured.flush().await.unwrap();
                true
            });
            let secured = connect(
                client_io,
                &certificate,
                client_config(&certificate).unwrap(),
            )
            .await;
            if correct {
                let mut secured = secured.unwrap();
                secured.write_all(&vec![42; 64 * 1024]).await.unwrap();
                secured.flush().await.unwrap();
                let mut reply = [0; 9];
                secured.read_exact(&mut reply).await.unwrap();
                assert_eq!(&reply, b"confirmed");
            } else {
                assert!(secured.is_err());
            }
            assert_eq!(server.await.unwrap(), correct);
        }
    }

    #[tokio::test]
    async fn tls12_and_missing_alpn_are_rejected() {
        let identity = CoordinatorIdentity::generate().unwrap();
        for tls12 in [true, false] {
            let mut roots = RootCertStore::empty();
            roots
                .add(CertificateDer::from(
                    decode_der(&identity.certificate).unwrap(),
                ))
                .unwrap();
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let versions = [if tls12 {
                &rustls::version::TLS12
            } else {
                &rustls::version::TLS13
            }];
            let mut config = ClientConfig::builder_with_provider(provider)
                .with_protocol_versions(&versions)
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth();
            if tls12 {
                config.alpn_protocols = vec![ALPN.to_vec()];
            }
            let (client_io, server_io) = tokio::io::duplex(4096);
            let server_config = identity.server_config().unwrap();
            let server =
                tokio::spawn(async move { accept(server_io, server_config).await.is_err() });
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let result = connector
                .connect(ServerName::try_from(SERVER_NAME).unwrap(), client_io)
                .await;
            drop(result);
            assert!(server.await.unwrap());
        }
    }

    #[tokio::test]
    async fn buffered_selector_read_ahead_survives_tls_upgrade() {
        // A standard ClientConnection emits ClientHello. Feed it with the public
        // selector in ONE write, then continue driving that same TLS state.
        let identity = CoordinatorIdentity::generate().unwrap();
        let mut client = rustls::ClientConnection::new(
            client_config(&identity.certificate).unwrap(),
            ServerName::try_from(SERVER_NAME).unwrap(),
        )
        .unwrap();
        let mut combined =
            b"{\"type\":\"node_credential\",\"schema_version\":2,\"node_id\":\"a\"}\n".to_vec();
        client.write_tls(&mut combined).unwrap();
        let (mut client_io, server_io) = tokio::io::duplex(64 * 1024);
        client_io.write_all(&combined).await.unwrap();
        let config = identity.server_config().unwrap();
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_io);
            let selector = crate::protocol::read_wire_line(&mut reader, 8192)
                .await
                .unwrap()
                .unwrap();
            assert!(selector.starts_with(b"{"));
            assert!(
                !reader.buffer().is_empty(),
                "fixture did not exercise read-ahead"
            );
            let mut secured = accept(reader, config).await.unwrap();
            let mut payload = [0; 4];
            secured.read_exact(&mut payload).await.unwrap();
            assert_eq!(&payload, b"test");
        });
        while client.is_handshaking() {
            let mut bytes = [0; 8192];
            let n = client_io.read(&mut bytes).await.unwrap();
            assert_ne!(n, 0);
            client.read_tls(&mut Cursor::new(&bytes[..n])).unwrap();
            client.process_new_packets().unwrap();
            let mut output = Vec::new();
            client.write_tls(&mut output).unwrap();
            client_io.write_all(&output).await.unwrap();
        }
        client.writer().write_all(b"test").unwrap();
        let mut output = Vec::new();
        client.write_tls(&mut output).unwrap();
        client_io.write_all(&output).await.unwrap();
        server.await.unwrap();
    }
}
