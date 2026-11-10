//! Explicit reconciliation data, never inference, grant or continuation authority.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub use crate::request_recovery::{RecoverCommand as Command, RecoveryActor};

pub const MAX_RECOVERIES: usize = 48;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub command: Command,
    pub actor: RecoveryActor,
    pub stage: crate::interpreter_command::Stage,
    pub source_revision: u64,
    pub applied_revision: u64,
    pub decided_at_ms: u64,
    pub binding_sha256: String,
    pub report_sha256: String,
    pub proposal_sha256: String,
    pub capabilities_sha256: String,
}
impl Record {
    pub fn validate(&self) -> Result<()> {
        self.command.validate()?;
        for digest in [
            &self.binding_sha256,
            &self.report_sha256,
            &self.proposal_sha256,
            &self.capabilities_sha256,
        ] {
            crate::request_interpretation::digest(digest)?;
        }
        ensure!(
            self.schema_version == 1
                && self.source_revision > 0
                && self.source_revision <= self.command.expected_revision
                && self.command.expected_revision.checked_add(1) == Some(self.applied_revision)
                && self.decided_at_ms > 0
                && self.decided_at_ms <= i64::MAX as u64
                && serde_json::to_vec(self)?.len() <= 2048,
            "Invalid retained interpreter recovery receipt"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn record() -> Record {
        Record {
            schema_version: 1,
            command: Command {
                id: uuid::Uuid::new_v4().simple().to_string(),
                expected_revision: 3,
                operation_id: uuid::Uuid::new_v4().to_string(),
                grant_id: uuid::Uuid::new_v4().to_string(),
            },
            actor: RecoveryActor::OriginNode,
            stage: crate::interpreter_command::Stage::Initial,
            source_revision: 1,
            applied_revision: 4,
            decided_at_ms: 1,
            binding_sha256: "a".repeat(64),
            report_sha256: "b".repeat(64),
            proposal_sha256: "c".repeat(64),
            capabilities_sha256: "d".repeat(64),
        }
    }
    #[test]
    fn interpreter_recovery_command_accepts_only_exact_references_without_result_or_authority() {
        let command = record().command;
        command.validate().unwrap();
        for field in [
            "prompt",
            "proposal",
            "result",
            "claim",
            "provider",
            "actor",
            "execution_authorized",
        ] {
            let mut value = json!(command);
            value[field] = json!({});
            assert!(serde_json::from_value::<Command>(value).is_err(), "{field}");
        }
        for revision in [0, u64::MAX] {
            let mut invalid = command.clone();
            invalid.expected_revision = revision;
            assert!(invalid.validate().is_err());
        }
        let mut reused = command.clone();
        reused.operation_id = reused.grant_id.clone();
        assert!(reused.validate().is_err());
    }
    #[test]
    fn interpreter_recovery_history_requires_a_bounded_distinct_applied_revision_and_digests() {
        let original = record();
        original.validate().unwrap();
        for field in ["schema", "revision", "source", "time", "digest"] {
            let mut bad = original.clone();
            match field {
                "schema" => bad.schema_version = 2,
                "revision" => bad.applied_revision = bad.command.expected_revision,
                "source" => bad.source_revision = bad.command.expected_revision + 1,
                "time" => bad.decided_at_ms = 0,
                _ => bad.report_sha256 = "forged".into(),
            }
            assert!(bad.validate().is_err(), "{field}");
        }
        let mut injected = json!(original);
        injected["grant"] = json!({});
        assert!(serde_json::from_value::<Record>(injected).is_err());
    }
}
