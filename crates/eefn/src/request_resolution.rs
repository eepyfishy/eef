//! Explicit clarification reinterpretation, never planning or execution authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveCommand {
    pub id: String,
    pub expected_revision: u64,
    pub operation_id: String,
}
impl ResolveCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        if self.expected_revision == 0 || self.expected_revision == u64::MAX {
            bail!("resolution needs an exact positive source revision")
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ResolveCommand,
    #[serde(default)]
    pub inference_timeout_seconds: Option<u64>,
}

/// The origin reports its interpreter selection. eef verifies source identity,
/// not semantic correctness or that a remote model really performed inference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionEvidence {
    pub schema_version: u32,
    pub source_revision: u64,
    pub source_sha256: String,
    pub source_proposal_count: usize,
    pub source_answer_count: usize,
    pub backend: String,
    pub model_id: String,
}
impl ResolutionEvidence {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.source_revision == 0
            || self.source_revision == u64::MAX
            || self.source_proposal_count == 0
            || self.source_proposal_count > 16
            || self.source_answer_count == 0
            || self.source_answer_count > 16
            || self.source_revision < (self.source_proposal_count + self.source_answer_count) as u64
            || self.source_sha256.len() != 64
            || !self
                .source_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !matches!(self.backend.as_str(), "ollama" | "llamacpp")
            || self.model_id.trim().is_empty()
            || self.model_id.len() > 256
            || self.model_id.chars().any(char::is_control)
        {
            bail!("invalid clarification resolution evidence")
        }
        Ok(())
    }
    pub fn source_digest(draft: &crate::request_drafts::DraftRecord) -> Result<String> {
        crate::grants::definition_digest(&json!(draft))
    }
    pub fn confirms_source(&self, draft: &crate::request_drafts::DraftRecord) -> bool {
        self.validate().is_ok()
            && self.source_proposal_count == draft.proposals.len()
            && self.source_answer_count == draft.answers.len()
            && draft.resolvable()
            && Self::source_digest(draft).is_ok_and(|digest| digest == self.source_sha256)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolutionReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: Option<String>,
    command: ResolveCommand,
    success: bool,
    coordinator_acknowledged: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    outcome_unknown: bool,
    proposal_saved: bool,
    reused: bool,
    interpretation_source: String,
    interpretation_context: String,
    saving_attempted: bool,
    model_inference_requested: bool,
    coordinator_operation_id: Option<String>,
    result: Option<Value>,
    error_code: Option<String>,
    note: String,
}
pub fn confirmed(report: &Value, node: &str, command: &ResolveCommand) -> bool {
    let Ok(report) = serde_json::from_value::<ResolutionReport>(report.clone()) else {
        return false;
    };
    let Some(data) = &report.result else {
        return false;
    };
    let Some(receipt) = &report.coordinator_operation_id else {
        return false;
    };
    let Some(runtime) = &report.runtime_id else {
        return false;
    };
    if !report.success
        || report.schema_version != 1
        || report.report_type != "request_resolution"
        || report.node_id != node
        || report.command != *command
        || command.validate().is_err()
        || crate::job_commands::validate_operation_id(runtime).is_err()
        || crate::job_commands::validate_operation_id(receipt).is_err()
        || !report.coordinator_acknowledged
        || report.execution_authorized
        || report.dispatched
        || report.automatically_retried
        || report.outcome_unknown
        || !report.proposal_saved
        || report.interpretation_source != "node_selected_interpreter"
        || report.interpretation_context != "coordinator_disclosed_network"
        || report.error_code.is_some()
        || report.note.len() > 2048
        || !crate::request_drafts::confirmed_report(data, node, receipt, Some(&command.id))
    {
        return false;
    }
    let Ok(draft) =
        serde_json::from_value::<crate::request_drafts::DraftRecord>(data["draft"].clone())
    else {
        return false;
    };
    let Some((index, revision)) = draft
        .proposals
        .iter()
        .enumerate()
        .find(|(_, p)| p.operation_id == command.operation_id)
    else {
        return false;
    };
    let Some(evidence) = &revision.resolution else {
        return false;
    };
    evidence.source_revision == command.expected_revision
        && data["revision"]
            .as_u64()
            .is_some_and(|revision| revision > command.expected_revision)
        && if report.reused {
            !report.saving_attempted
                && !report.model_inference_requested
                && receipt != &command.operation_id
        } else {
            report.saving_attempted
                && report.model_inference_requested
                && receipt == &command.operation_id
                && data["reused"] == false
                && data["status"] == "review"
                && data["revision"].as_u64() == command.expected_revision.checked_add(1)
                && index + 1 == draft.proposals.len()
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn answered() -> crate::request_drafts::DraftRecord {
        serde_json::from_value(json!({"schema_version":1,"original_text":"  Inspect that node\n",
            "proposals":[{"operation_id":uuid::Uuid::new_v4().to_string(),"allowed_capabilities":["system.info"],"at_ms":1,
                "proposal":{"schema_version":1,"intent":"information","goal":"Inspect node status","context_hints":[],"constraints":[],
                "suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot","needs_clarification":true,"clarification":"Which node?"}}],
            "answers":[{"operation_id":uuid::Uuid::new_v4().to_string(),"proposal_index":0,"question":"Which node?","text":"Bedroom node","at_ms":2}]})).unwrap()
    }
    fn evidence(draft: &crate::request_drafts::DraftRecord) -> ResolutionEvidence {
        ResolutionEvidence {
            schema_version: 1,
            source_revision: 2,
            source_sha256: ResolutionEvidence::source_digest(draft).unwrap(),
            source_proposal_count: 1,
            source_answer_count: 1,
            backend: "ollama".into(),
            model_id: "fixture".into(),
        }
    }
    #[test]
    fn clarification_prompt_and_evidence_preserve_original_answers_and_bounded_lineage() {
        let source = answered();
        let messages =
            crate::interpretation::clarification_messages(&source, &["system.info".into()])
                .unwrap();
        let turns = messages.as_array().unwrap();
        assert_eq!(turns[turns.len() - 2]["content"], source.original_text);
        assert!(
            turns.last().unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("Bedroom node")
        );
        assert!(
            !messages
                .to_string()
                .contains(&source.answers[0].operation_id)
        );
        let proof = evidence(&source);
        assert!(proof.confirms_source(&source));
        let mut retained = source.clone();
        let mut proposal = source.current().clone();
        proposal.needs_clarification = false;
        proposal.clarification = None;
        retained
            .proposals
            .push(crate::request_drafts::ProposalRevision {
                operation_id: uuid::Uuid::new_v4().to_string(),
                proposal,
                allowed_capabilities: vec!["system.info".into()],
                at_ms: 3,
                resolution: Some(proof),
                preparation: None,
            });
        retained.validate().unwrap();
        assert!(!retained.resolvable());
        for field in ["original", "answer", "count", "digest", "model", "revision"] {
            let mut changed = retained.clone();
            match field {
                "original" => changed.original_text.push_str(" changed"),
                "answer" => changed.answers[0].text.push_str(" changed"),
                "count" => {
                    changed.proposals[1]
                        .resolution
                        .as_mut()
                        .unwrap()
                        .source_proposal_count = 2
                }
                "digest" => {
                    changed.proposals[1]
                        .resolution
                        .as_mut()
                        .unwrap()
                        .source_sha256 = "a".repeat(64)
                }
                "model" => {
                    changed.proposals[1].resolution.as_mut().unwrap().backend = "automatic".into()
                }
                "revision" => {
                    changed.proposals[1]
                        .resolution
                        .as_mut()
                        .unwrap()
                        .source_revision = 1
                }
                _ => unreachable!(),
            }
            assert!(changed.validate().is_err(), "{field}");
        }
        let mut no_answer = source.clone();
        no_answer.answers.clear();
        assert!(crate::interpretation::clarification_messages(&no_answer, &[]).is_err());
        let mut huge = source.clone();
        for _ in 0..8 {
            let mut answer = huge.answers[0].clone();
            answer.operation_id = uuid::Uuid::new_v4().to_string();
            answer.text = "x".repeat(8192);
            huge.answers.push(answer);
        }
        huge.validate().unwrap();
        assert!(crate::interpretation::clarification_messages(&huge, &[]).is_err());
        let mut overflow = source;
        overflow.answers[0].proposal_index = usize::MAX;
        assert!(!overflow.resolvable());
        assert!(overflow.validate().is_err());
    }
    #[test]
    fn confirmed_resolution_distinguishes_new_save_from_read_only_receipt_and_never_authorizes() {
        let mut draft = answered();
        let command = ResolveCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 2,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        let proof = evidence(&draft);
        draft
            .proposals
            .push(crate::request_drafts::ProposalRevision {
                operation_id: command.operation_id.clone(),
                proposal: draft.current().clone(),
                allowed_capabilities: vec!["system.info".into()],
                at_ms: 3,
                resolution: Some(proof),
                preparation: None,
            });
        let value = json!({"schema_version":1,"report_type":"request_resolution","node_id":"node","runtime_id":uuid::Uuid::new_v4().to_string(),
            "command":command,"success":true,"coordinator_acknowledged":true,"execution_authorized":false,"dispatched":false,
            "automatically_retried":false,"outcome_unknown":false,"proposal_saved":true,"reused":false,
            "interpretation_source":"node_selected_interpreter","interpretation_context":"coordinator_disclosed_network",
            "saving_attempted":true,"model_inference_requested":true,"coordinator_operation_id":command.operation_id,"error_code":null,"note":"Review only",
            "result":{"schema_version":1,"report_type":"coordinator_request_draft","id":command.id,"revision":3,"status":"review","operation_id":command.operation_id,
            "request_context":{"request_id":draft.proposals[0].operation_id,"origin_node":"node","origin_area":[]},"draft":draft,"review_state":draft.state(),
            "reused":false,"execution_authorized":false,"dispatched":false,"persisted":true,"note":"Review only"}});
        assert!(confirmed(&value, "node", &command));
        for (pointer, replacement) in [
            ("/execution_authorized", json!(true)),
            ("/dispatched", json!(true)),
            ("/automatically_retried", json!(true)),
            ("/interpretation_context", json!("local_node")),
            ("/proposal_saved", json!(false)),
            ("/outcome_unknown", json!(true)),
            ("/saving_attempted", json!(false)),
            ("/result/request_context/origin_node", json!("other")),
            ("/result/revision", json!(4)),
            ("/result/status", json!("running")),
            ("/result/draft/answers/0/text", json!("different")),
            ("/command/expected_revision", json!(3)),
            (
                "/coordinator_operation_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(!confirmed(&bad, "node", &command), "{pointer}");
        }
        let mut reused = value.clone();
        let inspection = uuid::Uuid::new_v4().to_string();
        reused["reused"] = json!(true);
        reused["saving_attempted"] = json!(false);
        reused["model_inference_requested"] = json!(false);
        reused["coordinator_operation_id"] = json!(inspection);
        reused["result"]["operation_id"] = json!(inspection);
        assert!(confirmed(&reused, "node", &command));
        reused["model_inference_requested"] = json!(true);
        assert!(!confirmed(&reused, "node", &command));
        let mut extra = value;
        extra["automatic_approval"] = json!(false);
        assert!(!confirmed(&extra, "node", &command));
    }
}
