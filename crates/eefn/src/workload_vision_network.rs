//! Explicit vision protocol negotiation. Metadata-only support never implies
//! this contract, and none of these serializable values creates execution authority.
use crate::workload_leases::{Policy, definition::Source};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub runtime_id: Option<String>,
    pub policy: Option<Policy>,
    pub source_kind: String,
    pub definition_schema_version: u32,
    pub offer_schema_version: u32,
    pub completion_schema_version: u32,
    pub results_schema_version: u32,
    pub private_images: bool,
    pub restart_authorized: bool,
}
impl Advertisement {
    pub fn unavailable() -> Self {
        Self {
            schema_version: 1,
            available: false,
            runtime_id: None,
            policy: None,
            source_kind: "camera_vision".into(),
            definition_schema_version: 3,
            offer_schema_version: 3,
            completion_schema_version: 3,
            results_schema_version: 1,
            private_images: true,
            restart_authorized: false,
        }
    }
    pub fn for_runtime(runtime: &str, policy: Policy, configuration_ready: bool) -> Result<Self> {
        policy.validate()?;
        if !configuration_ready || !policy.enabled || !policy.vision_analysis {
            return Ok(Self::unavailable());
        }
        let mut value = Self::unavailable();
        value.available = true;
        value.runtime_id = Some(runtime.into());
        value.policy = Some(policy);
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1
                && self.source_kind == "camera_vision"
                && self.definition_schema_version == 3
                && self.offer_schema_version == 3
                && self.completion_schema_version == 3
                && self.results_schema_version == 1
                && self.private_images
                && !self.restart_authorized,
            "Unsupported vision execution contract; no metadata fallback"
        );
        match (&self.runtime_id, &self.policy, self.available) {
            (Some(runtime), Some(policy), true) => {
                crate::automation_occurrence::uuid(runtime)?;
                policy.validate()?;
                ensure!(
                    policy.enabled && policy.vision_analysis,
                    "Vision owner approval missing"
                );
            }
            (None, None, false) => {}
            _ => anyhow::bail!("Vision support lacks its exact runtime and owner policy"),
        }
        Ok(())
    }
    pub fn confirms(&self, source: &Source, runtime: &str) -> Result<()> {
        self.validate()?;
        ensure!(
            matches!(source, Source::CameraVision { .. })
                && self.available
                && self.runtime_id.as_deref() == Some(runtime),
            "Vision support differs from the reviewed source/runtime"
        );
        Ok(())
    }
    pub(crate) fn validate_registration(&self, registered: &Value) -> Result<()> {
        self.validate()?;
        if !self.available {
            return Ok(());
        }
        let results: crate::vision_result_inspection::Advertisement =
            serde_json::from_value(registered["workload_vision_results"].clone())?;
        results.validate()?;
        let stop: crate::grants::stop::Advertisement =
            serde_json::from_value(registered["grant_stop"].clone())?;
        stop.validate()?;
        ensure!(
            results.available
                && stop.available
                && registered["execution_grants"]["available"] == true
                && registered["registration_receipt"]["runtime_id"].as_str()
                    == self.runtime_id.as_deref(),
            "Vision execution requires its registered runtime, journal, analysis reads and stop support"
        );
        Ok(())
    }
}

/// Select one exact protocol. Required vision fields cannot be supplied by a
/// metadata advertisement, and a failed vision negotiation never falls back.
pub(crate) fn registered_support(registered: &Value, source: &Source) -> Result<(String, Policy)> {
    if matches!(source, Source::CameraVision { .. }) {
        let support: Advertisement =
            serde_json::from_value(registered["workload_vision_execution"].clone())?;
        let runtime = support
            .runtime_id
            .as_deref()
            .context("Vision runtime unavailable")?;
        support.confirms(source, runtime)?;
        support.validate_registration(registered)?;
        Ok((
            runtime.into(),
            support.policy.context("Vision policy unavailable")?,
        ))
    } else {
        let support: crate::workload_network::Advertisement =
            serde_json::from_value(registered["workload_execution"].clone())?;
        support.confirms_source(source)?;
        ensure!(
            support.available && support.completion_payload_prefix,
            "Metadata workload completion support unavailable"
        );
        Ok((
            support.runtime_id.context("Workload runtime unavailable")?,
            support.policy.context("Workload policy unavailable")?,
        ))
    }
}

#[cfg(test)]
#[path = "workload_vision_network_tests.rs"]
mod tests;
