//! File-free local-owner pairing. Invitations are secrets, never execution grants.
use anyhow::{Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_INPUT_BYTES: usize = 128 * 1024;
pub const MAX_CODE_BYTES: usize = 4096;
const PAIRING_NOTE: &str = "Private invitation saved; node app restart requested, not verified. Connection-enabled preference and feature permissions are unchanged. Inspect connection and coordinator registration; no model or workload readiness is claimed.";

/// Startup may expose the local owner API before runtime initialization finishes.
/// Poll only read-only diagnostics; never submit/replay pairing while waiting.
pub async fn wait_for_runtime(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    node: &str,
) -> Result<String> {
    wait_for_runtime_bounded(client, address, node, std::time::Duration::from_secs(15)).await
}

async fn wait_for_runtime_bounded(
    client: &reqwest::Client,
    address: std::net::SocketAddr,
    node: &str,
    budget: std::time::Duration,
) -> Result<String> {
    if !address.ip().is_loopback() || address.port() == 0 {
        bail!("pairing readiness requires the node's local owner API")
    }
    crate::network::validate_remote_node_id(node)?;
    let inspection = async {
        loop {
            let response = client
                .get(format!("http://{address}/api/diagnostics"))
                .send()
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "local pairing diagnostics unavailable; no pairing command was sent"
                    )
                })?;
            if !response.status().is_success() {
                bail!("local pairing diagnostics rejected; no pairing command was sent")
            }
            let value = crate::model_manager::bounded_json(response)
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "local pairing diagnostics unconfirmed; no pairing command was sent"
                    )
                })?;
            if value["schema_version"] != 1
                || value["success"] != true
                || value["node_id"] != node
                || value["report_type"] != "node_diagnostics"
                || value.get("runtime_id").is_none()
            {
                bail!(
                    "local pairing diagnostics do not match this node; no pairing command was sent"
                )
            }
            if let Some(runtime) = value["runtime_id"].as_str() {
                if uuid::Uuid::parse_str(runtime).map_or(true, |id| id.is_nil()) {
                    bail!("local pairing runtime unconfirmed; no pairing command was sent")
                }
                return Ok(runtime.to_owned());
            }
            if !value["runtime_id"].is_null() {
                bail!("local pairing runtime unconfirmed; no pairing command was sent")
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(budget, inspection).await
        .map_err(|_| anyhow::anyhow!("node runtime is still starting; inspect eefn setup before pairing again. No pairing command was sent"))?
}

/// CLI/file adapter decoding only; it does not establish identity or authority.
pub fn input_code(bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_INPUT_BYTES {
        bail!("pairing input exceeds bounds")
    }
    let raw = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            bail!("invalid pairing input encoding")
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
        String::from_utf16(&units).map_err(|_| anyhow::anyhow!("invalid pairing input encoding"))?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("invalid pairing input encoding"))?
            .trim_start_matches('\u{feff}')
            .to_owned()
    };
    // Match the existing private-file adapter: a report or a raw base64 code.
    let report: Option<Value> = serde_json::from_str(&raw).ok();
    let code = report
        .as_ref()
        .and_then(|v| v.get("code"))
        .and_then(Value::as_str)
        .unwrap_or(raw.trim());
    if code.is_empty() || code.len() > MAX_CODE_BYTES {
        bail!("pairing code is empty or exceeds bounds")
    }
    Ok(code.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Invitation {
    schema_version: u32,
    node_id: String,
    pub(crate) address: String,
    auth_mode: crate::authentication::AuthMode,
    pub(crate) psk: String,
    pub(crate) coordinator_certificate: String,
}
impl std::fmt::Debug for Invitation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invitation([PRIVATE])")
    }
}
impl Invitation {
    pub(crate) fn parse(code: &str, node: &str) -> Result<Self> {
        if code.is_empty() || code.len() > MAX_CODE_BYTES {
            bail!("pairing code is empty or exceeds bounds")
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(code)
            .map_err(|_| anyhow::anyhow!("invalid pairing code"))?;
        let invitation: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid pairing invitation"))?;
        if invitation.schema_version != 2
            || invitation.node_id != node
            || invitation.auth_mode != crate::authentication::AuthMode::NodeCredential
        {
            bail!("pairing invitation does not match this node")
        }
        crate::authentication::validate_credential(&invitation.psk)?;
        crate::network::validate_remote_node_id(node)?;
        crate::tls::client_config(&invitation.coordinator_certificate)?;
        crate::network::validate_address(&invitation.address, true)?;
        Ok(invitation)
    }
    fn fingerprint(&self) -> Result<String> {
        crate::tls::certificate_fingerprint(&self.coordinator_certificate)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub request_id: String,
    pub code: String,
}
impl std::fmt::Debug for PairRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairRequest([PRIVATE])")
    }
}
impl PairRequest {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        for id in [&self.expected_runtime_id, &self.request_id] {
            if uuid::Uuid::parse_str(id).map_or(true, |id| id.is_nil()) {
                bail!("invalid pairing correlation identity")
            }
        }
        if self.schema_version != 1 {
            bail!("unsupported pairing command schema")
        }
        Invitation::parse(&self.code, &self.expected_node_id)?;
        Ok(())
    }
    /// No invitation/credential, endpoint or user text is included in reports.
    pub fn failure(&self, error: &str, unknown: bool) -> Value {
        json!({"schema_version":1,"report_type":"node_credential_pairing","success":false,
            "node_id":self.expected_node_id,"request_id":self.request_id,"previous_runtime_id":self.expected_runtime_id,
            "acknowledged":false,"saved":null,"restart_requested":null,"completed":false,"durable_receipt":false,
            "outcome_unknown":unknown,"execution_authorized":false,"automatically_retried":false,
            "error_code":error,"note":"No confirmed pairing reply. Inspect connection show and diagnostics before deciding whether to pair again. No fallback or automatic retry; no feature permissions were granted."})
    }
}

