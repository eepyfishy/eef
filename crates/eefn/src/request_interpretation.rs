//! Shared saved-input checkpoint contracts. Ownership is never a model grant.
use crate::{context::RequestContext, request_intent::Input};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interpreter {
    pub node_id: String,
    pub backend: String,
    pub model_id: String,
    pub runtime_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub operation_id: String,
    #[serde(default)]
    pub inference_timeout_seconds: Option<u64>,
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::inference_budget::InferenceBudget::new(self.inference_timeout_seconds)
            .map_err(|_| anyhow::anyhow!("Invalid interpretation timeout"))?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "Unsupported saved interpretation schema"
        );
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        self.command.validate()
    }
}
impl Interpreter {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        crate::model_selection::validate_id(&self.model_id)?;
        crate::automation_occurrence::uuid(&self.runtime_id)?;
        ensure!(
            matches!(self.backend.as_str(), "ollama" | "llamacpp"),
            "Unsupported interpreter backend"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub id: String,
    pub expected_revision: u64,
    pub source_sha256: String,
    pub operation_id: String,
    pub origin_runtime_id: String,
    /// Missing in early internal journal records is unknown, never a provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Interpreter>,
}
pub fn digest(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "Invalid interpretation digest"
    );
    Ok(())
}
impl Claim {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::automation_occurrence::uuid(&self.origin_runtime_id)?;
        ensure!(
            self.expected_revision > 0,
            "Missing interpretation source revision"
        );
        digest(&self.source_sha256)?;
        if let Some(interpreter) = &self.interpreter {
            interpreter.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Claimed,
    Unknown,
    Promoted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema_version: u32,
    pub claim: Claim,
    pub input: Input,
    pub request_context: RequestContext,
    pub claimed_at_ms: u64,
    pub state: State,
    pub promoted_revision: Option<u64>,
    pub proposal_sha256: Option<String>,
}
impl Record {
    pub fn validate_source(&self, id: &str, revision: u64, context: &RequestContext) -> Result<()> {
        self.claim.validate()?;
        self.input.validate()?;
        ensure!(
            self.schema_version == 1
                && self.claim.id == id
                && self.claim.expected_revision < revision
                && self.claimed_at_ms > 0
                && &self.request_context == context
                && self.input.digest(context)? == self.claim.source_sha256,
            "Interpretation source evidence is inconsistent"
        );
        if self.state == State::Promoted {
            ensure!(
                self.promoted_revision.is_some_and(|r| r > self
                    .claim
                    .expected_revision
                    .saturating_add(1)
                    && r <= revision),
                "Invalid interpretation promotion revision"
            );
            digest(self.proposal_sha256.as_deref().unwrap_or(""))?;
        } else {
            ensure!(
                self.promoted_revision.is_none() && self.proposal_sha256.is_none(),
                "Unpromoted interpretation cannot contain completion evidence"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimCommand {
    pub schema_version: u32,
    pub claim: Claim,
}
impl ClaimCommand {
    pub fn validate(&self) -> Result<()> {
        self.claim.validate()?;
        ensure!(
            self.schema_version == 1 && self.claim.interpreter.is_some(),
            "Interpretation needs an explicit interpreter binding"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionCommand {
    pub schema_version: u32,
    pub claim: Claim,
    pub expected_revision: u64,
    pub proposal: crate::interpretation::Proposal,
}
impl PromotionCommand {
    pub fn validate(&self) -> Result<()> {
        ClaimCommand {
            schema_version: self.schema_version,
            claim: self.claim.clone(),
        }
        .validate()?;
        ensure!(
            self.expected_revision > self.claim.expected_revision,
            "Invalid interpretation completion revision"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= 64 * 1024,
            "Interpretation checkpoint exceeds its byte limit"
        );
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub id: String,
    pub revision: u64,
    pub status: String,
    pub operation_id: String,
    pub record: Record,
    pub reused: bool,
    pub draft: Option<crate::request_drafts::DraftRecord>,
    pub inference_authorized: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
}
impl Checkpoint {
    pub fn validate(&self, claim: &Claim, origin: &str) -> Result<()> {
        claim.validate()?;
        self.record
            .validate_source(&self.id, self.revision, &self.record.request_context)?;
        ensure!(
            self.schema_version == 1
                && self.report_type == "coordinator_input_interpretation"
                && self.success
                && self.id == claim.id
                && self.record.claim == *claim
                && self.operation_id == claim.operation_id
                && self.record.request_context.origin_node() == origin
                && !self.inference_authorized
                && !self.execution_authorized
                && !self.dispatched,
            "Interpretation checkpoint was not confirmed"
        );
        if self.record.state == State::Promoted {
            let draft = self
                .draft
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Promoted checkpoint is missing its review"))?;
            draft.validate()?;
            ensure!(
                draft.original_text == self.record.input.original_text
                    && draft.proposals[0].operation_id == self.record.request_context.request_id()
                    && Some(crate::grants::definition_digest(&json!(
                        draft.proposals[0].proposal
                    ))?) == self.record.proposal_sha256
                    && matches!(
                        self.status.as_str(),
                        "review"
                            | "awaiting_approval"
                            | "approved"
                            | "pending"
                            | "running"
                            | "completed"
                            | "failed"
                            | "cancelled"
                            | "paused"
                            | "pausing"
                            | "stopping"
                            | "interrupted"
                            | "abandoned"
                    ),
                "Retained interpretation review does not match its source"
            );
        } else {
            ensure!(
                self.draft.is_none() && matches!(self.status.as_str(), "review" | "cancelled"),
                "Unpromoted checkpoint cannot contain a review or executable state"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn claim() -> Claim {
        let runtime = uuid::Uuid::new_v4().to_string();
        Claim {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 1,
            source_sha256: "0".repeat(64),
            operation_id: uuid::Uuid::new_v4().to_string(),
            origin_runtime_id: runtime.clone(),
            interpreter: Some(Interpreter {
                node_id: "origin-node".into(),
                backend: "ollama".into(),
                model_id: "selected".into(),
                runtime_id: runtime,
            }),
        }
    }
    fn checkpoint() -> Checkpoint {
        let mut claim = claim();
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin-node".into(),
            vec!["home".into()],
        )
        .unwrap();
        let input = Input {
            schema_version: 1,
            original_text: "Check status".into(),
        };
        claim.source_sha256 = input.digest(&context).unwrap();
        Checkpoint {
            schema_version: 1,
            report_type: "coordinator_input_interpretation".into(),
            success: true,
            id: claim.id.clone(),
            revision: 2,
            status: "review".into(),
            operation_id: claim.operation_id.clone(),
            record: Record {
                schema_version: 1,
                claim,
                input,
                request_context: context,
                claimed_at_ms: 1,
                state: State::Claimed,
                promoted_revision: None,
                proposal_sha256: None,
            },
            reused: false,
            draft: None,
            inference_authorized: false,
            execution_authorized: false,
            dispatched: false,
        }
    }
    #[test]
    fn unknown_legacy_provider_never_becomes_a_wire_inference_selection() {
        let mut old = json!(claim());
        old.as_object_mut().unwrap().remove("interpreter");
        let parsed: Claim = serde_json::from_value(old.clone()).unwrap();
        parsed.validate().unwrap();
        assert!(parsed.interpreter.is_none());
        assert!(
            ClaimCommand {
                schema_version: 1,
                claim: parsed
            }
            .validate()
            .is_err()
        );
        old["execute"] = json!(true);
        assert!(serde_json::from_value::<Claim>(old).is_err());
    }
    #[test]
    fn checkpoint_rejects_forged_authority_source_and_attempt_bindings() {
        let good = checkpoint();
        let claim = good.record.claim.clone();
        good.validate(&claim, "origin-node").unwrap();
        for field in ["inference_authorized", "execution_authorized", "dispatched"] {
            let mut altered = json!(good);
            altered[field] = json!(true);
            assert!(
                serde_json::from_value::<Checkpoint>(altered)
                    .unwrap()
                    .validate(&claim, "origin-node")
                    .is_err()
            );
        }
        let mut changed = good.clone();
        changed.record.input.original_text.push('?');
        assert!(changed.validate(&claim, "origin-node").is_err());
        let mut other = claim.clone();
        other.interpreter.as_mut().unwrap().model_id = "replacement".into();
        assert!(good.validate(&other, "origin-node").is_err());
        assert!(good.validate(&claim, "stranger").is_err());
        let mut changed = good;
        changed.record.state = State::Promoted;
        assert!(changed.validate(&claim, "origin-node").is_err());
    }
    #[test]
    fn unknown_attempt_is_readable_without_any_completion_or_model_authorization() {
        let mut report = checkpoint();
        report.record.state = State::Unknown;
        report.reused = true;
        let claim = report.record.claim.clone();
        report.validate(&claim, "origin-node").unwrap();
        report.record.proposal_sha256 = Some("0".repeat(64));
        assert!(report.validate(&claim, "origin-node").is_err());
        let mut invalid = claim;
        invalid.interpreter.as_mut().unwrap().backend = "unrecognized".into();
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn checkpoint_rejects_unknown_wire_fields_and_invalid_source_revisions() {
        let report = checkpoint();
        let mut extra = json!(report);
        extra["permissions"] = json!({});
        assert!(serde_json::from_value::<Checkpoint>(extra).is_err());
        let mut report = report;
        report.revision = report.record.claim.expected_revision;
        assert!(
            report
                .validate(&report.record.claim, "origin-node")
                .is_err()
        );
    }
}
