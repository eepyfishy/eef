//! Passive camera source bindings, not device probing or execution authority.
use crate::workload_leases::definition::{Source, Template, camera_resource_digest};
pub use crate::workload_sources::{Request, Resource};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    pub saved_camera_permission: bool,
    pub applied_camera_permission: bool,
    /// Configuration only; does not prove OpenCV, driver or endpoint availability.
    pub python_configured: bool,
    pub capture_started: bool,
    pub execution_authorized: bool,
    pub execution_available: bool,
}
impl Report {
    pub fn validate(&self, request: &Request) -> Result<()> {
        request.validate()?;
        ensure!(
            matches!(
                (self.schema_version, self.execution_available),
                (1, false) | (2, true)
            ) && self.report_type == "camera_workload_sources"
                && self.success
                && self.node_id == request.expected_node_id
                && self.runtime_id == request.expected_runtime_id
                && self.source_kind == "camera_motion"
                && crate::workload_sources::digest(&self.adapter_sha256)
                && self.listening_revision <= i64::MAX as u64
                && !self.capture_started
                && !self.execution_authorized
                && self.resources.len() <= 128
                && serde_json::to_vec(self)?.len() <= 256 * 1024,
            "Unconfirmed camera source snapshot or unsupported execution claim"
        );
        if self.execution_available {
            ensure!(
                self.local_bindings_current
                    && self.listening_enabled
                    && !self.listening_blocked
                    && self.saved_camera_permission
                    && self.applied_camera_permission
                    && self.python_configured
                    && self
                        .resources
                        .iter()
                        .any(|r| r.available && r.resource_sha256.is_some()),
                "Executable camera snapshot lacks current local readiness"
            );
        }
        crate::workload_sources::validate_resources(&self.node_id, &self.resources)
    }
    /// Exact current binding for review, never permission or workload readiness.
    pub fn confirms_template(&self, template: &Template) -> Result<()> {
        template.validate()?;
        self.validate(&Request {
            schema_version: 2,
            expected_node_id: template.executor_node.clone(),
            expected_runtime_id: self.runtime_id.clone(),
        })?;
        let Source::CameraMotion {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &template.source
        else {
            bail!("Microphone source inspection is not supported by the camera endpoint");
        };
        ensure!(
            self.local_bindings_current
                && self.adapter_sha256 == *adapter_sha256
                && self.resources.iter().any(|r| r.available
                    && r.resource_id == *resource_id
                    && r.resource_sha256.as_ref() == Some(resource_sha256)),
            "Reviewed camera source differs from the current node snapshot"
        );
        Ok(())
    }
}

impl crate::NodeService {
    pub fn camera_workload_sources(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Camera inspection targets another node"
        );
        let version = (
            self.source_readiness.version(),
            self.camera_listening_gate.snapshot(),
        );
        ensure!(version.0 < u64::MAX, "Source readiness version exhausted");
        let engine = self.passive_source_engine(&request.expected_runtime_id)?;
        #[cfg(test)]
        self.source_readiness
            .inspecting
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let result = self.inspect_grant_configuration(|config| {
            let saved: crate::context::NodeMetadata =
                serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
            saved.validate()?;
            let permissions: crate::NodePolicy =
                serde_json::from_value(config.get("permissions").cloned().unwrap_or(json!({})))?;
            ensure!(
                config["node_id"] == self.node_id
                    && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                "Camera inspection runtime changed"
            );
            let consent = crate::listening::Consent::from_config(config)?;
            let state = self.camera_listening_gate.snapshot();
            let policy = crate::workload_policy::from_config(config)?;
            let mut report = Report {
                schema_version: 1,
                report_type: "camera_workload_sources".into(),
                success: true,
                node_id: self.node_id.clone(),
                runtime_id: request.expected_runtime_id.clone(),
                source_kind: "camera_motion".into(),
                adapter_sha256: crate::workload_sources::camera_adapter_digest(),
                resources: engine
                    .applied_resources()
                    .iter()
                    .filter(|r| r.capability == "camera.capture")
                    .map(|r| Resource {
                        resource_id: crate::context::resource_id(&self.node_id, &r.id),
                        name: r.name.clone(),
                        area: r.area.clone(),
                        available: r.available,
                        resource_sha256: camera_resource_digest(r).ok(),
                    })
                    .collect(),
                local_bindings_current: json!(saved.resources) == json!(engine.applied_resources()),
                listening_enabled: consent.enabled,
                listening_revision: crate::listening::revision(config)?,
                listening_epoch: state.0,
                listening_blocked: state.1,
                saved_camera_permission: permissions.media.camera,
                applied_camera_permission: engine.applied_permissions().media.camera,
                python_configured: engine.python_configured(),
                capture_started: false,
                execution_authorized: false,
                execution_available: false,
            };
            if cfg!(windows)
                && policy.enabled
                && !self.workload_gate.snapshot().1
                && report.local_bindings_current
                && report.listening_enabled
                && !report.listening_blocked
                && report.saved_camera_permission
                && report.applied_camera_permission
                && report.python_configured
                && engine
                    .camera_runtime(&self.node_id, &self.camera_pool)
                    .is_ok()
                && report
                    .resources
                    .iter()
                    .any(|r| r.available && r.resource_sha256.is_some())
            {
                report.schema_version = 2;
                report.execution_available = true;
            }
            report.validate(&request)?;
            Ok(json!(report))
        })?;
        ensure!(
            version
                == (
                    self.source_readiness.version(),
                    self.camera_listening_gate.snapshot()
                ),
            "Camera configuration or runtime changed during inspection"
        );
        Ok(result)
    }
}

