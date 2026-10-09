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
                eefn::network::validate_node_id(node)?;
                saved.credentials.insert(node.clone(), None);
            },
            CredentialCommand::Provision { node, address, rotate } => {
                eefn::network::validate_node_id(node)?;
                eefn::network::validate_address(address, true)?;
                if saved.credentials.contains_key(node) && !rotate {
                    bail!("identity already provisioned or revoked; explicitly rotate to replace its credential")
                }
                let secret = eefn::authentication::generate_credential();
                let invitation = json!({"schema_version":1,"node_id":node,"address":address,
                    "auth_mode":"node_credential","psk":secret});
                saved.credentials.insert(node.clone(), Some(secret));
                code = Some(base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&invitation)?));
            },
        }
        saved.validate()?;
        saved.validate_legacy_key(&config.string("node.psk", ""))?;
        let changed = saved != before;
        if changed { value["node"]["authentication"] = serde_json::to_value(&saved)?; }
        let mut report = json!({"schema_version":1,"report_type":"node_credentials","success":true,
            "changed":changed,"saved_policy":saved.summary(),"applied_policy":applied.summary(),
            "restart_required":saved != *applied,"execution_authorized":false,
            "note":"Credential changes apply after explicit EEF restart. Existing sessions are not revoked until restart. Pairing never grants node feature permissions; no automatic replay was sent."});
        if let Some(code) = code {
            report["code"] = json!(code);
            report["private"] = json!(true);
        }
        Ok((report, changed))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_ne!(invite["psk"], "legacy-network-secret");
        let bytes = std::fs::read(config.source().unwrap()).unwrap();
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
