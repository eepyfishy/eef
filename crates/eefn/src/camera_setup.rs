//! Owner camera bindings only. No device probe, consent, capture or workload.
use crate::workload_leases::definition::camera_resource_digest;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NOTE: &str = "Camera configuration only. Enable camera permission and local listening separately, then review and approve work. No device, endpoint, model, workload, restart or retry was started. Apply saved bindings with an explicit node restart.";

#[derive(Clone, Debug, Default, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Select a local camera resource; create it if absent. Required when ambiguous.
    #[arg(long)]
    pub resource: Option<String>,
    /// Webcam index; otherwise preserve the current binding or use index zero.
    #[arg(long, conflicts_with="rtsp", value_parser=clap::value_parser!(u16).range(..=1023))]
    pub device: Option<u16>,
    /// Private RTSP/RTSPS URL, retained only in this node's saved configuration.
    #[arg(long, conflicts_with = "device")]
    pub rtsp: Option<String>,
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.device.is_none() || self.rtsp.is_none(),
            "Choose a webcam index or an RTSP URL"
        );
        let candidate = crate::context::Resource {
            id: "camera".into(),
            name: String::new(),
            capability: "camera.capture".into(),
            area: vec![],
            available: true,
            parameters: serde_json::from_value(
                json!({"device":self.device_value().unwrap_or(json!(0))}),
            )?,
            properties: Default::default(),
        };
        camera_resource_digest(&candidate)?;
        if let Some(id) = &self.resource {
            local_id("node", id, false)?;
        }
        Ok(())
    }
    fn device_value(&self) -> Option<Value> {
        self.rtsp
            .as_ref()
            .map(|s| json!(s))
            .or_else(|| self.device.map(|n| json!(n)))
    }
}
fn local_id<'a>(node: &str, id: &'a str, check_owner: bool) -> Result<&'a str> {
    let id = if let Some((owner, id)) = id.split_once("::") {
        crate::network::validate_remote_node_id(owner)?;
        ensure!(
            !check_owner || owner == node,
            "Camera setup cannot select another node's resource"
        );
        id
    } else {
        id
    };
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "Use a local resource ID, not a path"
    );
    Ok(id)
}
#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Check selected Python/OpenCV dependencies using a tiny in-memory image.
    Check(crate::camera_check::Settings),
    /// Inspect saved/applied camera bindings and permission; never probe a device.
    Show,
    /// Save webcam/RTSP bindings; permissions, consent and execution stay separate.
    Configure(Settings),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Configure {
        settings: Settings,
        expected_digest: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported camera configuration schema"
        );
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        if let Command::Configure {
            settings,
            expected_digest,
        } = &self.command
        {
            settings.validate()?;
            if let Some(id) = &settings.resource {
                local_id(&self.expected_node_id, id, true)?;
            }
            ensure!(
                crate::request_result::valid_digest(expected_digest),
                "Exact camera configuration digest required"
            );
        }
        Ok(())
    }
}
fn digest(config: &Value) -> Result<String> {
    crate::grants::definition_digest(&config.get("metadata").cloned().unwrap_or(json!({})))
}
fn apply(config: &mut Value, settings: &Settings) -> Result<()> {
    let node = config["node_id"]
        .as_str()
        .context("Node identity missing")?;
    let selected = settings
        .resource
        .as_deref()
        .map(|id| local_id(node, id, true))
        .transpose()?;
    let metadata: crate::context::NodeMetadata =
        serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
    metadata.validate()?;
    let matching: Vec<_> = metadata
        .resources
        .iter()
        .filter(|r| selected.map_or(r.capability == "camera.capture", |id| r.id == id))
        .collect();
    ensure!(
        matching.len() <= 1,
        "Multiple cameras: choose --resource explicitly"
    );
    let mut resource = if let Some(r) = matching.first() {
        ensure!(
            r.capability == "camera.capture" && r.available,
            "Selected resource is disabled or is not a camera"
        );
        camera_resource_digest(r)?;
        (*r).clone()
    } else {
        crate::context::Resource {
            id: selected
                .map(str::to_owned)
                .unwrap_or_else(|| format!("camera-{}", uuid::Uuid::new_v4().simple())),
            name: "Camera".into(),
            capability: "camera.capture".into(),
            area: vec![],
            available: true,
            parameters: Default::default(),
            properties: Default::default(),
        }
    };
    if let Some(value) = settings.device_value() {
        resource.parameters.insert("device".into(), value);
    }
    camera_resource_digest(&resource)?;
    // Preserve unknown metadata/resource fields rather than reserializing the container.
    let object = config
        .as_object_mut()
        .context("Node settings must be an object")?;
    let metadata = object
        .entry("metadata")
        .or_insert(json!({}))
        .as_object_mut()
        .context("Metadata must be an object")?;
    let resources = metadata
        .entry("resources")
        .or_insert(json!([]))
        .as_array_mut()
        .context("Resources must be an array")?;
    if let Some(current) = resources.iter_mut().find(|r| r["id"] == resource.id) {
        if let Some(value) = settings.device_value() {
            current
                .as_object_mut()
                .unwrap()
                .entry("parameters")
                .or_insert(json!({}))
                .as_object_mut()
                .context("Bindings must be an object")?
                .insert("device".into(), value);
        }
    } else {
        resources.push(json!(resource));
    }
    let metadata: crate::context::NodeMetadata = serde_json::from_value(json!(metadata))?;
    metadata.validate()?;
    Ok(())
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ResourceView {
    id: String,
    available: bool,
    binding_sha256: Option<String>,
}
fn resources(node: &str, values: &[crate::context::Resource]) -> Vec<ResourceView> {
    values
        .iter()
        .filter(|r| r.capability == "camera.capture")
        .map(|r| ResourceView {
            id: crate::context::resource_id(node, &r.id),
            available: r.available,
            binding_sha256: camera_resource_digest(r).ok(),
        })
        .collect()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    success: bool,
    request_sha256: String,
    saved_digest: String,
    changed: bool,
    restart_required: bool,
    saved_resources: Vec<ResourceView>,
    applied_resources: Vec<ResourceView>,
    saved_camera_permission: bool,
    applied_camera_permission: bool,
    capture_started: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && valid_report(&r)
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && crate::grants::definition_digest(&json!(request)).is_ok_and(|d| d == r.request_sha256)
        && (!matches!(request.command, Command::Show) || !r.changed)
}
fn valid_report(r: &Report) -> bool {
    r.schema_version == 1
        && r.report_type == "node_camera_setup"
        && r.success
        && crate::network::validate_remote_node_id(&r.node_id).is_ok()
        && crate::automation_occurrence::uuid(&r.runtime_id).is_ok()
        && crate::request_result::valid_digest(&r.request_sha256)
        && crate::request_result::valid_digest(&r.saved_digest)
        && r.saved_resources.len() <= 128
        && r.applied_resources.len() <= 128
        && [&r.saved_resources, &r.applied_resources]
            .into_iter()
            .all(|rows| {
                let mut ids = std::collections::BTreeSet::new();
                rows.iter().all(|row| {
                    local_id(&r.node_id, &row.id, true).is_ok()
                        && row.id.starts_with(&format!("{}::", r.node_id))
                        && ids.insert(&row.id)
                        && row
                            .binding_sha256
                            .as_ref()
                            .is_none_or(|s| crate::request_result::valid_digest(s))
                })
            })
        && (r.restart_required
            || (r.saved_resources == r.applied_resources
                && r.saved_camera_permission == r.applied_camera_permission))
        && !r.capture_started
        && !r.execution_authorized
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == NOTE
}
impl crate::NodeService {
    pub fn camera_setup(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Camera setup targets another node"
        );
        self.with_permission_runtime(&request.expected_runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                let mut live = self.live.lock().unwrap();
                ensure!(
                    config["node_id"] == self.node_id
                        && live["runtime_id"] == request.expected_runtime_id,
                    "Camera setup runtime changed"
                );
                let mut next = config.clone();
                if let Command::Configure {
                    settings,
                    expected_digest,
                } = &request.command
                {
                    ensure!(
                        !remote || config.pointer("/management/allow_remote") == Some(&json!(true)),
                        "Current local owner approval required for remote camera configuration"
                    );
                    ensure!(
                        digest(config)? == *expected_digest,
                        "Camera configuration changed; inspect before deciding what to do"
                    );
                    apply(&mut next, settings)?;
                }
                let changed = digest(&next)? != digest(config)?;
                if changed {
                    self.save_config_locked(next.clone())?;
                    live["pending_restart"] = json!(true);
                }
                let metadata: crate::context::NodeMetadata =
                    serde_json::from_value(next.get("metadata").cloned().unwrap_or(json!({})))?;
                metadata.validate()?;
                let saved_resources = resources(&self.node_id, &metadata.resources);
                let applied_resources = resources(&self.node_id, engine.applied_resources());
                let saved_camera_permission =
                    next.pointer("/permissions/media/camera") == Some(&json!(true));
                let applied_camera_permission = engine.applied_permissions().media.camera;
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_camera_setup".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    success: true,
                    request_sha256: crate::grants::definition_digest(&json!(request))?,
                    saved_digest: digest(&next)?,
                    changed,
                    restart_required: live["pending_restart"] == true
                        || saved_resources != applied_resources
                        || saved_camera_permission != applied_camera_permission,
                    saved_resources,
                    applied_resources,
                    saved_camera_permission,
                    applied_camera_permission,
                    capture_started: false,
                    execution_authorized: false,
                    restart_requested: false,
                    automatically_retried: false,
                    note: NOTE.into()
                });
                ensure!(
                    confirmed(&report, &request),
                    "Camera setup outcome unconfirmed; inspect current state"
                );
                Ok(report)
            })
        })
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    ensure!(valid_report(&r), "Unconfirmed camera setup report");
    let mut text = format!(
        "Node {} camera setup\nCamera permission (saved/applied): {}/{}\nSaved settings need restart: {}\n",
        r.node_id, r.saved_camera_permission, r.applied_camera_permission, r.restart_required
    );
    for row in r.saved_resources {
        text.push_str(&format!(
            "{}: enabled={}, supported binding={}\n",
            row.id,
            row.available,
            row.binding_sha256.is_some()
        ));
    }
    text.push_str(NOTE);
    text.push('\n');
    Ok(text)
}
#[cfg(test)]
#[path = "camera_setup_tests.rs"]
mod tests;
