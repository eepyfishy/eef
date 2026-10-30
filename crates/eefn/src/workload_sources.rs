//! Node-owned source snapshots for review, never capture or execution authority.
use crate::workload_leases::definition::microphone_resource_digest;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// The reviewed built-in source. Future execution must use these exact bytes,
/// rather than reopening a path whose contents may change after inspection.
pub(crate) const MICROPHONE_SOURCE: &[u8] =
    include_bytes!("../../../python/streams/microphone_energy.py");
pub fn microphone_adapter_digest() -> String {
    hex::encode(Sha256::digest(MICROPHONE_SOURCE))
}
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Inspect configured microphone resources without opening a device.
    Sources,
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
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Report {
    pub fn validate(&self, request: &Request) -> Result<()> {
        request.validate()?;
        ensure!(
            self.success
                && self.schema_version == 2
                && self.report_type == "workload_sources"
                && self.node_id == request.expected_node_id
                && self.runtime_id == request.expected_runtime_id
                && self.source_kind == "microphone_energy"
                && digest(&self.adapter_sha256)
                && !self.capture_started
                && !self.execution_authorized
                && !self.execution_available
                && self.listening_revision <= i64::MAX as u64
                && self.resources.len() <= 128
                && serde_json::to_vec(self)?.len() <= 256 * 1024,
            "Unconfirmed source snapshot or unsupported execution claim"
        );
        let prefix = format!("{}::", self.node_id);
        let mut ids = HashSet::new();
        for row in &self.resources {
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
        } = &template.source;
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
        self.with_permission_runtime(&request.expected_runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                let saved: crate::context::NodeMetadata =
                    serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
                saved.validate()?;
                let policy: crate::NodePolicy = serde_json::from_value(
                    config.get("permissions").cloned().unwrap_or(json!({})),
                )?;
                let consent = crate::listening::Consent::from_config(config)?;
                let revision = crate::listening::revision(config)?;
                let state = self.listening_gate.snapshot();
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                    "Source inspection runtime changed"
                );
                let report = Report {
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
                    local_bindings_current: json!(saved.resources)
                        == json!(engine.applied_resources()),
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
                report.validate(&request)?;
                Ok(json!(report))
            })
        })
    }
}

impl crate::NodeServer {
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
    text.push_str("Configuration snapshots only. Device availability is unverified; continuous execution integration remains in development.\n");
    Ok(text)
}

#[cfg(test)]
#[path = "workload_sources_tests.rs"]
mod tests;
