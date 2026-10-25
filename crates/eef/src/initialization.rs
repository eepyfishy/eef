//! Offline local-owner first creation. Not trust migration or runtime health.
use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

pub fn command(path: &Path) -> Result<Value> {
    let external = std::env::var("EEF_NODE_PSK").ok().filter(|s| !s.is_empty());
    initialize_with_external(path, external.as_deref())
}

fn initialize_with_external(path: &Path, external: Option<&str>) -> Result<Value> {
    let Some(_guard) = eefn::setup::instance_lock(path)? else {
        return Ok(
            json!({"schema_version":1,"report_type":"coordinator_initialization","success":false,
            "state":"coordinator_running","configuration_created":false,"existing_configuration_preserved":true,
            "network_started":false,"enrollment_requested":false,"restart_requested":false,"runtime_verified":false,
            "note":"Stop this coordinator before offline initialization. No configuration or running service was changed."}),
        );
    };
    let state = eefn::coordinator_config::initialize(path)?;
    let config = crate::config::Config::load(Some(path)).map_err(|_| {
        anyhow::anyhow!(
            "Coordinator settings failed validation; no existing configuration was replaced"
        )
    })?;
    let policy = eefn::authentication::AuthenticationPolicy::parse(
        config.get("node.authentication"),
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "Coordinator authentication settings are invalid; existing settings were not replaced"
        )
    })?;
    let saved = config.string("node.psk", "");
    let valid =
        !policy.allow_legacy || crate::runtime::validate_secret(external.unwrap_or(&saved)).is_ok();
    let created = state == eefn::coordinator_config::Initialization::Created;
    Ok(
        json!({"schema_version":1,"report_type":"coordinator_initialization","success":valid,
        "state":if created {"created"} else {"existing"},"configuration_created":created,
        "existing_configuration_preserved":!created,"configuration_validated":true,
        "legacy_secret_requirement_satisfied":valid,"external_secret_available":external.is_some(),
        "existing_authentication_changed":false,"permissions_changed":false,"models_changed":false,
        "model_download_started":false,"network_started":false,"enrollment_requested":false,
        "restart_requested":false,"runtime_verified":false,
        "note":if valid {"Saved coordinator settings are initialized. Start EEF normally. No network, enrollment, feature grant, model download or restart was requested; runtime health is not verified."}
            else {"Existing legacy credentials are not configured for startup. Existing settings were preserved; initialization does not rotate or repair them. Explicit owner configuration is required."}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_initialization_is_persistent_nonsecret_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/identity.yaml");
        let report = initialize_with_external(&path, None).unwrap();
        assert_eq!(report["success"], true);
        assert_eq!(report["configuration_created"], true);
        let bytes = std::fs::read(&path).unwrap();
        let value: Value = serde_yml::from_slice(&bytes).unwrap();
        let secret = value["node"]["psk"].as_str().unwrap();
        assert!(!report.to_string().contains(secret));
        let second = initialize_with_external(&path, None).unwrap();
        assert_eq!(second["existing_configuration_preserved"], true);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        for key in [
            "network_started",
            "enrollment_requested",
            "restart_requested",
            "runtime_verified",
            "models_changed",
            "permissions_changed",
        ] {
            assert_eq!(report[key], false);
        }
    }
    #[test]
    fn existing_invalid_or_external_credentials_are_not_rotated_or_disclosed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.yaml");
        let before = b"# User-owned\nnode:\n  psk: change-me-eef\n";
        std::fs::write(&path, before).unwrap();
        let refused = initialize_with_external(&path, None).unwrap();
        assert_eq!(refused["success"], false);
        let external = "external-private-fixture-credential";
        let valid = initialize_with_external(&path, Some(external)).unwrap();
        assert_eq!(valid["success"], true);
        assert!(!valid.to_string().contains(external));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::write(&path, b"PRIVATE_invalid: [ unclosed").unwrap();
        let error = initialize_with_external(&path, None).unwrap_err();
        assert!(!format!("{error:#}").contains("PRIVATE_invalid"));
    }
    #[test]
    fn running_coordinator_blocks_offline_creation_and_existing_inspection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.yaml");
        let _running = eefn::setup::instance_lock(&path).unwrap().unwrap();
        let report = initialize_with_external(&path, None).unwrap();
        assert_eq!(report["success"], false);
        assert_eq!(report["state"], "coordinator_running");
        assert!(!path.exists());
    }
}
