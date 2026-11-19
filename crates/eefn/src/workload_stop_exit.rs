//! Read-only evidence for the exact continuous capture's owned process tree.
use crate::{grants::stop::Scope, process_stop::Observation};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub(crate) const KIND: &str = "workload_stop_exit";
pub(crate) const REGISTRATION_FIELD: &str = "workload_stop_exit_read";

#[derive(Debug, thiserror::Error)]
#[error("Node does not advertise retained continuous stop exit inspection")]
pub struct Unsupported;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Advertisement {
    schema_version: u32,
    pub(crate) available: bool,
    read_only: bool,
}
impl Advertisement {
    pub(crate) fn new(available: bool) -> Self {
        Self {
            schema_version: 1,
            available,
            read_only: true,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.read_only,
            "Invalid workload stop exit advertisement"
        );
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub(crate) schema_version: u32,
    pub(crate) report_type: String,
    pub(crate) scope: Scope,
    pub stop_recorded_at_ms: Option<u64>,
    pub observation: Option<Observation>,
    pub(crate) read_only: bool,
}
impl Report {
    pub(crate) fn validate(&self, scope: &Scope) -> Result<()> {
        scope.validate()?;
        ensure!(
            self.schema_version == 1
                && self.report_type == KIND
                && self.scope == *scope
                && self.read_only
                && self
                    .stop_recorded_at_ms
                    .is_none_or(|t| (1..=i64::MAX as u64).contains(&t)),
            "Continuous stop exit inspection differs from the original issuance"
        );
        if let Some(observation) = &self.observation {
            observation.validate()?;
            ensure!(
                self.stop_recorded_at_ms
                    .is_some_and(|at| at <= observation.observed_at_ms),
                "Owned capture exit lacks preceding retained stop intent"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stop_exit_requires_exact_scope_and_ordered_retained_intent() {
        let scope = Scope::from_grant(&crate::grants::tests::grant()).unwrap();
        let baseline = json!({
            "schema_version":1,"report_type":KIND,"scope":scope,"stop_recorded_at_ms":41,"read_only":true,
            "observation":{"schema_version":1,"kind":"owned_process_tree_empty_after_stop_request",
                "mode":"windows_owned_job","observed_at_ms":42,"stop_requested":true,
                "owned_process_tree_empty":true,"external_effects_stopped":false,"finished_actions_undone":false}
        });
        serde_json::from_value::<Report>(baseline.clone())
            .unwrap()
            .validate(&scope)
            .unwrap();
        for (path, value) in [
            ("/scope/session_id", json!(uuid::Uuid::new_v4().to_string())),
            ("/scope/grant_sha256", json!("f".repeat(64))),
            ("/stop_recorded_at_ms", json!(43)),
            ("/stop_recorded_at_ms", serde_json::Value::Null),
            ("/observation/external_effects_stopped", json!(true)),
            ("/read_only", json!(false)),
        ] {
            let mut forged = baseline.clone();
            *forged.pointer_mut(path).unwrap() = value;
            assert!(
                serde_json::from_value::<Report>(forged)
                    .unwrap()
                    .validate(&scope)
                    .is_err(),
                "{path}"
            );
        }
    }
}
