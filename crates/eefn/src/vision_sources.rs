//! Passive camera/model configuration inspection. No device or provider probe,
//! execution negotiation, model selection, download or execution authority.
pub use crate::workload_sources::Request;
use crate::{
    model_selection::Backend,
    workload_leases::definition::{Source, Template},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub backend: Backend,
    pub model_id: String,
    pub local_endpoint_configured: bool,
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
    pub camera: crate::camera_sources::Report,
    pub models: Vec<Model>,
    pub model_bindings_current: bool,
    pub workload_enabled: bool,
    pub vision_enabled: bool,
    pub policy_revision: u64,
    pub policy_epoch: u64,
    pub policy_blocked: bool,
    /// Applied configuration only; not evidence of installed model or hardware.
    pub configuration_ready: bool,
    pub capture_started: bool,
    pub model_contacted: bool,
    pub execution_authorized: bool,
    /// This passive view does not negotiate the separate execution protocol.
    pub execution_available: bool,
}
impl Report {
    pub fn validate(&self, request: &Request) -> Result<()> {
        request.validate()?;
        self.camera.validate(request)?;
        ensure!(
            self.schema_version == 1
                && self.report_type == "vision_workload_sources"
                && self.success
                && self.node_id == request.expected_node_id
                && self.runtime_id == request.expected_runtime_id
                && self.source_kind == "camera_vision"
                && crate::workload_sources::digest(&self.adapter_sha256)
                && self.policy_revision <= i64::MAX as u64
                && (!self.vision_enabled || self.workload_enabled)
                && self.models.len() <= 128
                && serde_json::to_vec(self)?.len() <= 256 * 1024
                && !self.capture_started
                && !self.model_contacted
                && !self.execution_authorized
                && !self.execution_available,
            "Unconfirmed vision configuration snapshot or unsupported execution claim"
        );
        let mut ids = std::collections::HashSet::new();
        for model in &self.models {
            crate::model_selection::validate_id(&model.model_id)?;
            ensure!(
                ids.insert((model.backend.name(), &model.model_id)),
                "Duplicate vision model binding"
            );
        }
        ensure!(
            self.configuration_ready
                == (self.camera.execution_available
                    && self.model_bindings_current
                    && self.workload_enabled
                    && self.vision_enabled
                    && !self.policy_blocked
                    && self.policy_epoch < u64::MAX
                    && self.models.iter().any(|m| m.local_endpoint_configured)),
            "Vision configuration readiness differs from its current settings"
        );
        Ok(())
    }
    /// Confirm an exact binding for review, without authorizing its execution or
    /// attesting the owner-selected installed digest. Execution checks it live.
    pub fn confirms_template(&self, template: &Template) -> Result<()> {
        template.validate()?;
        self.validate(&Request {
            schema_version: 2,
            expected_node_id: template.executor_node.clone(),
            expected_runtime_id: self.runtime_id.clone(),
        })?;
        let Source::CameraVision {
            resource_id,
            resource_sha256,
            adapter_sha256,
            parameters,
        } = &template.source
        else {
            bail!("Vision inspection requires its distinct reviewed source");
        };
        ensure!(
            self.camera.local_bindings_current
                && self.model_bindings_current
                && self.adapter_sha256 == *adapter_sha256
                && self.camera.resources.iter().any(|r| r.available
                    && r.resource_id == *resource_id
                    && r.resource_sha256.as_ref() == Some(resource_sha256))
                && self.models.iter().any(|m| m.local_endpoint_configured
                    && m.backend == parameters.model.backend
                    && m.model_id == parameters.model.model_id),
            "Reviewed vision camera or model differs from current applied configuration"
        );
        Ok(())
    }
}
impl crate::NodeService {
    pub fn vision_workload_sources(&self, request: Request) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Vision inspection targets another node"
        );
        let version = (
            self.source_readiness.version(),
            self.workload_gate.snapshot(),
            self.camera_listening_gate.snapshot(),
        );
        ensure!(
            version.0 < u64::MAX,
            "Vision source readiness version exhausted"
        );
        let engine = self.passive_source_engine(&request.expected_runtime_id)?;
        let camera: crate::camera_sources::Report =
            serde_json::from_value(self.camera_workload_sources(request.clone())?)?;
        let report = self.inspect_grant_configuration(|config| {
            ensure!(
                config["node_id"] == self.node_id
                    && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                "Vision inspection runtime changed"
            );
            let policy = crate::workload_policy::from_config(config)?;
            let (models, model_bindings_current) =
                engine.passive_vision_models(&config["models"])?;
            let state = self.workload_gate.snapshot();
            let configuration_ready = camera.execution_available
                && model_bindings_current
                && policy.enabled
                && policy.vision_analysis
                && !state.1
                && state.0 < u64::MAX
                && models.iter().any(|m| m.local_endpoint_configured);
            Ok(Report {
                schema_version: 1,
                report_type: "vision_workload_sources".into(),
                success: true,
                node_id: self.node_id.clone(),
                runtime_id: request.expected_runtime_id.clone(),
                source_kind: "camera_vision".into(),
                adapter_sha256: crate::workload_sources::vision_adapter_digest(),
                camera,
                models,
                model_bindings_current,
                workload_enabled: policy.enabled,
                vision_enabled: policy.vision_analysis,
                policy_revision: crate::workload_policy::revision(config)?,
                policy_epoch: state.0,
                policy_blocked: state.1,
                configuration_ready,
                capture_started: false,
                model_contacted: false,
                execution_authorized: false,
                execution_available: false,
            })
        })?;
        ensure!(
            version
                == (
                    self.source_readiness.version(),
                    self.workload_gate.snapshot(),
                    self.camera_listening_gate.snapshot()
                ),
            "Vision configuration changed during inspection"
        );
        report.validate(&request)?;
        Ok(json!(report))
    }
}
impl crate::NodeServer {
    /// Independently authenticated read; no retry or older camera-only fallback.
    pub async fn vision_workload_sources(
        &self,
        node: &str,
        freshness_seconds: u64,
    ) -> Result<Report> {
        use std::{collections::BTreeSet, time::Duration};
        ensure!(
            (1..=60).contains(&freshness_seconds),
            "Invalid vision freshness bound"
        );
        let before = self.grant_session(node).await?;
        let (_, age) = self
            .peer_records(&BTreeSet::from([node.into()]))
            .await?
            .pop()
            .context("Vision node is not registered")?;
        ensure!(
            age <= Duration::from_secs(freshness_seconds),
            "Vision registration is stale"
        );
        let state = self
            .node_state(node)
            .await
            .context("Vision node state unavailable")?;
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
                "vision_workload_sources",
                json!(request),
                Duration::from_secs(5),
            )
            .await?;
        ensure!(
            response["success"] == true,
            "Vision node refused inspection"
        );
        let report: Report = serde_json::from_value(response["data"].clone())?;
        report.validate(&request)?;
        ensure!(
            self.grant_session(node).await? == before,
            "Authenticated vision session changed during inspection"
        );
        let latest = self.node_state(node).await.context("Vision node retired")?;
        let latest: crate::registration::Offer =
            serde_json::from_value(latest.registration["registration_receipt"].clone())?;
        latest.validate()?;
        ensure!(
            latest.runtime_id == request.expected_runtime_id,
            "Vision runtime changed during inspection"
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
        "Node {} vision configuration\nVision owner approval: {}; workload sharing: {}; blocked: {}\nModel bindings current: {}; configuration ready: {}\n",
        report.node_id,
        report.vision_enabled,
        report.workload_enabled,
        report.policy_blocked,
        report.model_bindings_current,
        report.configuration_ready
    );
    text.push_str(&crate::camera_sources::render(&json!(report.camera))?);
    if report.models.is_empty() {
        text.push_str("No applied vision model selection. Choose a vision-capable model using this node's model commands.\n");
    }
    for model in report.models {
        text.push_str(&format!(
            "{} / {}  local endpoint configured: {}\n",
            model.backend.name(),
            model.model_id,
            model.local_endpoint_configured
        ));
    }
    text.push_str("Configuration inspection only. No camera or model was contacted; installed model identity and hardware availability remain unchecked. Execution requires a separately reviewed job and current node support.\n");
    Ok(text)
}

#[cfg(test)]
#[path = "vision_sources_tests.rs"]
mod tests;
