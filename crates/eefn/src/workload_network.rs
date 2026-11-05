//! Typed connection metadata for the continuous workload route. These values
//! cannot create a lease proof, consume a grant or authorize a source by itself.
use crate::workload_leases::{Policy, definition::Definition};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub runtime_id: Option<String>,
    pub policy: Option<Policy>,
    pub source_kind: String,
    pub event_delivery: String,
    pub restart_authorized: bool,
}
impl Advertisement {
    pub fn unavailable() -> Self {
        Self {
            schema_version: 2,
            available: false,
            runtime_id: None,
            policy: None,
            source_kind: "microphone_energy".into(),
            event_delivery: "bounded_metadata".into(),
            restart_authorized: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 2
                && self.source_kind == "microphone_energy"
                && self.event_delivery == "bounded_metadata"
                && !self.restart_authorized,
            "Unsupported workload execution advertisement"
        );
        if let Some(runtime) = &self.runtime_id {
            crate::automation_occurrence::uuid(runtime)?;
        }
        if let Some(policy) = &self.policy {
            policy.validate()?;
        }
        ensure!(
            !self.available
                || (self.runtime_id.is_some() && self.policy.as_ref().is_some_and(|p| p.enabled)),
            "Workload support lacks its original runtime or enabled policy"
        );
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub expected_runtime_id: String,
    pub definition: Definition,
}
impl Offer {
    pub fn validate(&self, issuer: &str, session: &str, node: &str, runtime: &str) -> Result<()> {
        self.definition.validate()?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        ensure!(
            self.kind == "execute_workload"
                && self.schema_version == 2
                && self.id == self.definition.grant_id
                && self.expected_runtime_id == runtime
                && self
                    .definition
                    .scope()?
                    .matches_session(issuer, session, node),
            "Workload offer differs from its pinned node session/runtime"
        );
        Ok(())
    }
}
