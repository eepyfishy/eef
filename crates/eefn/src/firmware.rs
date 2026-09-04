use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::NodeServer;

pub const CHUNK_BYTES: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CapabilitySpec {
    pub ability: String,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub params: Value,
}

fn default_action() -> String {
    "run".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirmwareConfig {
    pub ssid: String,
    pub wifi_pass: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_node")]
    pub node_id: String,
    #[serde(default = "default_node")]
    pub node_name: String,
    #[serde(default = "default_psk")]
    pub psk: String,
}

const fn default_port() -> u16 {
    51335
}
fn default_node() -> String {
    "esp-01".into()
}
fn default_psk() -> String {
    String::new()
}

pub fn resolve_esp(text: &str) -> Result<Vec<CapabilitySpec>> {
    let text = text.to_lowercase();
    let mut caps = Vec::new();
    if text.contains("servo") {
        caps.push(CapabilitySpec {
            ability: "servo".into(),
            action: "write".into(),
            params: json!({"pin": pin_after(&text, "servo").unwrap_or(4), "angle": 0}),
        });
    }
    if ["temperature", "temp", "sensor_temperature"]
        .iter()
        .any(|word| text.contains(word))
    {
        let pin = pin_after(&text, "sensor")
            .or_else(|| pin_after(&text, "temperature"))
            .or_else(|| pin_after(&text, "temp"))
            .unwrap_or(4);
        caps.push(CapabilitySpec {
            ability: "sensor_temperature".into(),
            action: "read".into(),
            params: json!({"pin": pin}),
        });
    }
    let gpio = Regex::new(r"gpio\s+(\d+)").expect("static regex");
    if let Some(captures) = gpio.captures(&text) {
        let pin = captures[1].parse::<u32>()?;
        caps.push(CapabilitySpec {
            ability: "gpio".into(),
            action: "write".into(),
            params: json!({"pin": pin, "value": 0}),
        });
    }
    if caps.is_empty() {
        bail!("could not resolve any ESP ability from that description");
    }
    Ok(caps)
}

fn pin_after(text: &str, keyword: &str) -> Option<u32> {
    let start = text.find(keyword)?;
    let tail = &text[start..];
    let clause = tail.split([';', ',', '.']).next().unwrap_or(tail);
    Regex::new(r"pin\s*(\d+)")
        .ok()?
        .captures(clause)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

pub fn generate_firmware(
    config: &FirmwareConfig,
    caps: &[CapabilitySpec],
    version: &str,
) -> String {
    let abilities = caps
        .iter()
        .map(|cap| cap.ability.as_str())
        .collect::<Vec<_>>();
    let mut source = format!(
        "/* EEF generated firmware v{version}; do not edit. */\n#include <WiFi.h>\n#include <ArduinoJson.h>\n\nconst char* SSID = {};\nconst char* WIFI_PASS = {};\nconst char* EEF_HOST = {};\nconst uint16_t EEF_PORT = {};\nconst char* NODE_ID = {};\nconst char* NODE_NAME = {};\nconst char* PSK = {};\n// capabilities: {}\n\nvoid handleFirmwareBegin(JsonDocument& doc) {{}}\nvoid handleFirmwareChunk(JsonDocument& doc) {{}}\nvoid handleFirmwareApply(JsonDocument& doc) {{ ESP.restart(); }}\n",
        c_string(&config.ssid),
        c_string(&config.wifi_pass),
        c_string(&config.host),
        config.port,
        c_string(&config.node_id),
        c_string(&config.node_name),
        c_string(&config.psk),
        abilities.join(","),
    );
    for cap in caps {
        let pin = cap.params.get("pin").and_then(Value::as_u64).unwrap_or(4);
        match cap.ability.as_str() {
            "gpio" => source.push_str(&format!("\n// capability: gpio\nvoid handleGpioWrite(JsonDocument& doc) {{ int pin = doc[\"params\"][\"pin\"] | {pin}; pinMode(pin, OUTPUT); digitalWrite(pin, doc[\"params\"][\"value\"] | 0); }}\n")),
            "servo" => source.push_str(&format!("\n// capability: servo\n#include <ESP32Servo.h>\nServo servo_{pin};\nvoid handleServoWrite(JsonDocument& doc) {{ if (!servo_{pin}.attached()) servo_{pin}.attach({pin}); servo_{pin}.write(doc[\"params\"][\"angle\"] | 0); }}\n")),
            "sensor_temperature" => source.push_str("\n// capability: sensor_temperature (ESP32 internal sensor)\nfloat read_temperature_c() { return temperatureRead(); }\nvoid handleSensorRead(JsonDocument& doc) { (void)doc; }\n"),
            _ => {}
        }
    }
    source
}

fn c_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FirmwareEntry {
    pub node_id: String,
    pub version: String,
    pub sha256: String,
    pub file: PathBuf,
}

pub struct FirmwareRepo {
    root: PathBuf,
    index: BTreeMap<String, FirmwareEntry>,
}

impl FirmwareRepo {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let index_path = root.join("index.json");
        let index = if index_path.is_file() {
            serde_json::from_slice(&fs::read(index_path)?).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        Ok(Self { root, index })
    }

    pub fn put(&mut self, node_id: &str, source: &str, version: &str) -> Result<FirmwareEntry> {
        let sha256 = hex::encode(Sha256::digest(source.as_bytes()));
        let file = self.root.join(format!("{node_id}-v{version}.ino"));
        fs::write(&file, source)?;
        let entry = FirmwareEntry {
            node_id: node_id.into(),
            version: version.into(),
            sha256,
            file,
        };
        self.index
            .insert(format!("{node_id}@{version}"), entry.clone());
        self.index
            .insert(format!("{node_id}@latest"), entry.clone());
        fs::write(
            self.root.join("index.json"),
            serde_json::to_vec_pretty(&self.index)?,
        )?;
        Ok(entry)
    }

    pub fn latest(&self, node_id: &str) -> Option<&FirmwareEntry> {
        self.index.get(&format!("{node_id}@latest"))
    }
    pub fn get(&self, node_id: &str, version: &str) -> Option<&FirmwareEntry> {
        self.index.get(&format!("{node_id}@{version}"))
    }
    pub fn all(&self) -> &BTreeMap<String, FirmwareEntry> {
        &self.index
    }
}

pub async fn ota_stream(
    server: &NodeServer,
    node_id: &str,
    firmware_name: &str,
    data: &[u8],
    version: &str,
    timeout: Duration,
) -> Result<Value> {
    let sha256 = hex::encode(Sha256::digest(data));
    let chunks = data.len().div_ceil(CHUNK_BYTES);
    server
        .invoke_remote(
            node_id,
            "firmware.begin",
            "run",
            json!({
                "name": firmware_name, "size": data.len(), "num_chunks": chunks,
                "sha256": sha256, "version": version,
            }),
            timeout,
        )
        .await
        .context("firmware.begin failed")?;
    for (index, chunk) in data.chunks(CHUNK_BYTES).enumerate() {
        server
            .invoke_remote(
                node_id,
                "firmware.chunk",
                "run",
                json!({
                    "index": index, "data": STANDARD.encode(chunk),
                }),
                timeout,
            )
            .await
            .with_context(|| format!("firmware.chunk {index} failed"))?;
    }
    let apply = server
        .invoke_remote(
            node_id,
            "firmware.apply",
            "run",
            json!({"sha256": sha256}),
            timeout,
        )
        .await
        .context("firmware.apply failed")?;
    Ok(
        json!({"node_id": node_id, "sha256": sha256, "size": data.len(), "chunks": chunks, "apply": apply}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_and_generate() {
        let caps = resolve_esp("servo on pin 3 and temp sensor on pin 4").unwrap();
        assert_eq!(caps[0].params["pin"], 3);
        assert_eq!(caps[1].params["pin"], 4);
        let cfg = FirmwareConfig {
            ssid: "wifi".into(),
            wifi_pass: "pw".into(),
            host: "host".into(),
            port: 51335,
            node_id: "esp".into(),
            node_name: "ESP".into(),
            psk: "secret".into(),
        };
        let source = generate_firmware(&cfg, &caps, "1.2.3");
        assert!(source.contains("capability: servo"));
        assert!(source.contains("capability: sensor_temperature"));
    }

    #[test]
    fn repository_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = FirmwareRepo::open(dir.path()).unwrap();
        let entry = repo.put("esp", "source", "1.0.0").unwrap();
        assert_eq!(repo.latest("esp").unwrap().sha256, entry.sha256);
        assert!(entry.file.is_file());
    }
}
