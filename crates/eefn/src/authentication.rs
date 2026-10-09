//! Opt-in exact-node credentials. Reuses the existing encrypted protocol;
//! this is not a session-scoped execution grant or peer-session protocol.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    Legacy,
    NodeCredential,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthenticationPolicy {
    #[serde(default = "legacy_allowed")]
    pub allow_legacy: bool,
    /// Null is a revoked identity, not permission to use the legacy key.
    pub credentials: BTreeMap<String, Option<String>>,
}
fn legacy_allowed() -> bool {
    true
}
impl Default for AuthenticationPolicy {
    fn default() -> Self {
        Self {
            allow_legacy: true,
            credentials: BTreeMap::new(),
        }
    }
}

impl AuthenticationPolicy {
    pub fn parse(value: Option<&Value>) -> Result<Self> {
        let policy: Self = serde_json::from_value(value.cloned().unwrap_or_else(|| json!({})))
            .map_err(|_| anyhow::anyhow!("invalid authentication policy"))?;
        policy.validate()?;
        Ok(policy)
    }
    pub fn validate(&self) -> Result<()> {
        if self.credentials.len() > 256 {
            bail!("authentication identity limit reached")
        }
        let mut keys = HashSet::new();
        for (id, key) in &self.credentials {
            crate::network::validate_node_id(id)?;
            if id == "*" {
                bail!("credentials require exact node identities")
            }
            if let Some(key) = key {
                validate_credential(key)?;
                if !keys.insert(key.to_ascii_lowercase()) {
                    bail!("node credentials must be independent")
                }
            }
        }
        Ok(())
    }
    pub fn validate_legacy_key(&self, key: &str) -> Result<()> {
        if self
            .credentials
            .values()
            .flatten()
            .any(|v| v.eq_ignore_ascii_case(key))
        {
            bail!("a node credential cannot reuse the network key")
        }
        Ok(())
    }
    pub fn summary(&self) -> Value {
        json!({"allow_legacy":self.allow_legacy,"identities":self.credentials.iter()
            .map(|(id,key)| json!({"node_id":id,"state":if key.is_some(){"provisioned"}else{"revoked"}}))
            .collect::<Vec<_>>()})
    }
    pub fn redact(value: &mut Value) {
        if let Some(keys) = value
            .pointer_mut("/node/authentication/credentials")
            .and_then(Value::as_object_mut)
        {
            for key in keys.values_mut() {
                if !key.is_null() {
                    *key = json!(crate::service::SECRET_PLACEHOLDER);
                }
            }
        }
    }
}

pub fn validate_credential(key: &str) -> Result<()> {
    // Generated 256-bit secrets only, rather than human passwords.
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("node credential must be a generated 256-bit hexadecimal secret")
    }
    Ok(())
}

pub fn generate_credential() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

pub async fn write_selector<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    node_id: &str,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    crate::network::validate_node_id(node_id)?;
    if node_id == "*" {
        bail!("credentials require exact node identities")
    }
    let mut bytes = serde_json::to_vec(
        &json!({"type":"node_credential","schema_version":1,"node_id":node_id}),
    )?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialSelector {
    pub r#type: String,
    pub schema_version: u32,
    pub node_id: String,
}

