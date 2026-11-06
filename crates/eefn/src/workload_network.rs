//! Typed connection metadata for the continuous workload route. These values
//! cannot create a lease proof, consume a grant or authorize a source by itself.
pub use crate::workload_events::{Event, Kind as EventKind};
use crate::workload_leases::{Policy, definition::Definition};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Historical metadata only. Hashes and event rows cannot mint source/exit evidence.
pub struct EventSummary {
    pub count: u64,
    pub bytes: usize,
    pub prefix_sha256: String,
}
pub fn summarize_events(definition: &Definition, events: &[Event]) -> Result<EventSummary> {
    definition.validate()?;
    ensure!(
        events.len() as u64 <= definition.events.max_events,
        "Workload event count exceeded"
    );
    let mut prefix = hex::encode(Sha256::digest(definition.scope()?.grant_sha256.as_bytes()));
    let mut bytes = 0usize;
    let mut previous = None;
    for event in events {
        event.follows(definition, previous)?;
        let data = serde_json::to_vec(event)?;
        bytes = bytes
            .checked_add(data.len())
            .ok_or_else(|| anyhow::anyhow!("Workload event bytes overflow"))?;
        ensure!(
            data.len() <= definition.events.max_event_bytes
                && bytes <= definition.events.max_total_bytes,
            "Workload event metadata budget exceeded"
        );
        let row = hex::encode(Sha256::digest(&data));
        prefix = hex::encode(Sha256::digest(format!("{prefix}{row}").as_bytes()));
        previous = Some(event);
    }
    Ok(EventSummary {
        count: events.len() as u64,
        bytes,
        prefix_sha256: prefix,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub node: String,
    pub issuer: String,
    pub session: String,
    pub runtime: String,
    pub policy: Policy,
}

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
