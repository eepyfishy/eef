//! Local-owner credential provisioning. Never grants remote device permissions.
use anyhow::{Result, bail};
use base64::Engine as _;
use eefn::authentication::AuthenticationPolicy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialCommand {
    Show,
    /// Create a PRIVATE pairing code for one exact node; applies after EEF restart.
    Provision {
        #[arg(long)]
        node: String,
        #[arg(long)]
        address: String,
        /// Explicitly replace an existing/revoked credential. Never automatic.
        #[arg(long, default_value_t = false)]
        rotate: bool,
    },
    /// Export a PRIVATE invitation using an existing saved credential; never rotates.
    Export {
        #[arg(long)]
        node: String,
        #[arg(long)]
        address: String,
    },
    /// Revoke the exact identity, including shared-key fallback, after EEF restart.
    Revoke {
        #[arg(long)]
        node: String,
    },
    /// Explicitly allow/deny legacy shared-key nodes after EEF restart.
    Legacy {
        #[arg(long, action = clap::ArgAction::Set, required = true)]
        allow: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: CredentialCommand,
}

const CREDENTIAL_NOTE: &str = "Credential changes apply after explicit EEF restart. Existing sessions are not revoked until restart. Pairing never grants node feature permissions; no automatic replay was sent.";

/// Validate a private export before an adapter prints or pipes it. Inspection
/// summaries cannot authorize work; never echo additional fields or server prose.
pub fn confirmed_export(
    report: &Value,
    runtime: &str,
    node: &str,
    address: &str,
    saved_summary: &Value,
    applied_summary: &Value,
) -> bool {
    const FIELDS: &[&str] = &[
        "schema_version",
        "report_type",
        "success",
        "changed",
        "saved_policy",
        "applied_policy",
        "restart_required",
        "execution_authorized",
        "note",
        "code",
        "private",
        "runtime_id",
    ];
    if report.as_object().is_none_or(|v| {
        v.len() != FIELDS.len() || v.keys().any(|key| !FIELDS.contains(&key.as_str()))
    }) || report["schema_version"] != 1
        || report["report_type"] != "node_credentials"
        || report["success"] != true
        || report["changed"] != false
        || report["private"] != true
        || report["execution_authorized"] != false
        || report["runtime_id"] != runtime
        || report["note"] != CREDENTIAL_NOTE
        || !report["restart_required"].is_boolean()
        || report["saved_policy"] != *saved_summary
        || report["applied_policy"] != *applied_summary
        || !valid_summary(saved_summary)
        || !valid_summary(applied_summary)
        || uuid::Uuid::parse_str(runtime).map_or(true, |id| id.is_nil())
    {
        return false;
    }
    let Some(code) = report["code"].as_str() else {
        return false;
    };
    let request = eefn::credential_pairing::PairRequest {
        schema_version: 1,
        expected_node_id: node.into(),
        expected_runtime_id: runtime.into(),
        request_id: runtime.into(),
        code: code.into(),
    };
    if request.validate().is_err() {
        return false;
    }
    // PairRequest validates the strict bounded invitation, exact node, credential
    // and pinned certificate. Here additionally bind the caller's chosen endpoint
    // and the inspected saved trust, without printing a decoding error or secret.
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(code) else {
        return false;
    };
    let Ok(invitation) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Some(certificate) = invitation["coordinator_certificate"].as_str() else {
        return false;
    };
    let Ok(fingerprint) = eefn::tls::certificate_fingerprint(certificate) else {
        return false;
    };
    invitation["address"] == address
        && saved_summary["coordinator_fingerprint"] == fingerprint
        && saved_summary["identities"].as_array().is_some_and(|ids| {
            ids.iter()
                .any(|id| id["node_id"] == node && id["state"] == "provisioned")
        })
}

fn valid_summary(value: &Value) -> bool {
    eefn::authentication::valid_summary(value)
}

pub fn command(
    config: &crate::config::Config,
    applied: &AuthenticationPolicy,
    request: &CredentialCommand,
) -> Result<Value> {
    config.edit_for_restart(|value| {
        if !value["node"].is_object() { bail!("node settings must be an object") }
        let before = AuthenticationPolicy::parse(value.pointer("/node/authentication"))?;
        let mut saved = before.clone();
        let mut code = None;
        match request {
            CredentialCommand::Show => {},
            CredentialCommand::Legacy { allow } => saved.allow_legacy = *allow,
            CredentialCommand::Revoke { node } => {
                eefn::network::validate_remote_node_id(node)?;
                saved.credentials.insert(node.clone(), None);
            },
            CredentialCommand::Export { node, address } => {
                eefn::network::validate_remote_node_id(node)?;
                eefn::network::validate_address(address, true)?;
                let secret = saved.credentials.get(node).and_then(Option::as_ref)
                    .ok_or_else(|| anyhow::anyhow!("identity is not provisioned or is revoked; export never creates or restores credentials"))?;
                let identity = saved.tls.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("saved TLS identity unavailable; export never creates coordinator trust"))?;
                code = Some(invitation_code(node, address, secret, &identity.certificate)?);
            },
            CredentialCommand::Provision { node, address, rotate } => {
                eefn::network::validate_remote_node_id(node)?;
                eefn::network::validate_address(address, true)?;
                if saved.credentials.contains_key(node) && !rotate {
                    bail!("identity already provisioned or revoked; explicitly rotate to replace its credential")
                }
                let secret = eefn::authentication::generate_credential();
                if saved.tls.is_none() {
                    saved.tls = Some(eefn::tls::CoordinatorIdentity::generate()?);
                }
                code = Some(invitation_code(node, address, &secret,
                    &saved.tls.as_ref().expect("generated identity").certificate)?);
                saved.credentials.insert(node.clone(), Some(secret));
            },
        }
        saved.validate()?;
        if matches!(request, CredentialCommand::Legacy { allow: true }) {
            let secret = std::env::var("EEF_NODE_PSK").ok().filter(|s| !s.is_empty())
                .unwrap_or_else(|| value.pointer("/node/psk").and_then(Value::as_str).unwrap_or("").into());
            crate::runtime::validate_secret(&secret)?;
            saved.validate_legacy_key(&secret)?;
        }
        saved.validate_legacy_key(&config.string("node.psk", ""))?;
        let changed = saved != before;
        if changed { value["node"]["authentication"] = serde_json::to_value(&saved)?; }
        let mut report = json!({"schema_version":1,"report_type":"node_credentials","success":true,
            "changed":changed,"saved_policy":saved.summary(),"applied_policy":applied.summary(),
            "restart_required":saved != *applied,"execution_authorized":false,
            "note":CREDENTIAL_NOTE});
        if let Some(code) = code {
            report["code"] = json!(code);
            report["private"] = json!(true);
        }
        Ok((report, changed))
    })
}

