//! Shared owner-only enrollment contracts; never execution grants.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PREPARE_NOTE: &str = "PRIVATE reusable invitation. Saved trust is not active registration. No feature permissions, models, work or restart were requested.";
pub const RESTART_NOTE: &str = "Coordinator restart requested, not completion. This applies all pending coordinator settings and interrupts work; no workload is replayed by enrollment.";

pub fn digest(policy: &crate::authentication::AuthenticationPolicy) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(policy)?)))
}
pub fn hash_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn runtime_valid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub node_id: String,
    pub address: std::net::SocketAddr,
    #[serde(deserialize_with = "Option::deserialize")]
    pub expected_fingerprint: Option<String>,
    pub allow_coordinator_restart: bool,
}
impl PrepareRequest {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        if self.schema_version != 1
            || !runtime_valid(&self.expected_runtime_id)
            || !self.address.ip().is_loopback()
            || self.address.port() == 0
            || self
                .expected_fingerprint
                .as_ref()
                .is_some_and(|fp| !hash_valid(fp))
        {
            bail!("invalid local enrollment preparation request")
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub node_id: String,
    pub fingerprint: String,
    pub authentication_digest: String,
    pub changed: bool,
    pub restart_required: bool,
    pub execution_authorized: bool,
    pub code: String,
    pub note: String,
}
impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Prepared([PRIVATE])")
    }
}
impl Prepared {
    pub fn validate(&self, request: &PrepareRequest) -> Result<()> {
        request.validate()?;
        let invitation =
            crate::credential_pairing::Invitation::parse(&self.code, &request.node_id)?;
        if self.schema_version != 1
            || self.report_type != "coordinator_enrollment_prepared"
            || !self.success
            || self.runtime_id != request.expected_runtime_id
            || self.node_id != request.node_id
            || self.execution_authorized
            || !hash_valid(&self.authentication_digest)
            || self.note != PREPARE_NOTE
            || invitation.address != request.address.to_string()
            || crate::tls::certificate_fingerprint(&invitation.coordinator_certificate)?
                != self.fingerprint
            || request
                .expected_fingerprint
                .as_ref()
                .is_some_and(|fp| fp != &self.fingerprint)
            || (self.restart_required && !request.allow_coordinator_restart)
            || (self.changed && !self.restart_required)
        {
            bail!("private enrollment preparation reply unconfirmed")
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub saved_digest: String,
    pub applied_digest: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub fingerprint: Option<String>,
}
impl Status {
    pub fn valid(&self) -> bool {
        self.schema_version == 1
            && self.report_type == "coordinator_enrollment_status"
            && self.success
            && runtime_valid(&self.runtime_id)
            && hash_valid(&self.saved_digest)
            && hash_valid(&self.applied_digest)
            && self.fingerprint.as_ref().is_none_or(|fp| hash_valid(fp))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub expected_authentication_digest: String,
}
impl RestartRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || !runtime_valid(&self.expected_runtime_id)
            || !hash_valid(&self.expected_authentication_digest)
        {
            bail!("invalid enrollment restart request")
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub authentication_digest: String,
    pub restart_requested: bool,
    pub completed: bool,
    pub note: String,
}
impl RestartReport {
    pub fn valid(&self, request: &RestartRequest) -> bool {
        request.validate().is_ok()
            && self.schema_version == 1
            && self.report_type == "coordinator_enrollment_restart"
            && self.success
            && self.runtime_id == request.expected_runtime_id
            && self.authentication_digest == request.expected_authentication_digest
            && self.restart_requested
            && !self.completed
            && self.note == RESTART_NOTE
    }
}

/// Private snapshot stays inside the node service, never the public report.
pub(crate) fn connection_snapshot(config: &Value) -> Value {
    let mut snapshot = serde_json::Map::new();
    for field in [
        "node_id",
        "auth_mode",
        "psk",
        "coordinator_certificate",
        "endpoints",
        "auto_local",
        "local_pairing",
        "connection_enabled",
    ] {
        snapshot.insert(
            field.into(),
            config.get(field).cloned().unwrap_or(Value::Null),
        );
    }
    Value::Object(snapshot)
}
