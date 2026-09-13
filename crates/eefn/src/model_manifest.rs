//! Normalized, versioned GGUF artifact metadata shared by planning and transfer.
//! Valid metadata is not a license review, runtime compatibility proof or signature.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct GgufArtifact {
    pub schema_version: u32,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

impl GgufArtifact {
    pub fn from_catalog(entry: &Value) -> Result<Self> {
        let raw_url = entry["url"]
            .as_str()
            .filter(|s| s.len() <= 8192)
            .context("Model download address is missing or too long")?;
        let url = reqwest::Url::parse(raw_url).context("Invalid model download address")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            bail!("Model downloads require HTTPS without embedded credentials or fragments");
        }
        let sha256 = entry["sha256"]
            .as_str()
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .context("Model catalog needs a SHA-256 checksum")?;
        let bytes = entry["bytes"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= i64::MAX as u64)
            .context("Model catalog needs a positive supported download size")?;
        Ok(Self {
            schema_version: 1,
            url: url.to_string(),
            sha256: sha256.to_owned(),
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn shared_artifact_contract_rejects_incomplete_or_unsafe_metadata() {
        let valid = json!({"url":"https://example.invalid/model.gguf", "sha256":"a".repeat(64), "bytes":10});
        let artifact = GgufArtifact::from_catalog(&valid).unwrap();
        assert_eq!(artifact.schema_version, 1);
        for (key, value) in [
            ("url", json!("http://example.invalid/model.gguf")),
            (
                "url",
                json!("https://user:secret@example.invalid/model.gguf"),
            ),
            ("url", json!("https://example.invalid/model.gguf#hidden")),
            ("url", json!("x".repeat(8193))),
            ("sha256", json!("../outside")),
            ("sha256", json!("g".repeat(64))),
            ("bytes", json!(0)),
            ("bytes", json!(u64::MAX)),
            ("bytes", json!("10")),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(GgufArtifact::from_catalog(&invalid).is_err());
        }
    }
}
