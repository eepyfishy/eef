//! Bounded recovery within one explicit, checksum-pinned artifact transfer.
//! No install-command retry, restart recovery or persistent partial-file trust.
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::{StatusCode, header};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const ATTEMPTS: usize = 8;
const BUDGET: Duration = Duration::from_secs(3600);

fn decimal(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("Invalid model byte range")
    }
    value.parse().context("Invalid model byte range")
}
fn validate_headers(
    status: StatusCode,
    headers: &header::HeaderMap,
    offset: u64,
    total: u64,
) -> Result<()> {
    if headers
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| value.as_bytes() != b"identity")
    {
        bail!("Encoded model transfer cannot be byte-range verified")
    }
    if offset > 0 || status == StatusCode::PARTIAL_CONTENT {
        if status != StatusCode::PARTIAL_CONTENT {
            bail!("Model source did not honor the requested recovery range")
        }
        let raw = headers
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 128)
            .context("Model recovery range is missing")?;
        let (range, size) = raw
            .strip_prefix("bytes ")
            .and_then(|value| value.split_once('/'))
            .context("Invalid model byte range")?;
        let (start, end) = range.split_once('-').context("Invalid model byte range")?;
        if decimal(start)? != offset || decimal(end)? != total - 1 || decimal(size)? != total {
            bail!("Model recovery range does not match the pinned artifact")
        }
    } else if status != StatusCode::OK || headers.contains_key(header::CONTENT_RANGE) {
        bail!("Model source returned an unsupported artifact response")
    }
    if let Some(length) = headers.get(header::CONTENT_LENGTH) {
        if decimal(length.to_str().context("Invalid model response length")?)? != total - offset {
            bail!("Model response length does not match the pinned artifact")
        }
    }
    Ok(())
}

