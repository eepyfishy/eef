use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use eefn::firmware::{
    CapabilitySpec, FirmwareConfig, FirmwareEntry, FirmwareRepo, generate_firmware, ota_stream,
    resolve_esp,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct FirmwareService {
    repo: Arc<Mutex<FirmwareRepo>>,
    server: eefn::NodeServer,
    defaults: FirmwareConfig,
}

impl FirmwareService {
    pub fn open(root: PathBuf, server: eefn::NodeServer, defaults: FirmwareConfig) -> Result<Self> {
        Ok(Self {
            repo: Arc::new(Mutex::new(FirmwareRepo::open(root)?)),
            server,
            defaults,
        })
    }

    pub async fn generate(
        &self,
        description: &str,
        node_id: &str,
        node_name: &str,
        version: &str,
        overrides: Option<Value>,
    ) -> Result<Value> {
        let capabilities = resolve_esp(description)?;
        let mut config = self.defaults.clone();
        config.node_id = node_id.into();
        config.node_name = node_name.into();
        apply_overrides(&mut config, overrides.as_ref());
        let source = generate_firmware(&config, &capabilities, version);
        let entry = self.repo.lock().await.put(node_id, &source, version)?;
        Ok(json!({"entry": entry, "capabilities": capabilities, "source": source}))
    }

    pub async fn push(
        &self,
        node_id: &str,
        source: Option<String>,
        version: &str,
    ) -> Result<Value> {
        let (source, name) = if let Some(source) = source {
            (source, format!("{node_id}-v{version}.ino"))
        } else {
            let entry = self
                .repo
                .lock()
                .await
                .latest(node_id)
                .cloned()
                .with_context(|| format!("no firmware stored for '{node_id}'"))?;
            (
                tokio::fs::read_to_string(&entry.file).await?,
                entry
                    .file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("firmware.ino")
                    .into(),
            )
        };
        ota_stream(
            &self.server,
            node_id,
            &name,
            source.as_bytes(),
            version,
            Duration::from_secs(30),
        )
        .await
    }

    pub async fn list(&self) -> Vec<FirmwareEntry> {
        self.repo
            .lock()
            .await
            .all()
            .iter()
            .filter(|(key, _)| !key.ends_with("@latest"))
            .map(|(_, entry)| entry.clone())
            .collect()
    }
}

fn apply_overrides(config: &mut FirmwareConfig, value: Option<&Value>) {
    let Some(value) = value else { return };
    if let Some(value) = value.get("ssid").and_then(Value::as_str) {
        config.ssid = value.into();
    }
    if let Some(value) = value.get("wifi_pass").and_then(Value::as_str) {
        config.wifi_pass = value.into();
    }
    if let Some(value) = value.get("host").and_then(Value::as_str) {
        config.host = value.into();
    }
    if let Some(value) = value
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
    {
        config.port = value;
    }
    if let Some(value) = value.get("psk").and_then(Value::as_str) {
        config.psk = value.into();
    }
}

pub fn capabilities_to_value(capabilities: &[CapabilitySpec]) -> Value {
    serde_json::to_value(capabilities).unwrap_or_else(|_| json!([]))
}
