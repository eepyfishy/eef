//! Read-only scoped stop observations, not a job-wide or external-effect guarantee.
use crate::{grants::stop::Scope, process_stop::Observation};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const NOTE: &str = "Read-only observations for the listed exact issued grants. Unlisted attempts and external effects remain unconfirmed; finished actions are not undone. No stop signal, execution, replay, renewal, recovery or tracking mutation was requested.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        self.command.validate()?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported process-stop inspection")
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    OwnedProcessTreeEmpty,
    Unconfirmed,
    Unsupported,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantStatus {
    pub scope: Scope,
    pub availability: Availability,
    pub intent_recorded: bool,
    pub observation: Option<Observation>,
}
impl GrantStatus {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        if (self.availability == Availability::OwnedProcessTreeEmpty) != self.observation.is_some()
            || (self.observation.is_some() && !self.intent_recorded)
        {
            bail!("Unconfirmed scoped process-stop status")
        }
        if let Some(observation) = &self.observation {
            observation.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub origin_node: String,
    pub stop_requested: bool,
    pub issued_attempts: usize,
    pub not_inspected_issued_attempts: usize,
    pub grants: Vec<GrantStatus>,
    pub read_only: bool,
    pub execution_requested: bool,
    pub stop_signal_sent: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub external_effects_stopped: bool,
    pub finished_actions_undone: bool,
    pub note: String,
}
impl Report {
    pub fn validate(&self) -> Result<()> {
        crate::automation_occurrence::uuid(&self.runtime_id)?;
        crate::request_drafts::validate_id(&self.id)?;
        crate::network::validate_remote_node_id(&self.origin_node)?;
        if self.schema_version != 1
            || self.report_type != "job_process_stop_status"
            || !self.success
            || self.revision == 0
            || self.grants.len() > 2
            || self.issued_attempts < self.grants.len()
            || self.not_inspected_issued_attempts != self.issued_attempts - self.grants.len()
            || (!self.stop_requested && !self.grants.is_empty())
            || !self.read_only
            || self.execution_requested
            || self.stop_signal_sent
            || self.automatically_retried
            || self.reconciled
            || self.external_effects_stopped
            || self.finished_actions_undone
            || self.note != NOTE
        {
            bail!("Unconfirmed job process-stop inspection")
        }
        let mut ids = std::collections::BTreeSet::new();
        for grant in &self.grants {
            grant.validate()?;
            if grant.scope.job_id != self.id || !ids.insert(&grant.scope.grant_id) {
                bail!("Process-stop inspection scope changed")
            }
        }
        Ok(())
    }
}
pub fn confirmed(
    value: &Value,
    command: &Command,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && report.validate().is_ok()
        && report.id == command.id
        && origin.is_none_or(|s| s == report.origin_node)
        && runtime.is_none_or(|s| s == report.runtime_id)
}
pub fn render(value: &Value, command: &Command, origin: Option<&str>) -> Result<String> {
    if !confirmed(value, command, origin, None) {
        bail!("Unconfirmed process-stop status; no fallback")
    }
    let report: Report = serde_json::from_value(value.clone())?;
    let mut text = format!("Process stop status for job {}\n", report.id);
    for grant in &report.grants {
        let description = match grant.availability {
            Availability::OwnedProcessTreeEmpty => {
                "owned process tree exit observed; external effects unconfirmed"
            }
            Availability::Unconfirmed => "no confirmed owned-process stop observation",
            Availability::Unsupported => {
                "this operation does not support an owned-process stop observation"
            }
            Availability::Unavailable => "node evidence unavailable; stop effects unconfirmed",
        };
        text.push_str(&format!(
            "Node {}: {description}\n",
            grant.scope.executor_node
        ));
    }
    if report.grants.is_empty() {
        text.push_str("No retained stop-delivery targets to inspect; this does not prove no work is running.\n");
    }
    text.push_str(&format!(
        "{} of {} issued attempts not inspected.\n{}\n",
        report.not_inspected_issued_attempts, report.issued_attempts, NOTE
    ));
    Ok(text)
}

#[cfg(test)]
#[path = "process_stop_status_tests.rs"]
mod tests;