pub(crate) async fn receive(
    client: &reqwest::Client,
    artifact: &crate::model_manifest::GgufArtifact,
    file: &mut tokio::fs::File,
    mut space: impl FnMut(u64) -> Result<()>,
    mut progress: impl FnMut(u64, u64, usize) -> Result<()>,
) -> Result<String> {
    // Keep the shared metadata policy even when a test/custom caller supplies a client.
    let artifact = crate::model_manifest::GgufArtifact::from_catalog(&serde_json::json!({
        "url":artifact.url,"sha256":artifact.sha256,"bytes":artifact.bytes}))?;
    let transfer = async {
        let mut count = 0u64;
        let mut hash = Sha256::new();
        for attempt in 0..ATTEMPTS {
            space(artifact.bytes - count)?;
            progress(count, artifact.bytes, attempt)?;
            let mut request = client
                .get(&artifact.url)
                .header(header::ACCEPT_ENCODING, "identity");
            if count > 0 {
                request = request.header(header::RANGE, format!("bytes={count}-"));
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
            };
            validate_headers(response.status(), response.headers(), count, artifact.bytes)?;
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(_) => break,
                };
                let next = count
                    .checked_add(chunk.len() as u64)
                    .context("Model size overflow")?;
                if next > artifact.bytes {
                    bail!("Model download is larger than its catalog entry")
                }
                space(artifact.bytes - count)?;
                file.write_all(&chunk).await?;
                hash.update(&chunk);
                count = next;
                progress(count, artifact.bytes, attempt)?;
            }
            if count == artifact.bytes {
                let digest = hex::encode(hash.finalize());
                if digest != artifact.sha256.to_ascii_lowercase() {
                    bail!("Model verification failed after transfer; no artifact was installed")
                }
                return Ok(digest);
            }
            // Retry only the remaining bytes inside this already-approved GET job.
            // Never append an ignored/full 200 response to a partial artifact.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        bail!("Model transfer interrupted; bounded byte-range recovery was exhausted")
    };
    tokio::time::timeout(BUDGET, transfer)
        .await
        .context("Model artifact transfer exceeded its total time budget")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_requires_exact_unencoded_single_range() {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::CONTENT_RANGE, "bytes 3-9/10".parse().unwrap());
        headers.insert(header::CONTENT_LENGTH, "7".parse().unwrap());
        validate_headers(StatusCode::PARTIAL_CONTENT, &headers, 3, 10).unwrap();
        assert!(validate_headers(StatusCode::OK, &headers, 3, 10).is_err());
        for range in [
            "bytes 0-9/10",
            "bytes 3-8/10",
            "bytes 3-9/*",
            "bytes +3-9/10",
            "bytes 3-9/11",
            "bytes 3-9/10, 0-2/10",
        ] {
            headers.insert(header::CONTENT_RANGE, range.parse().unwrap());
            assert!(validate_headers(StatusCode::PARTIAL_CONTENT, &headers, 3, 10).is_err());
        }
        headers.insert(header::CONTENT_RANGE, "bytes 3-9/10".parse().unwrap());
        headers.insert(header::CONTENT_ENCODING, "gzip".parse().unwrap());
        assert!(validate_headers(StatusCode::PARTIAL_CONTENT, &headers, 3, 10).is_err());
    }
    #[tokio::test]
    async fn real_tls_transfer_recovers_once_without_restarting_prefix_and_refuses_wrong_ranges() {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::rustls::{self, pki_types::PrivatePkcs8KeyDer};
        for valid in [true, false] {
            let rcgen::CertifiedKey { cert, signing_key } =
                rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
            )
            .unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let data = vec![42u8; 65536];
            let sent = data.clone();
            let ranges = Arc::new(Mutex::new(Vec::new()));
            let seen = ranges.clone();
            let server = tokio::spawn(async move {
                for attempt in 0..2 {
                    let (socket, _) = listener.accept().await.unwrap();
                    let mut socket = acceptor.accept(socket).await.unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        let byte = socket.read_u8().await.unwrap();
                        request.push(byte);
                        assert!(request.len() < 16384);
                    }
                    let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                    seen.lock().unwrap().push(
                        request
                            .lines()
                            .find(|line| line.starts_with("range:"))
                            .unwrap_or("")
                            .to_owned(),
                    );
                    let prefix = 3333;
                    let headers = if attempt == 0 {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            sent.len()
                        )
                    } else if valid {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {prefix}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            sent.len() - 1,
                            sent.len(),
                            sent.len() - prefix
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            sent.len()
                        )
                    };
                    socket.write_all(headers.as_bytes()).await.unwrap();
                    let bytes = if attempt == 0 {
                        &sent[..prefix]
                    } else if valid {
                        &sent[prefix..]
                    } else {
                        &sent[..]
                    };
                    let _ = socket.write_all(bytes).await;
                    let _ = socket.shutdown().await;
                }
            });
            let client = reqwest::Client::builder()
                .no_proxy()
                .add_root_certificate(reqwest::Certificate::from_der(cert.der()).unwrap())
                .resolve("localhost", address)
                .build()
                .unwrap();
            let artifact = crate::model_manifest::GgufArtifact::from_catalog(
                &serde_json::json!({"url":format!("https://localhost:{}/artifact",address.port()),
                "sha256":hex::encode(Sha256::digest(&data)),"bytes":data.len()}),
            )
            .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("owned.partial");
            let mut file = tokio::fs::File::create(&path).await.unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                receive(&client, &artifact, &mut file, |_| Ok(()), |_, _, _| Ok(())),
            )
            .await
            .unwrap();
            server.await.unwrap();
            file.flush().await.unwrap();
            file.sync_all().await.unwrap();
            drop(file);
            assert_eq!(*ranges.lock().unwrap(), ["", "range: bytes=3333-"]);
            if valid {
                assert_eq!(result.unwrap(), artifact.sha256);
                assert_eq!(std::fs::read(&path).unwrap(), data);
            } else {
                assert!(result.is_err());
                assert_eq!(std::fs::metadata(&path).unwrap().len(), 3333);
            }
        }
    }
}
