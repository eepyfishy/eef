//! Feature-wide local-owner preferences, never grants or administrator access.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const NOTE: &str = "Permission preferences only, not workload approval, installed capability health or administrator access. Save does not restart, execute or stop already-running work. Inspect after uncertainty; no automatic retry or replay.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    FilesystemRead,
    FilesystemWrite,
    ProcessExecution,
    Applications,
    HttpRequests,
    SpeechToText,
    WakeOnLan,
    Microphone,
    AudioOutput,
    TextToSpeech,
    Camera,
    ScreenCapture,
    InputControl,
    PeerTransfers,
    RemoteUpdates,
    RemoteManagement,
}
impl Feature {
    pub const ALL: [Self; 16] = [
        Self::FilesystemRead,
        Self::FilesystemWrite,
        Self::ProcessExecution,
        Self::Applications,
        Self::HttpRequests,
        Self::SpeechToText,
        Self::WakeOnLan,
        Self::Microphone,
        Self::AudioOutput,
        Self::TextToSpeech,
        Self::Camera,
        Self::ScreenCapture,
        Self::InputControl,
        Self::PeerTransfers,
        Self::RemoteUpdates,
        Self::RemoteManagement,
    ];
    fn field(self) -> &'static str {
        match self {
            Self::FilesystemRead => "/permissions/filesystem/read",
            Self::FilesystemWrite => "/permissions/filesystem/write",
            Self::ProcessExecution => "/permissions/process/enabled",
            Self::Applications => "/permissions/applications/enabled",
            Self::HttpRequests => "/permissions/http/enabled",
            Self::SpeechToText => "/permissions/stt/enabled",
            Self::WakeOnLan => "/permissions/wake_on_lan/enabled",
            Self::Microphone => "/permissions/media/microphone",
            Self::AudioOutput => "/permissions/media/audio_output",
            Self::TextToSpeech => "/permissions/media/tts",
            Self::Camera => "/permissions/media/camera",
            Self::ScreenCapture => "/permissions/media/screen_capture",
            Self::InputControl => "/permissions/media/input_control",
            Self::PeerTransfers => "/permissions/peer_transfers/enabled",
            Self::RemoteUpdates => "/permissions/remote_updates",
            Self::RemoteManagement => "/management/allow_remote",
        }
    }
    fn unrestricted(self) -> Option<&'static str> {
        match self {
            Self::FilesystemRead => Some("filesystem.read"),
            Self::FilesystemWrite => Some("filesystem.write"),
            Self::ProcessExecution => Some("process.exec"),
            Self::Applications => Some("application.control"),
            Self::HttpRequests => Some("http.request"),
            Self::WakeOnLan => Some("network.wol"),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::FilesystemRead => "Read files",
            Self::FilesystemWrite => "Modify files",
            Self::ProcessExecution => "Run programs",
            Self::Applications => "Control apps",
            Self::HttpRequests => "HTTP / network requests",
            Self::SpeechToText => "Speech recognition",
            Self::WakeOnLan => "Wake other computers",
            Self::Microphone => "Microphone",
            Self::AudioOutput => "Audio output",
            Self::TextToSpeech => "Voice output",
            Self::Camera => "Camera",
            Self::ScreenCapture => "Screen capture",
            Self::InputControl => "Keyboard / mouse",
            Self::PeerTransfers => "Approved peer transfers",
            Self::RemoteUpdates => "Remote updates",
            Self::RemoteManagement => "Management from EEF",
        }
    }
}
#[derive(Debug, clap::Subcommand)]
pub enum PermissionAction {
    /// Inspect saved/applied feature permission modes; no files/apps are accessed.
    Show {},
    /// Explicitly allow the feature's supported actions with this account's access.
    Allow {
        #[arg(value_enum)]
        feature: Feature,
    },
    /// Disable the whole feature, preserving old scoped settings for compatibility.
    Deny {
        #[arg(value_enum)]
        feature: Feature,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionCommand {
    Show {},
    Set {
        feature: Feature,
        enabled: bool,
        expected_digest: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: PermissionCommand,
}
impl PermissionRequest {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported permission command schema")
        }
        if let PermissionCommand::Set {
            expected_digest, ..
        } = &self.command
        {
            if !valid_digest(expected_digest) {
                bail!("Exact saved permission digest is required")
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Disabled,
    Scoped,
    FeatureWide,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureState {
    feature: Feature,
    saved: Mode,
    applied: Mode,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    command: PermissionCommand,
    success: bool,
    saved_digest: String,
    features: Vec<FeatureState>,
    changed: bool,
    restart_required: bool,
    remote_management_allowed: bool,
    execution_authorized: bool,
    restart_requested: bool,
    model_inference_requested: bool,
    automatically_retried: bool,
    note: String,
}
fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(config: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!({
            "permissions":config.get("permissions").cloned().unwrap_or(json!({})),
            "remote_management":config.pointer("/management/allow_remote").and_then(Value::as_bool).unwrap_or(false),
        }))?)
    ))
}
fn mode(config: &Value, feature: Feature) -> Mode {
    if config.pointer(feature.field()).and_then(Value::as_bool) != Some(true) {
        return Mode::Disabled;
    }
    if feature.unrestricted().is_none_or(|token| {
        config
            .pointer("/permissions/full_access")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(token)))
    }) {
        Mode::FeatureWide
    } else {
        Mode::Scoped
    }
}
fn set_field(config: &mut Value, pointer: &str, enabled: bool) -> Result<()> {
    let mut parts = pointer.trim_start_matches('/').split('/').peekable();
    let mut cursor = config;
    while let Some(part) = parts.next() {
        let object = cursor
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("Permission settings must be objects"))?;
        if parts.peek().is_none() {
            object.insert(part.into(), json!(enabled));
            return Ok(());
        }
        cursor = object.entry(part).or_insert(json!({}));
    }
    bail!("Invalid permission feature field")
}
fn apply(config: &mut Value, feature: Feature, enabled: bool) -> Result<bool> {
    let before = config.clone();
    set_field(config, feature.field(), enabled)?;
    if let Some(token) = feature.unrestricted() {
        let permissions = config["permissions"]
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("Permissions must be an object"))?;
        let grants = permissions
            .entry("full_access")
            .or_insert(json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Feature grants must be an array"))?;
        if grants.iter().any(|v| !v.is_string()) {
            bail!("Feature grants must be strings")
        }
        if enabled {
            if !grants.iter().any(|v| v.as_str() == Some(token)) {
                grants.push(json!(token));
            }
        } else {
            grants.retain(|v| v.as_str() != Some(token));
        }
    }
    // Reuse executor validation. Keep original JSON/unknown future settings and
    // old scopes intact; this does not save normalized/clamped policy wholesale.
    let policy: crate::NodePolicy =
        serde_json::from_value(config.get("permissions").cloned().unwrap_or(json!({})))?;
    policy.validate()?;
    Ok(*config != before)
}
pub fn confirmed(value: &Value, request: &PermissionRequest) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || report.schema_version != 1
        || report.report_type != "node_permissions"
        || !report.success
        || report.node_id != request.expected_node_id
        || report.runtime_id != request.expected_runtime_id
        || report.command != request.command
        || !valid_digest(&report.saved_digest)
        || report.features.len() != Feature::ALL.len()
        || report
            .features
            .iter()
            .zip(Feature::ALL)
            .any(|(row, feature)| {
                row.feature != feature
                    || row.feature.unrestricted().is_none()
                        && (row.saved == Mode::Scoped || row.applied == Mode::Scoped)
            })
        || report.execution_authorized
        || report.restart_requested
        || report.model_inference_requested
        || report.automatically_retried
        || report.note != NOTE
    {
        return false;
    }
    let management = report
        .features
        .iter()
        .find(|row| row.feature == Feature::RemoteManagement)
        .unwrap();
    if management.saved != management.applied
        || report.remote_management_allowed != (management.saved == Mode::FeatureWide)
        || report.features.iter().any(|row| row.saved != row.applied) && !report.restart_required
    {
        return false;
    }
    match &request.command {
        PermissionCommand::Show {} => !report.changed,
        PermissionCommand::Set {
            feature,
            enabled,
            expected_digest,
        } => {
            let row = report
                .features
                .iter()
                .find(|row| row.feature == *feature)
                .unwrap();
            row.saved
                == (if *enabled {
                    Mode::FeatureWide
                } else {
                    Mode::Disabled
                })
                && report.changed == (report.saved_digest != *expected_digest)
        }
    }
}