pub fn confirmed(value: &Value, request: &PairRequest) -> bool {
    // Never echo unexpected private/authority fields or arbitrary server prose.
    const FIELDS: &[&str] = &[
        "schema_version",
        "report_type",
        "success",
        "node_id",
        "request_id",
        "previous_runtime_id",
        "acknowledged",
        "saved",
        "restart_requested",
        "completed",
        "outcome_unknown",
        "execution_authorized",
        "automatically_retried",
        "durable_receipt",
        "saved_auth_mode",
        "auto_local",
        "local_pairing",
        "saved_coordinator_fingerprint",
        "saved_connection_enabled",
        "note",
    ];
    if value.as_object().is_none_or(|object| {
        object.len() != FIELDS.len() || object.keys().any(|field| !FIELDS.contains(&field.as_str()))
    }) {
        return false;
    }
    let Ok(invitation) = Invitation::parse(&request.code, &request.expected_node_id) else {
        return false;
    };
    let Ok(fingerprint) = invitation.fingerprint() else {
        return false;
    };
    value["schema_version"] == 1
        && value["report_type"] == "node_credential_pairing"
        && value["success"] == true
        && value["node_id"] == request.expected_node_id
        && value["request_id"] == request.request_id
        && value["previous_runtime_id"] == request.expected_runtime_id
        && value["acknowledged"] == true
        && value["saved"] == true
        && value["restart_requested"] == true
        && value["completed"] == false
        && value["outcome_unknown"] == false
        && value["execution_authorized"] == false
        && value["automatically_retried"] == false
        && value["saved_auth_mode"] == "node_credential"
        && value["auto_local"] == false
        && value["local_pairing"] == false
        && value["saved_coordinator_fingerprint"] == fingerprint
        && value["saved_connection_enabled"].is_boolean()
        && value["durable_receipt"] == false
        && value["note"] == PAIRING_NOTE
}

