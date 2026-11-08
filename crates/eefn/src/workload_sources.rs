//! Node-owned source snapshots for review, never capture or execution authority.
use crate::workload_leases::definition::microphone_resource_digest;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// The reviewed built-in source. Future execution must use these exact bytes,
/// rather than reopening a path whose contents may change after inspection.
pub(crate) const MICROPHONE_SOURCE: &[u8] =
    include_bytes!("../../../python/streams/microphone_energy.py");
pub(crate) const CAMERA_SOURCE: &[u8] = include_bytes!("../../../python/streams/camera_motion.py");
pub fn camera_adapter_digest() -> String {
    hex::encode(Sha256::digest(CAMERA_SOURCE))
}
pub fn microphone_adapter_digest() -> String {
    hex::encode(Sha256::digest(MICROPHONE_SOURCE))
}
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Inspect configured microphone resources without opening a device.
    Sources,
    /// Inspect configured camera resources without opening a device.
    CameraSources,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        ensure!(
            self.schema_version == 2,
            "Unsupported source inspection schema"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub resource_id: String,
    pub name: String,
    pub area: crate::context::AreaPath,
    pub available: bool,
    /// Includes private binding data, which is never returned in this view.
    pub resource_sha256: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub node_id: String,
    pub runtime_id: String,
    pub source_kind: String,
    pub adapter_sha256: String,
    pub resources: Vec<Resource>,
    pub local_bindings_current: bool,
    pub listening_enabled: bool,
    pub listening_revision: u64,
    pub listening_epoch: u64,
    pub listening_blocked: bool,
    pub saved_microphone_permission: bool,
    pub applied_microphone_permission: bool,
    /// Configured interpreter only; no device import/probe or hardware claim.
    pub python_configured: bool,
    pub capture_started: bool,
    pub execution_authorized: bool,
    pub execution_available: bool,
}
pub(crate) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn validate_resources(node: &str, resources: &[Resource]) -> Result<()> {
    let prefix = format!("{node}::");
    let mut ids = HashSet::new();
    for row in resources {
        let local = row.resource_id.strip_prefix(&prefix).unwrap_or("");
        ensure!(
            !local.is_empty()
                && local.len() <= 100
                && local
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && ids.insert(&row.resource_id)
                && row.resource_sha256.as_deref().is_none_or(digest),
            "Invalid source resource snapshot"
        );
        crate::context::validate_area(&row.area)?;
    }
    Ok(())
}
impl Report {
    pub fn validate(&self, request: &Request) -> Result<()> {
        request.validate()?;
        ensure!(
            self.success
                && matches!(
                    (self.schema_version, self.execution_available),
                    (2, false) | (3, true)
                )
                && self.report_type == "workload_sources"
                && self.node_id == request.expected_node_id
                && self.runtime_id == request.expected_runtime_id
                && self.source_kind == "microphone_energy"
                && digest(&self.adapter_sha256)
                && !self.capture_started
                && !self.execution_authorized
                && self.listening_revision <= i64::MAX as u64
                && self.resources.len() <= 128
                && serde_json::to_vec(self)?.len() <= 256 * 1024,
            "Unconfirmed source snapshot or unsupported execution claim"
        );
        if self.execution_available {
            ensure!(
                self.local_bindings_current
                    && self.listening_enabled
                    && !self.listening_blocked
                    && self.saved_microphone_permission
                    && self.applied_microphone_permission
                    && self.python_configured
                    && self
                        .resources
                        .iter()
                        .any(|r| r.available && r.resource_sha256.is_some()),
                "Executable source snapshot lacks current local readiness"
            );
        }
        validate_resources(&self.node_id, &self.resources)
    }
    pub fn confirms_template(
        &self,
        template: &crate::workload_leases::definition::Template,
    ) -> Result<()> {
        template.validate()?;
        self.validate(&Request {
            schema_version: 2,
            expected_node_id: template.executor_node.clone(),
            expected_runtime_id: self.runtime_id.clone(),
        })?;
        let crate::workload_leases::definition::Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &template.source
        else {
            bail!("Camera source inspection is not supported by the microphone endpoint")
        };
        ensure!(
            self.local_bindings_current
                && self.adapter_sha256 == *adapter_sha256
                && self.resources.iter().any(|r| r.available
                    && r.resource_id == *resource_id
                    && r.resource_sha256.as_ref() == Some(resource_sha256)),
            "Reviewed workload source differs from the current node snapshot"
        );
        Ok(())
    }
}

