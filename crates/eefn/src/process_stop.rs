//! Scoped owned-process quiescence, never rollback or outside-service termination.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub schema_version: u32,
    pub kind: String,
    pub mode: String,
    pub observed_at_ms: u64,
    pub stop_requested: bool,
    pub owned_process_tree_empty: bool,
    pub external_effects_stopped: bool,
    pub finished_actions_undone: bool,
}
impl Observation {
    #[cfg(windows)]
    pub(crate) fn observed() -> Self {
        Self {
            schema_version: 1,
            kind: "owned_process_tree_empty_after_stop_request".into(),
            mode: "windows_owned_job".into(),
            observed_at_ms: crate::protocol::now_ms(),
            stop_requested: true,
            owned_process_tree_empty: true,
            external_effects_stopped: false,
            finished_actions_undone: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.kind != "owned_process_tree_empty_after_stop_request"
            || self.mode != "windows_owned_job"
            || self.observed_at_ms == 0
            || self.observed_at_ms > i64::MAX as u64
            || !self.stop_requested
            || !self.owned_process_tree_empty
            || self.external_effects_stopped
            || self.finished_actions_undone
        {
            bail!("Unconfirmed owned-process stop observation")
        }
        Ok(())
    }
}

/// Both records must bind the original exact grant. This never authorizes work
/// or changes the v1 durable-intent-only stop receipt into an effects guarantee.
pub fn confirmed(
    grant: &crate::grants::ExecutionGrant,
    stop: &crate::grants::stop::Report,
    result: &crate::grants::GrantReport,
) -> Option<Observation> {
    if !stop.confirms(grant) {
        return None;
    }
    from_result(grant, result)
}
fn from_result(
    grant: &crate::grants::ExecutionGrant,
    result: &crate::grants::GrantReport,
) -> Option<Observation> {
    if grant.capability != "process.exec"
        || grant.action != "run"
        || !result.confirms(grant)
        || result.state != crate::grants::ReceiptState::Completed
        || !result.persisted
        || result.success
        || result.error_code.as_deref() != Some("process_exit_unsuccessful")
        || result.data.get("success") != Some(&Value::Bool(false))
    {
        return None;
    }
    let observation: Observation =
        serde_json::from_value(result.data.get("process_stop_observation")?.clone()).ok()?;
    observation.validate().ok()?;
    Some(observation)
}

pub fn confirmed_inspection(
    grant: &crate::grants::ExecutionGrant,
    intent: &crate::grants::stop::inspection::Inspection,
    result: &crate::grants::GrantReport,
) -> Option<Observation> {
    let scope = crate::grants::stop::Scope::from_grant(grant).ok()?;
    if !intent.confirms(&scope) || intent.evidence.is_none() {
        return None;
    }
    from_result(grant, result)
}

/// Private in-memory notification. No wire/configuration/ordinary-RPC selector.
pub(crate) struct Control(pub(crate) tokio::sync::watch::Receiver<bool>);
pub(crate) fn control(
    grant: &crate::grants::ExecutionGrant,
) -> Option<(tokio::sync::watch::Sender<bool>, Control)> {
    if cfg!(windows) && grant.capability == "process.exec" && grant.action == "run" {
        let (sender, receiver) = tokio::sync::watch::channel(false);
        Some((sender, Control(receiver)))
    } else {
        None
    }
}
impl Control {
    pub(crate) async fn requested(&mut self) {
        loop {
            if *self.0.borrow() {
                return;
            }
            if self.0.changed().await.is_err() {
                // A vanished controller is not a durable stop notification.
                std::future::pending::<()>().await;
            }
        }
    }
}

#[cfg(test)]
#[path = "process_stop_tests.rs"]
mod tests;