impl crate::NodeService {
    pub fn pair_credential_command(&self, request: PairRequest) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id
            || self.live.lock().unwrap()["runtime_id"] != request.expected_runtime_id
        {
            bail!("pairing command target or runtime changed")
        }
        let invitation = Invitation::parse(&request.code, &self.node_id)?;
        let fingerprint = invitation.fingerprint()?;
        let connection_enabled = self.save_credential_pair_for_runtime(
            &invitation.address,
            &invitation.psk,
            &invitation.coordinator_certificate,
            Some(&request.expected_runtime_id),
        )?;
        Ok(
            json!({"schema_version":1,"report_type":"node_credential_pairing","success":true,
            "node_id":self.node_id,"request_id":request.request_id,"previous_runtime_id":request.expected_runtime_id,
            "acknowledged":true,"saved":true,"restart_requested":true,"completed":false,
            "outcome_unknown":false,"execution_authorized":false,"automatically_retried":false,"durable_receipt":false,
            "saved_auth_mode":"node_credential","auto_local":false,"local_pairing":false,
            "saved_coordinator_fingerprint":fingerprint,"saved_connection_enabled":connection_enabled,
            "note":PAIRING_NOTE}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn startup_wait_polls_only_diagnostics_and_fails_closed_before_pairing() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let runtime = uuid::Uuid::new_v4().to_string();
        let base = json!({"schema_version":1,"success":true,"node_id":"stable",
            "report_type":"node_diagnostics","runtime_id":runtime});
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        for case in [
            "becomes_ready",
            "never_ready",
            "wrong_node",
            "missing_runtime",
            "invalid_runtime",
            "nil_runtime",
        ] {
            let gets = Arc::new(AtomicUsize::new(0));
            let posts = Arc::new(AtomicUsize::new(0));
            let read_count = gets.clone();
            let write_count = posts.clone();
            let report = base.clone();
            let router = axum::Router::new()
                .route(
                    "/api/diagnostics",
                    axum::routing::get(move || {
                        let count = read_count.fetch_add(1, Ordering::SeqCst);
                        let mut value = report.clone();
                        match case {
                            "becomes_ready" if count == 0 => value["runtime_id"] = Value::Null,
                            "never_ready" => value["runtime_id"] = Value::Null,
                            "wrong_node" => value["node_id"] = json!("other"),
                            "missing_runtime" => {
                                value.as_object_mut().unwrap().remove("runtime_id");
                            }
                            "invalid_runtime" => value["runtime_id"] = json!("PRIVATE-invalid"),
                            "nil_runtime" => {
                                value["runtime_id"] = json!(uuid::Uuid::nil().to_string())
                            }
                            _ => {}
                        }
                        async move { axum::Json(value) }
                    }),
                )
                .route(
                    "/api/commands/connection/pair",
                    axum::routing::post(move || {
                        write_count.fetch_add(1, Ordering::SeqCst);
                        async { axum::Json(json!({"unexpected":true})) }
                    }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let result = wait_for_runtime_bounded(
                &client,
                address,
                "stable",
                std::time::Duration::from_millis(350),
            )
            .await;
            server.abort();
            if case == "becomes_ready" {
                assert_eq!(result.unwrap(), runtime);
                assert_eq!(gets.load(Ordering::SeqCst), 2);
            } else {
                let error = result.unwrap_err().to_string();
                assert!(!error.contains("PRIVATE"));
                if case != "never_ready" {
                    assert_eq!(gets.load(Ordering::SeqCst), 1);
                }
            }
            assert_eq!(posts.load(Ordering::SeqCst), 0);
        }
    }
    fn make_invitation(node: &str) -> (String, Value) {
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let value = json!({"schema_version":2,"node_id":node,"address":"127.0.0.1:51335",
            "auth_mode":"node_credential","psk":crate::authentication::generate_credential(),
            "coordinator_certificate":identity.certificate});
        (
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&value).unwrap()),
            value,
        )
    }
    fn request(node: &str, runtime: &str, code: String) -> PairRequest {
        PairRequest {
            schema_version: 1,
            expected_node_id: node.into(),
            expected_runtime_id: runtime.into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            code,
        }
    }
    #[test]
    fn file_and_pipe_decoding_share_bounds_and_encodings() {
        let (code, _) = make_invitation("stable");
        let report = json!({"private":true,"code":code}).to_string();
        for text in [format!(" \n{code}\r\n"), format!("\u{feff}{report}")] {
            assert_eq!(input_code(text.as_bytes()).unwrap(), code);
        }
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in report.encode_utf16() {
                bytes.extend(if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert_eq!(input_code(&bytes).unwrap(), code);
        }
        for bad in [
            vec![],
            vec![0xff],
            vec![0xff, 0xfe, 0x00],
            vec![b'x'; MAX_INPUT_BYTES + 1],
            vec![b'x'; MAX_CODE_BYTES + 1],
        ] {
            assert!(input_code(&bad).is_err());
        }
    }
    #[test]
    fn typed_private_requests_reject_extra_authority_and_invalid_identity() {
        let runtime = uuid::Uuid::new_v4().to_string();
        let (code, value) = make_invitation("stable");
        let good = request("stable", &runtime, code);
        good.validate().unwrap();
        assert!(!format!("{good:?}").contains(&good.code));
        assert!(
            !format!("{:?}", Invitation::parse(&good.code, "stable").unwrap())
                .contains(value["psk"].as_str().unwrap())
        );
        let wire = serde_json::to_value(&good).unwrap();
        for (field, value) in [
            ("schema_version", json!(2)),
            ("expected_runtime_id", json!(uuid::Uuid::nil().to_string())),
            ("request_id", json!("bad")),
            ("expected_node_id", json!("other")),
            ("code", json!("private-malformed-token")),
        ] {
            let mut bad = wire.clone();
            bad[field] = value;
            assert!(
                serde_json::from_value::<PairRequest>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut bad = wire;
        bad["permissions"] = json!(true);
        assert!(serde_json::from_value::<PairRequest>(bad).is_err());
        let failed = good.failure("local_pairing_reply_unconfirmed", true);
        assert_eq!(failed["saved"], Value::Null);
        assert_eq!(failed["durable_receipt"], false);
        assert!(!failed.to_string().contains(&good.code));
        assert!(!failed.to_string().contains(value["psk"].as_str().unwrap()));
    }
    #[tokio::test]
    async fn pairing_is_target_bound_preserves_policy_and_only_requests_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path.clone(), "stable".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let original = json!({"node_id":"stable","name":"Kept name","connection_enabled":false,
            "permissions":{"remote_updates":false},"models":{"provider":"auto","ollama":{"selected":[]}},"custom":42});
        service.save_config(original.clone()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let (code, invitation) = make_invitation("stable");
        for bad in [
            request("stable", &uuid::Uuid::new_v4().to_string(), code.clone()),
            request("other", &runtime, code.clone()),
            request("stable", &runtime, make_invitation("other").0),
        ] {
            assert!(service.pair_credential_command(bad).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                service.restart.notified()
            )
            .await
            .is_err()
        );
        let mut changed_identity = original.clone();
        changed_identity["node_id"] = json!("other");
        std::fs::write(&path, serde_json::to_vec(&changed_identity).unwrap()).unwrap();
        let altered = std::fs::read(&path).unwrap();
        assert!(
            service
                .pair_credential_command(request("stable", &runtime, code.clone()))
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), altered);
        // Undo only the test's simulated external file corruption. The general
        // settings service deliberately cannot rename an existing stable ID.
        std::fs::write(&path, &before).unwrap();
        let wire = request("stable", &runtime, code);
        let expected = serde_json::to_value(&wire).unwrap();
        let report = service.pair_credential_command(wire).unwrap();
        let wire: PairRequest = serde_json::from_value(expected).unwrap();
        assert!(confirmed(&report, &wire));
        let saved = service.read_config().unwrap();
        for field in [
            "node_id",
            "name",
            "connection_enabled",
            "permissions",
            "models",
            "custom",
        ] {
            assert_eq!(saved[field], original[field]);
        }
        assert_eq!(saved["psk"], invitation["psk"]);
        assert_eq!(
            saved["coordinator_certificate"],
            invitation["coordinator_certificate"]
        );
        assert_eq!(report["saved_connection_enabled"], false);
        assert_eq!(report["completed"], false);
        assert!(
            !report
                .to_string()
                .contains(invitation["psk"].as_str().unwrap())
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.restart.notified(),
        )
        .await
        .unwrap();
    }
    #[test]
    fn saved_confirmation_cannot_claim_connection_or_replace_request_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path, "stable".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let code = make_invitation("stable").0;
        let request = request("stable", &runtime, code);
        let wire = serde_json::to_value(&request).unwrap();
        let report = service.pair_credential_command(request).unwrap();
        let request: PairRequest = serde_json::from_value(wire).unwrap();
        assert!(confirmed(&report, &request));
        for (key, value) in [
            ("node_id", json!("wrong")),
            ("request_id", json!(uuid::Uuid::new_v4().to_string())),
            (
                "previous_runtime_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
            ("saved_coordinator_fingerprint", json!("0".repeat(64))),
            ("completed", json!(true)),
            ("execution_authorized", json!(true)),
            ("saved_auth_mode", json!("legacy")),
            ("auto_local", json!(true)),
            ("automatically_retried", json!(true)),
            ("durable_receipt", json!(true)),
            ("note", json!(request.code)),
            ("code", json!(request.code)),
            ("permissions", json!(true)),
        ] {
            let mut corrupt = report.clone();
            corrupt[key] = value;
            assert!(!confirmed(&corrupt, &request));
        }
    }
}
