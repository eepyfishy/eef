use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::NodeCrypto;

pub const PROTOCOL_VERSION: u64 = 1;
pub const MAX_WIRE_LINE: usize = 8 * 1024 * 1024;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn encode_line(crypto: &NodeCrypto, message: &Value) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(message)?;
    let mut line = crypto.encrypt(&payload, b"")?.into_bytes();
    line.push(b'\n');
    Ok(line)
}

pub fn decode_line(crypto: &NodeCrypto, line: &[u8]) -> Result<Value> {
    if line.len() > MAX_WIRE_LINE {
        bail!("wire message exceeds {MAX_WIRE_LINE} bytes");
    }
    let token = std::str::from_utf8(line)?.trim();
    if token.is_empty() {
        bail!("empty wire message");
    }
    let plaintext = crypto.decrypt(token, b"")?;
    serde_json::from_slice(&plaintext).context("decrypted message is not JSON")
}

pub async fn read_message<R>(crypto: &NodeCrypto, reader: &mut R) -> Result<Option<Value>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    let count = reader.read_until(b'\n', &mut line).await?;
    if count == 0 {
        return Ok(None);
    }
    if line.len() > MAX_WIRE_LINE {
        bail!("wire message exceeds {MAX_WIRE_LINE} bytes");
    }
    Ok(Some(decode_line(crypto, &line)?))
}

pub async fn write_message<W>(crypto: &NodeCrypto, writer: &mut W, message: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&encode_line(crypto, message)?).await?;
    writer.flush().await?;
    Ok(())
}

pub fn build_auth(crypto: &NodeCrypto, node_id: &str, version: &str) -> Value {
    let nonce = now_ms().to_string();
    let timestamp = now_ms().to_string();
    let raw = format!("{nonce}|{node_id}|{version}|{timestamp}");
    json!({
        "type": "auth",
        "protocol": PROTOCOL_VERSION,
        "node_id": node_id,
        "version": version,
        "nonce": nonce,
        "timestamp": timestamp,
        "signature": crypto.sign(raw.as_bytes()),
    })
}

pub fn build_register(
    node_id: &str,
    name: &str,
    version: &str,
    capabilities: &[String],
    specs: &Value,
    models: &[Value],
) -> Value {
    json!({
        "type": "register",
        "node_id": node_id,
        "name": name,
        "version": version,
        "capabilities": capabilities,
        "specs": specs,
        "models": models,
    })
}

pub fn build_heartbeat(node_id: &str, load: &Value, latency_ms: f64, specs: &Value) -> Value {
    json!({
        "type": "heartbeat",
        "node_id": node_id,
        "load": load,
        "latency_ms": latency_ms,
        "specs": specs,
    })
}

pub fn success_response(request_id: &str, data: Value) -> Value {
    json!({"type": "response", "id": request_id, "success": true, "data": data})
}

pub fn error_response(request_id: &str, error: impl std::fmt::Display) -> Value {
    json!({"type": "response", "id": request_id, "success": false, "error": error.to_string()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_line_round_trip() {
        let crypto = NodeCrypto::new("secret").unwrap();
        let value = json!({"type": "heartbeat", "load": {"cpu": 0.25}});
        let line = encode_line(&crypto, &value).unwrap();
        assert_eq!(decode_line(&crypto, &line).unwrap(), value);
        assert!(!String::from_utf8_lossy(&line).contains("heartbeat"));
    }
}