impl crate::NodeServer {
    /// Passive read from one independently authenticated and pinned live session.
    /// The calling coordinator still enforces origin and disclosure rules.
    pub async fn camera_workload_sources(
        &self,
        node: &str,
        freshness_seconds: u64,
    ) -> Result<Report> {
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
            .context("Camera node is not registered")?;
        ensure!(
            age <= Duration::from_secs(freshness_seconds),
            "Camera node registration is stale"
        );
        let state = self
            .node_state(node)
            .await
            .context("Camera node state unavailable")?;
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
                "camera_workload_sources",
                json!(request),
                Duration::from_secs(5),
            )
            .await?;
        ensure!(
            response["success"] == true,
            "Camera node refused inspection"
        );
        let report: Report = serde_json::from_value(response["data"].clone())?;
        report.validate(&request)?;
        ensure!(
            self.grant_session(node).await? == before,
            "Authenticated camera session changed during inspection"
        );
        let latest = self.node_state(node).await.context("Camera node retired")?;
        let latest: crate::registration::Offer =
            serde_json::from_value(latest.registration["registration_receipt"].clone())?;
        latest.validate()?;
        ensure!(
            latest.runtime_id == request.expected_runtime_id,
            "Camera runtime changed during inspection"
        );
        Ok(report)
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
        "Node {} camera sources\nListening consent: {} (blocked: {})\nCamera permission (saved/applied): {}/{}\nBindings current: {}; Python configured: {}\n",
        report.node_id,
        report.listening_enabled,
        report.listening_blocked,
        report.saved_camera_permission,
        report.applied_camera_permission,
        report.local_bindings_current,
        report.python_configured
    );
    if report.resources.is_empty() {
        text.push_str("No configured camera resource.\n");
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
        "Ready for an approved camera workload. Device access is checked when work begins; this inspection opened no device and granted no work.\n"
    } else {
        "Camera workload execution is not available with these applied settings. Check camera permission, local listening consent, workload policy and source configuration. This inspection opened no device and granted no work.\n"
    });
    Ok(text)
}

#[cfg(test)]
#[path = "camera_sources_tests.rs"]
mod tests;