impl crate::NodeService {
    pub(crate) fn pair_with_credential_file(&self, path: &std::path::Path) -> Result<()> {
        use base64::Engine as _;
        use std::io::Read;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Invitation {
            schema_version: u32,
            node_id: String,
            address: String,
            auth_mode: AuthMode,
            psk: String,
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            bail!("pairing file exceeds bounds")
        }
        // PowerShell 5 redirects as UTF-16; modern UTF-8/BOM output also works.
        let raw = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
            if bytes.len() % 2 != 0 {
                bail!("invalid pairing file encoding")
            }
            let little = bytes[0] == 0xff;
            let units = bytes[2..]
                .chunks_exact(2)
                .map(|b| {
                    if little {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    }
                })
                .collect::<Vec<_>>();
            String::from_utf16(&units)
                .map_err(|_| anyhow::anyhow!("invalid pairing file encoding"))?
        } else {
            std::str::from_utf8(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid pairing file encoding"))?
                .trim_start_matches('\u{feff}')
                .to_owned()
        };
        // Accept redirected JSON command output, or the raw private code.
        let report: Option<Value> = serde_json::from_str(&raw).ok();
        let code = report
            .as_ref()
            .and_then(|v| v.get("code"))
            .and_then(Value::as_str)
            .unwrap_or(raw.trim());
        if code.len() > 4096 {
            bail!("pairing code exceeds bounds")
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(code)
            .map_err(|_| anyhow::anyhow!("invalid pairing code"))?;
        let invitation: Invitation = serde_json::from_slice(&decoded)
            .map_err(|_| anyhow::anyhow!("invalid pairing invitation"))?;
        if invitation.schema_version != 1
            || invitation.node_id != self.node_id
            || invitation.auth_mode != AuthMode::NodeCredential
        {
            bail!("pairing invitation does not match this node")
        }
        validate_credential(&invitation.psk)?;
        crate::network::validate_address(&invitation.address, true)?;
        self.save_credential_pair(&invitation.address, &invitation.psk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_pairing_preserves_identity_permissions_and_rejects_bad_invitations() {
        use base64::Engine as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path.clone(), "stable".into());
        service
            .save_config(
                json!({"node_id":"stable","psk":"legacy-secret","endpoints":[],
            "permissions":{"remote_updates":false},"custom":42}),
            )
            .unwrap();
        let key = generate_credential();
        let invitation = json!({"schema_version":1,"node_id":"stable","address":"127.0.0.1:51335",
            "auth_mode":"node_credential","psk":key});
        let file = dir.path().join("private-code.json");
        for bad in [
            json!({"node_id":"wrong"}),
            json!({"psk":"password"}),
            json!({"auth_mode":"legacy"}),
            json!({"permissions":true}),
            json!({"address":"http://bad/path"}),
        ] {
            let mut value = invitation.clone();
            for (key, field) in bad.as_object().unwrap() {
                value[key] = field.clone();
            }
            std::fs::write(
                &file,
                base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&value).unwrap()),
            )
            .unwrap();
            let before = std::fs::read(&path).unwrap();
            assert!(service.pair_with_credential_file(&file).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        std::fs::write(&file, serde_json::to_vec(&json!({"code":base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&invitation).unwrap())})).unwrap()).unwrap();
        service.pair_with_credential_file(&file).unwrap();
        let saved = service.read_config().unwrap();
        assert_eq!(saved["node_id"], "stable");
        assert_eq!(saved["permissions"]["remote_updates"], false);
        assert_eq!(saved["custom"], 42);
        assert_eq!(saved["auto_local"], false);
        assert_eq!(saved["auth_mode"], "node_credential");
        assert_eq!(saved["psk"], key);
        assert!(!service.configuration().unwrap().to_string().contains(&key));
        assert!(!crate::setup::pair_local(&path).unwrap());
        assert!(service.pair_with_local_coordinator().is_err());
        assert_eq!(service.read_config().unwrap(), saved);
        std::fs::write(&file, vec![b'x'; 128 * 1024 + 1]).unwrap();
        assert!(service.pair_with_credential_file(&file).is_err());
    }
    #[test]
    fn legacy_defaults_revocation_bounds_independence_and_redaction() {
        assert!(AuthenticationPolicy::parse(None).unwrap().allow_legacy);
        let key = "a".repeat(64);
        let mut value = json!({"node":{"authentication":{"credentials":{"a":key,"b":null}}}});
        let saved = value.clone();
        let policy = AuthenticationPolicy::parse(value.pointer("/node/authentication")).unwrap();
        assert!(policy.validate_legacy_key(&key).is_err());
        assert!(!policy.summary().to_string().contains(&key));
        AuthenticationPolicy::redact(&mut value);
        assert!(!value.to_string().contains(&key));
        assert_eq!(saved["node"]["authentication"]["credentials"]["a"], key);
        for bad in [
            json!({"credentials":{"a":key,"b":key}}),
            json!({"credentials":{"a":"password"}}),
            json!({"allow_legacy":null}),
            json!({"unknown":true}),
            json!({"credentials":{"*":key}}),
        ] {
            assert!(AuthenticationPolicy::parse(Some(&bad)).is_err());
        }
    }
}