impl crate::NodeService {
    pub fn workload_sources(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Source inspection targets another node"
        );
        let version = self.source_readiness.version();
        ensure!(version < u64::MAX, "Source readiness version exhausted");
        let engine = self.passive_source_engine(&request.expected_runtime_id)?;
        #[cfg(test)]
        self.source_readiness
            .inspecting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let result = self.inspect_grant_configuration(|config| {
            let saved: crate::context::NodeMetadata =
                serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
            saved.validate()?;
            let policy: crate::NodePolicy =
                serde_json::from_value(config.get("permissions").cloned().unwrap_or(json!({})))?;
            let consent = crate::listening::Consent::from_config(config)?;
            let revision = crate::listening::revision(config)?;
            let state = self.listening_gate.snapshot();
            ensure!(
                config["node_id"] == self.node_id
                    && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                "Source inspection runtime changed"
            );
            let workload_policy = crate::workload_policy::from_config(config)?;
            let mut report = Report {
                schema_version: 2,
                report_type: "workload_sources".into(),
                success: true,
                node_id: self.node_id.clone(),
                runtime_id: request.expected_runtime_id.clone(),
                source_kind: "microphone_energy".into(),
                adapter_sha256: microphone_adapter_digest(),
                resources: engine
                    .applied_resources()
                    .iter()
                    .filter(|r| r.capability == "audio.capture")
                    .map(|r| Resource {
                        resource_id: crate::context::resource_id(&self.node_id, &r.id),
                        name: r.name.clone(),
                        area: r.area.clone(),
                        available: r.available,
                        resource_sha256: microphone_resource_digest(r).ok(),
                    })
                    .collect(),
                local_bindings_current: json!(saved.resources) == json!(engine.applied_resources()),
                listening_enabled: consent.enabled,
                listening_revision: revision,
                listening_epoch: state.0,
                listening_blocked: state.1,
                saved_microphone_permission: policy.media.microphone,
                applied_microphone_permission: engine.applied_permissions().media.microphone,
                python_configured: engine.python_configured(),
                capture_started: false,
                execution_authorized: false,
                execution_available: false,
            };
            if cfg!(windows)
                && workload_policy.enabled
                && !self.workload_gate.snapshot().1
                && report.local_bindings_current
                && report.listening_enabled
                && !report.listening_blocked
                && report.saved_microphone_permission
                && report.applied_microphone_permission
                && report.python_configured
                && report
                    .resources
                    .iter()
                    .any(|r| r.available && r.resource_sha256.is_some())
            {
                report.schema_version = 3;
                report.execution_available = true;
            }
            report.validate(&request)?;
            Ok(json!(report))
        })?;
        ensure!(
            version == self.source_readiness.version(),
            "Source configuration or runtime changed during inspection"
        );
        Ok(result)
    }
}

impl crate::NodeServer {
    /// Select only the reviewed source's independently authenticated snapshot.
    /// There is no fallback to another source kind or provider after refusal.
    pub async fn workload_source(
        &self,
        template: &crate::workload_leases::definition::Template,
        freshness_seconds: u64,
    ) -> Result<SourceSnapshot> {
        use crate::workload_leases::definition::SourceKind;
        template.validate()?;
        let snapshot = match template.source.source_kind() {
            SourceKind::MicrophoneEnergy => SourceSnapshot::Microphone(
                self.workload_sources(&template.executor_node, freshness_seconds)
                    .await?,
            ),
            SourceKind::CameraMotion => SourceSnapshot::Camera(
                self.camera_workload_sources(&template.executor_node, freshness_seconds)
                    .await?,
            ),
        };
        snapshot.confirms_template(template)?;
        Ok(snapshot)
    }
    /// Read from one independently authenticated, pinned live node session.
    /// Callers must also apply their existing origin/disclosure rules.
    pub async fn workload_sources(&self, node: &str, freshness_seconds: u64) -> Result<Report> {
        use anyhow::Context;
        use std::{collections::BTreeSet, time::Duration};
        ensure!(
            (1..=60).contains(&freshness_seconds),
            "Invalid source freshness bound"
        );
        let before = self.grant_session(node).await?;
        let (_, age) = self
            .peer_records(&BTreeSet::from([node.into()]))
            .await?
            .pop()
            .context("Source node is not registered")?;
        ensure!(
            age <= Duration::from_secs(freshness_seconds),
            "Source node registration is stale"
        );
        let state = self
            .node_state(node)
            .await
            .context("Source node state unavailable")?;
        let offer: crate::registration::Offer =
            serde_json::from_value(state.registration["registration_receipt"].clone())?;
        offer.validate()?;
        let request = Request {
            schema_version: 2,
            expected_node_id: node.into(),
            expected_runtime_id: offer.runtime_id,
        };
        let response = self
            .invoke_remote(
                node,
                "node.configure",
                "workload_sources",
                json!(request),
                Duration::from_secs(5),
            )
            .await?;
        ensure!(
            response["success"] == true,
            "Source node refused inspection"
        );
        let report: Report = serde_json::from_value(response["data"].clone())?;
        report.validate(&request)?;
        ensure!(
            self.grant_session(node).await? == before,
            "Authenticated source session changed during inspection"
        );
        let latest = self.node_state(node).await.context("Source node retired")?;
        let latest: crate::registration::Offer =
            serde_json::from_value(latest.registration["registration_receipt"].clone())?;
        latest.validate()?;
        ensure!(
            latest.runtime_id == request.expected_runtime_id,
            "Source runtime changed during inspection"
        );
        Ok(report)
    }
}