fn invitation_code(node: &str, address: &str, secret: &str, certificate: &str) -> Result<String> {
    let invitation = json!({"schema_version":2,"node_id":node,"address":address,
        "auth_mode":"node_credential","psk":secret,"coordinator_certificate":certificate});
    Ok(base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&invitation)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_recovers_saved_invitation_without_rotation_or_writes() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        config.save_for_restart(&config.as_value()).unwrap();
        let applied = AuthenticationPolicy::default();
        let provision = command(
            &config,
            &applied,
            &CredentialCommand::Provision {
                node: "stable".into(),
                address: "127.0.0.1:51335".into(),
                rotate: false,
            },
        )
        .unwrap();
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        let export = CredentialCommand::Export {
            node: "stable".into(),
            address: "127.0.0.1:51335".into(),
        };
        let report = command(&config, &applied, &export).unwrap();
        assert_eq!(report["code"], provision["code"]);
        assert_eq!(report["private"], true);
        assert_eq!(report["changed"], false);
        assert_eq!(report["restart_required"], true);
        assert_eq!(report["execution_authorized"], false);
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        let latest = crate::config::Config::load(config.source()).unwrap();
        let saved = AuthenticationPolicy::parse(latest.get("node.authentication")).unwrap();
        let applied_report = command(&config, &saved, &export).unwrap();
        assert_eq!(applied_report["restart_required"], false);
        assert_eq!(applied_report["code"], report["code"]);
        let relocated = command(
            &config,
            &saved,
            &CredentialCommand::Export {
                node: "stable".into(),
                address: "192.0.2.10:51335".into(),
            },
        )
        .unwrap();
        let decode = |report: &Value| -> Value {
            serde_json::from_slice(
                &base64::engine::general_purpose::STANDARD
                    .decode(report["code"].as_str().unwrap())
                    .unwrap(),
            )
            .unwrap()
        };
        let original = decode(&report);
        let mut expected = original.clone();
        expected["address"] = json!("192.0.2.10:51335");
        assert_eq!(decode(&relocated), expected);
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        let show = command(&config, &saved, &CredentialCommand::Show).unwrap();
        assert!(show.get("code").is_none());
        assert!(!show.to_string().contains(original["psk"].as_str().unwrap()));
        for node in ["missing", "local", "*", " stable", "stable "] {
            assert!(
                command(
                    &config,
                    &saved,
                    &CredentialCommand::Export {
                        node: node.into(),
                        address: "127.0.0.1:51335".into(),
                    }
                )
                .is_err()
            );
            assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        }
        for address in ["", "0.0.0.0:51335", "127.0.0.1:0", "https://example.com"] {
            assert!(
                command(
                    &config,
                    &saved,
                    &CredentialCommand::Export {
                        node: "stable".into(),
                        address: address.into(),
                    }
                )
                .is_err()
            );
            assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        }
        command(
            &config,
            &saved,
            &CredentialCommand::Revoke {
                node: "stable".into(),
            },
        )
        .unwrap();
        let revoked = std::fs::read(config.source().unwrap()).unwrap();
        // Applied trust still accepts the old key, but export must honor saved revocation.
        assert!(command(&config, &saved, &export).is_err());
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), revoked);
    }

    #[test]
    fn export_never_bootstraps_missing_tls_or_accepts_extra_authority() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        let mut value = config.as_value();
        value["node"]["authentication"] =
            json!({"credentials":{"stable":eefn::authentication::generate_credential()}});
        config.save_for_restart(&value).unwrap();
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        let export = CredentialCommand::Export {
            node: "stable".into(),
            address: "127.0.0.1:51335".into(),
        };
        assert!(command(&config, &AuthenticationPolicy::default(), &export).is_err());
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        let good = serde_json::to_value(export).unwrap();
        assert!(serde_json::from_value::<CredentialCommand>(good.clone()).is_ok());
        for field in ["rotate", "allow", "permissions", "psk", "private_key"] {
            let mut bad = good.clone();
            bad[field] = json!(true);
            assert!(serde_json::from_value::<CredentialCommand>(bad).is_err());
        }
    }

    #[test]
    fn private_export_confirmation_binds_request_and_rejects_extra_fields() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        config.save_for_restart(&config.as_value()).unwrap();
        command(
            &config,
            &AuthenticationPolicy::default(),
            &CredentialCommand::Provision {
                node: "stable".into(),
                address: "127.0.0.1:51335".into(),
                rotate: false,
            },
        )
        .unwrap();
        let mut report = command(
            &config,
            &AuthenticationPolicy::default(),
            &CredentialCommand::Export {
                node: "stable".into(),
                address: "127.0.0.1:51335".into(),
            },
        )
        .unwrap();
        let runtime = uuid::Uuid::new_v4().to_string();
        report["runtime_id"] = json!(runtime);
        let saved = report["saved_policy"].clone();
        let applied = report["applied_policy"].clone();
        let valid = |v: &Value| {
            confirmed_export(v, &runtime, "stable", "127.0.0.1:51335", &saved, &applied)
        };
        assert!(valid(&report));
        assert!(!confirmed_export(
            &report,
            &runtime,
            "other",
            "127.0.0.1:51335",
            &saved,
            &applied
        ));
        assert!(!confirmed_export(
            &report,
            &runtime,
            "stable",
            "127.0.0.1:51336",
            &saved,
            &applied
        ));
        for (field, value) in [
            ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("changed", json!(true)),
            ("success", json!(false)),
            ("private", json!(false)),
            ("restart_required", json!("yes")),
            ("execution_authorized", json!(true)),
            ("code", json!("invalid")),
            ("note", report["code"].clone()),
            ("psk", json!("unexpected")),
            ("saved_policy", json!({})),
        ] {
            let mut bad = report.clone();
            bad[field] = value;
            assert!(!valid(&bad), "{field}");
        }
        let mut substituted = report.clone();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(report["code"].as_str().unwrap())
            .unwrap();
        let mut invitation: Value = serde_json::from_slice(&bytes).unwrap();
        invitation["coordinator_certificate"] = json!(
            eefn::tls::CoordinatorIdentity::generate()
                .unwrap()
                .certificate
        );
        substituted["code"] = json!(
            base64::engine::general_purpose::STANDARD
                .encode(serde_json::to_vec(&invitation).unwrap())
        );
        assert!(!valid(&substituted));
        for summary in [&saved, &applied] {
            let mut extra = summary.clone();
            extra["code"] = report["code"].clone();
            assert!(!valid_summary(&extra));
            let mut missing = summary.clone();
            missing
                .as_object_mut()
                .unwrap()
                .remove("coordinator_fingerprint");
            assert!(!valid_summary(&missing));
        }
        let mut duplicate = saved.clone();
        duplicate["identities"]
            .as_array_mut()
            .unwrap()
            .push(saved["identities"][0].clone());
        assert!(!valid_summary(&duplicate));
    }
    #[test]
    fn provisioning_is_private_exact_atomic_and_revocation_blocks_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::config::Config::load(Some(&dir.path().join("eef.yaml"))).unwrap();
        let mut initial = config.as_value();
        initial["node"]["psk"] = json!("legacy-network-secret");
        initial["custom"] = json!(42);
        config.save_for_restart(&initial).unwrap();
        let applied = AuthenticationPolicy::default();
        let provision = CredentialCommand::Provision {
            node: "a".into(),
            address: "127.0.0.1:51335".into(),
            rotate: false,
        };
        let report = command(&config, &applied, &provision).unwrap();
        assert_eq!(report["restart_required"], true);
        let invite: Value = serde_json::from_slice(
            &base64::engine::general_purpose::STANDARD
                .decode(report["code"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(invite["node_id"], "a");
        assert_eq!(invite["schema_version"], 2);
        assert!(
            eefn::tls::client_config(invite["coordinator_certificate"].as_str().unwrap()).is_ok()
        );
        assert!(invite.get("private_key").is_none());
        assert_ne!(invite["psk"], "legacy-network-secret");
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
        for node in ["local", "*", " a", "a ", "bad\0id"] {
            for invalid in [
                CredentialCommand::Provision {
                    node: node.into(),
                    address: "127.0.0.1:51335".into(),
                    rotate: false,
                },
                CredentialCommand::Revoke { node: node.into() },
            ] {
                assert!(command(&config, &applied, &invalid).is_err());
                assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
            }
        }
        assert!(command(&config, &applied, &provision).is_err());
        let show = command(&config, &applied, &CredentialCommand::Show).unwrap();
        assert!(!show.to_string().contains(invite["psk"].as_str().unwrap()));
        assert_eq!(std::fs::read(config.source().unwrap()).unwrap(), bytes);
        command(
            &config,
            &applied,
            &CredentialCommand::Revoke { node: "a".into() },
        )
        .unwrap();
        let latest = crate::config::Config::load(config.source()).unwrap();
        assert_eq!(latest.get("custom"), Some(&json!(42)));
        let policy = AuthenticationPolicy::parse(latest.get("node.authentication")).unwrap();
        assert_eq!(
            policy.tls.as_ref().unwrap().certificate,
            invite["coordinator_certificate"].as_str().unwrap()
        );
        assert!(
            !show
                .to_string()
                .contains(&policy.tls.as_ref().unwrap().private_key)
        );
        assert_eq!(policy.credentials.get("a"), Some(&None));
        assert!(command(&config, &applied, &provision).is_err());
        command(
            &config,
            &applied,
            &CredentialCommand::Legacy { allow: false },
        )
        .unwrap();
        assert_eq!(
            command(&config, &applied, &CredentialCommand::Show).unwrap()["saved_policy"]["allow_legacy"],
            false
        );
    }
}