impl crate::NodeService {
    pub fn permission_command(&self, request: PermissionRequest, remote: bool) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Permission command targets another node")
        }
        self.with_permission_runtime(&request.expected_runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                let _: crate::NodePolicy = serde_json::from_value(
                    config.get("permissions").cloned().unwrap_or(json!({})),
                )?;
                if config
                    .get("management")
                    .is_some_and(|value| !value.is_object())
                    || config
                        .pointer("/management/allow_remote")
                        .is_some_and(|value| !value.is_boolean())
                {
                    bail!("Management preferences must be an object with a boolean approval flag")
                }
                // Lock order: existing preview binding -> config -> live. Main
                // runtime publication/drop must release/wait for these guards.
                let mut live = self.live.lock().unwrap();
                if live["runtime_id"] != request.expected_runtime_id
                    || config["node_id"] != self.node_id
                {
                    bail!("Permission runtime/identity changed before save")
                }
                if remote
                    && matches!(request.command, PermissionCommand::Set { .. })
                    && config.pointer("/management/allow_remote") != Some(&json!(true))
                {
                    bail!("Current local owner approval required for remote permission changes")
                }
                let mut next = config.clone();
                let mut changed = false;
                if let PermissionCommand::Set {
                    feature,
                    enabled,
                    expected_digest,
                } = &request.command
                {
                    if digest(config)? != *expected_digest {
                        bail!("Saved permissions changed; inspect before deciding what to do")
                    }
                    changed = apply(&mut next, *feature, *enabled)?;
                    if changed {
                        self.save_config_locked(next.clone())?;
                        if *feature != Feature::RemoteManagement {
                            live["pending_restart"] = json!(true);
                        }
                    }
                }
                let mut applied = json!({"permissions":engine.applied_permissions()});
                set_field(
                    &mut applied,
                    Feature::RemoteManagement.field(),
                    next.pointer(Feature::RemoteManagement.field())
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                )?;
                let features = Feature::ALL
                    .into_iter()
                    .map(|feature| FeatureState {
                        feature,
                        saved: mode(&next, feature),
                        applied: mode(&applied, feature),
                    })
                    .collect::<Vec<_>>();
                let restart = live["pending_restart"].as_bool().unwrap_or(false)
                    || features.iter().any(|row| row.saved != row.applied);
                let report = serde_json::to_value(Report {
                    schema_version: 1,
                    report_type: "node_permissions".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    success: true,
                    saved_digest: digest(&next)?,
                    features,
                    changed,
                    restart_required: restart,
                    remote_management_allowed: next
                        .pointer("/management/allow_remote")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    execution_authorized: false,
                    restart_requested: false,
                    model_inference_requested: false,
                    automatically_retried: false,
                    note: NOTE.into(),
                })?;
                if !confirmed(&report, &request) {
                    bail!("Permission save reply unconfirmed; inspect current state")
                }
                Ok(report)
            })
        })
    }
}

pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    let request = PermissionRequest {
        schema_version: 1,
        expected_node_id: report.node_id.clone(),
        expected_runtime_id: report.runtime_id.clone(),
        command: report.command.clone(),
    };
    if !confirmed(value, &request) {
        bail!("Unconfirmed permission report")
    }
    let label = |mode| match mode {
        Mode::Disabled => "off",
        Mode::Scoped => "limited by previous settings",
        Mode::FeatureWide => "whole feature allowed",
    };
    let mut text = format!("Node permissions: {}\n", report.node_id);
    for row in report.features {
        text.push_str(&format!(
            "{}: saved {}; applied {}\n",
            row.feature.label(),
            label(row.saved),
            label(row.applied)
        ));
    }
    if report.restart_required {
        text.push_str("Apply saved preferences with an explicit node restart. Saved changes gate affected operations; already-running effects are not guaranteed stopped or undone.\n");
    }
    text.push_str("Permissions do not approve tasks or grant administrator rights. Running programs or controlling input/apps can affect files independently of file permissions.\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn fixture() -> (
        tempfile::TempDir,
        Arc<crate::NodeService>,
        Arc<crate::NodeEngine>,
        crate::request_preview::PreviewGuard,
        PermissionRequest,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::setup::defaults();
        config["node_id"] = json!("owner");
        config["name"] = json!("PRIVATE-NAME");
        config["psk"] = json!("PRIVATE");
        config["custom"] = json!({"keep":true});
        let path = dir.path().join("node.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let core = crate::NodeService::new(path, "owner".into());
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(core.clone()),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        core.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let guard = core.attach_preview_runtime(&engine, runtime.clone());
        let request = PermissionRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: runtime,
            command: PermissionCommand::Show {},
        };
        (dir, core, engine, guard, request)
    }
    fn set(
        request: &PermissionRequest,
        inspected: &Value,
        feature: Feature,
        enabled: bool,
    ) -> PermissionRequest {
        PermissionRequest {
            command: PermissionCommand::Set {
                feature,
                enabled,
                expected_digest: inspected["saved_digest"].as_str().unwrap().into(),
            },
            ..request.clone()
        }
    }
    #[test]
    fn each_feature_uses_its_existing_executor_preference_without_per_object_allowlists() {
        let mut config = json!({"permissions":{"stt":{"endpoint":"http://127.0.0.1:1","model":"owner-choice"},"full_access":["future.feature"],"future":{"keep":true}},"custom":42});
        for feature in Feature::ALL {
            assert!(apply(&mut config, feature, true).unwrap());
            assert_eq!(mode(&config, feature), Mode::FeatureWide);
            assert!(!apply(&mut config, feature, true).unwrap());
            assert!(apply(&mut config, feature, false).unwrap());
            assert_eq!(mode(&config, feature), Mode::Disabled);
        }
        assert_eq!(
            config["permissions"]["full_access"],
            json!(["future.feature"])
        );
        assert_eq!(config["permissions"]["future"]["keep"], true);
        assert_eq!(config["custom"], 42);
        assert!(config.pointer("/permissions/filesystem/roots").is_none());
        assert!(
            config
                .pointer("/permissions/process/allowed_executables")
                .is_none()
        );
        assert!(
            config
                .pointer("/permissions/applications/allowed")
                .is_none()
        );
        assert!(config.pointer("/permissions/http/allowed_hosts").is_none());
    }
    #[test]
    fn full_feature_toggle_preserves_scopes_limits_and_unrelated_configuration() {
        let (dir, core, _engine, _guard, request) = fixture();
        let mut config = core.read_config().unwrap();
        config["permissions"]["filesystem"]["roots"] = json!([dir.path()]);
        config["permissions"]["filesystem"]["max_write_bytes"] = json!(8192);
        config["permissions"]["future"] = json!({"PRIVATE":true});
        core.save_configuration(config.clone()).unwrap();
        let show = core.permission_command(request.clone(), false).unwrap();
        assert!(confirmed(&show, &request));
        assert!(!show.to_string().contains("PRIVATE"));
        let allowed = core
            .permission_command(set(&request, &show, Feature::FilesystemWrite, true), false)
            .unwrap();
        let row = &allowed["features"][1];
        assert_eq!(row["saved"], "feature_wide");
        assert_eq!(row["applied"], "disabled");
        assert_eq!(allowed["restart_required"], true);
        let saved = core.read_config().unwrap();
        for field in ["node_id", "name", "psk", "models", "custom"] {
            assert_eq!(saved[field], config[field]);
        }
        assert_eq!(
            saved["permissions"]["filesystem"]["roots"],
            config["permissions"]["filesystem"]["roots"]
        );
        assert_eq!(saved["permissions"]["filesystem"]["max_write_bytes"], 8192);
        assert_eq!(
            saved["permissions"]["future"],
            config["permissions"]["future"]
        );
        let denied = core
            .permission_command(
                set(&request, &allowed, Feature::FilesystemWrite, false),
                false,
            )
            .unwrap();
        assert_eq!(denied["features"][1]["saved"], "disabled");
        assert_eq!(denied["restart_requested"], false);
        assert_eq!(
            core.read_config().unwrap()["permissions"]["filesystem"]["roots"],
            config["permissions"]["filesystem"]["roots"]
        );
    }
    #[test]
    fn stale_digest_runtime_target_and_retired_binding_cannot_save() {
        let (_dir, core, _engine, guard, request) = fixture();
        let inspected = core.permission_command(request.clone(), false).unwrap();
        let mutation = set(&request, &inspected, Feature::FilesystemRead, true);
        core.permission_command(set(&request, &inspected, Feature::Camera, true), false)
            .unwrap();
        let before = core.read_config().unwrap();
        assert!(core.permission_command(mutation.clone(), false).is_err());
        assert_eq!(core.read_config().unwrap(), before);
        assert!(
            core.permission_command(
                PermissionRequest {
                    expected_node_id: "other".into(),
                    ..mutation.clone()
                },
                false
            )
            .is_err()
        );
        assert!(
            core.permission_command(
                PermissionRequest {
                    expected_runtime_id: uuid::Uuid::new_v4().to_string(),
                    ..mutation.clone()
                },
                false
            )
            .is_err()
        );
        drop(guard);
        assert!(core.permission_command(request, false).is_err());
        assert_eq!(core.read_config().unwrap(), before);
    }
    #[test]
    fn remote_changes_recheck_current_approval_and_cannot_reenable_revoked_management() {
        let (_dir, core, _engine, _guard, request) = fixture();
        let show = core.permission_command(request.clone(), true).unwrap();
        let before = core.read_config().unwrap();
        assert!(
            core.permission_command(set(&request, &show, Feature::RemoteManagement, true), true)
                .is_err()
        );
        assert_eq!(core.read_config().unwrap(), before);
        let enabled = core
            .permission_command(set(&request, &show, Feature::RemoteManagement, true), false)
            .unwrap();
        assert_eq!(enabled["remote_management_allowed"], true);
        assert_eq!(
            enabled["features"][14]["saved"],
            enabled["features"][14]["applied"]
        );
        assert_eq!(enabled["restart_required"], false);
        let allowed = core
            .permission_command(set(&request, &enabled, Feature::FilesystemRead, true), true)
            .unwrap();
        let revoked = core
            .permission_command(
                set(&request, &allowed, Feature::RemoteManagement, false),
                false,
            )
            .unwrap();
        let before = core.read_config().unwrap();
        assert!(
            core.permission_command(
                set(&request, &revoked, Feature::RemoteManagement, true),
                true
            )
            .is_err()
        );
        assert_eq!(core.read_config().unwrap(), before);
    }
    #[test]
    fn strict_wire_reports_and_configuration_failure_never_imply_side_effects_or_scope_widening() {
        let (_dir, core, _engine, _guard, request) = fixture();
        let value = core.permission_command(request.clone(), false).unwrap();
        for (field, invalid) in [
            ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("changed", json!(true)),
            ("execution_authorized", json!(true)),
            ("restart_requested", json!(true)),
            ("note", json!("PRIVATE-PROSE")),
        ] {
            let mut bad = value.clone();
            bad[field] = invalid;
            assert!(!confirmed(&bad, &request));
        }
        let mut bad = value.clone();
        bad["features"][0]["feature"] = json!("camera");
        assert!(!confirmed(&bad, &request));
        let mut bad = value.clone();
        bad["features"][7]["saved"] = json!("scoped");
        assert!(!confirmed(&bad, &request));
        for raw in [
            json!({"operation":"set","feature":"camera","expected_digest":"a".repeat(64)}),
            json!({"operation":"show","enabled":true}),
            json!({"operation":"set","feature":"bluetooth","enabled":true,"expected_digest":"a".repeat(64)}),
        ] {
            assert!(serde_json::from_value::<PermissionCommand>(raw).is_err());
        }
        let before = core.read_config().unwrap();
        assert!(
            core.permission_command(set(&request, &value, Feature::SpeechToText, true), false)
                .is_err()
        );
        assert_eq!(core.read_config().unwrap(), before);
    }
    #[tokio::test]
    async fn reserved_status_action_cannot_mutate_through_the_control_lane() {
        let (_dir, core, _engine, _guard, request) = fixture();
        let inspected = core.permission_command(request.clone(), true).unwrap();
        let mutation = set(&request, &inspected, Feature::FilesystemRead, true);
        let before = core.read_config().unwrap();
        assert!(
            core.remote("permission_status", serde_json::to_value(mutation).unwrap())
                .await
                .is_err()
        );
        assert_eq!(core.read_config().unwrap(), before);
    }
    #[test]
    fn dormant_or_bypassed_scopes_are_preserved_without_requiring_missing_paths() {
        let missing = tempfile::tempdir()
            .unwrap()
            .path()
            .join("removed-owner-scope");
        let mut config = json!({"permissions":{"filesystem":{"roots":[missing]},
            "process":{"allowed_executables":[missing]},"applications":{"allowed":[missing]}}});
        let original = config.clone();
        for feature in [
            Feature::FilesystemRead,
            Feature::FilesystemWrite,
            Feature::ProcessExecution,
            Feature::Applications,
        ] {
            assert!(apply(&mut config, feature, true).unwrap());
        }
        assert_eq!(
            config["permissions"]["filesystem"]["roots"],
            original["permissions"]["filesystem"]["roots"]
        );
        assert_eq!(
            config["permissions"]["process"]["allowed_executables"],
            original["permissions"]["process"]["allowed_executables"]
        );
        assert_eq!(
            config["permissions"]["applications"]["allowed"],
            original["permissions"]["applications"]["allowed"]
        );
        let mut scoped = config.clone();
        scoped["permissions"]["full_access"] = json!([]);
        let policy: crate::NodePolicy =
            serde_json::from_value(scoped["permissions"].clone()).unwrap();
        assert!(policy.validate().is_err());
        for feature in [
            Feature::FilesystemRead,
            Feature::FilesystemWrite,
            Feature::ProcessExecution,
            Feature::Applications,
        ] {
            assert!(apply(&mut config, feature, false).unwrap());
        }
        assert_eq!(
            config["permissions"]["filesystem"]["roots"],
            original["permissions"]["filesystem"]["roots"]
        );
    }
}