/// Local typed result, never another serializable execution authority.
pub enum SourceSnapshot {
    Microphone(Report),
    Camera(crate::camera_sources::Report),
}
impl SourceSnapshot {
    pub fn runtime_id(&self) -> &str {
        match self {
            Self::Microphone(report) => &report.runtime_id,
            Self::Camera(report) => &report.runtime_id,
        }
    }
    pub fn confirms_template(
        &self,
        template: &crate::workload_leases::definition::Template,
    ) -> Result<()> {
        match self {
            Self::Microphone(report) => report.confirms_template(template),
            Self::Camera(report) => report.confirms_template(template),
        }
    }
    pub fn confirms_execution(
        &self,
        template: &crate::workload_leases::definition::Template,
        runtime: &str,
    ) -> Result<()> {
        self.confirms_template(template)?;
        let ready = match self {
            Self::Microphone(r) => {
                r.schema_version == 3
                    && r.execution_available
                    && r.listening_enabled
                    && !r.listening_blocked
                    && r.saved_microphone_permission
                    && r.applied_microphone_permission
                    && r.python_configured
            }
            Self::Camera(r) => {
                r.schema_version == 2
                    && r.execution_available
                    && r.listening_enabled
                    && !r.listening_blocked
                    && r.saved_camera_permission
                    && r.applied_camera_permission
                    && r.python_configured
            }
        };
        ensure!(
            ready && self.runtime_id() == runtime,
            "Workload source requires its applied feature permission, local listening consent, configured Python and current runtime"
        );
        Ok(())
    }
}

pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    report.validate(&Request {
        schema_version: 2,
        expected_node_id: report.node_id.clone(),
        expected_runtime_id: report.runtime_id.clone(),
    })?;
    let mut text = format!(
        "Node {} microphone sources\nListening consent: {} (blocked: {})\nMicrophone permission (saved/applied): {}/{}\nBindings current: {}; Python configured: {}\n",
        report.node_id,
        report.listening_enabled,
        report.listening_blocked,
        report.saved_microphone_permission,
        report.applied_microphone_permission,
        report.local_bindings_current,
        report.python_configured
    );
    if report.resources.is_empty() {
        text.push_str("No configured microphone resource. Prepare one with `eefn relay configure --microphone`, then restart to apply it.\n");
    }
    for row in report.resources {
        let name: String = row
            .name
            .chars()
            .map(|c| if c.is_control() { '\u{fffd}' } else { c })
            .collect();
        text.push_str(&format!(
            "{}  {}  available: {}; binding supported: {}\n",
            row.resource_id,
            name,
            row.available,
            row.resource_sha256.is_some()
        ));
    }
    text.push_str(if report.execution_available {
        "Ready for an approved continuous workload. No capture has started; device access is checked when work begins.\n"
    } else {
        "Continuous input is not ready. Check microphone permission, local listening consent, workload policy and applied source settings.\n"
    });
    Ok(text)
}

#[cfg(test)]
#[path = "workload_sources_tests.rs"]
mod tests;
