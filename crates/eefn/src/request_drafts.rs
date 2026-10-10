//! Shared durable review contract. A draft/answer is never execution authority.
use crate::interpretation::Proposal;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRevision {
    pub operation_id: String,
    pub proposal: Proposal,
    pub allowed_capabilities: Vec<String>,
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<crate::request_resolution::ResolutionEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<crate::request_preparation::PreparationSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationAnswer {
    pub operation_id: String,
    pub proposal_index: usize,
    pub question: String,
    pub text: String,
    pub at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRecord {
    pub schema_version: u32,
    pub original_text: String,
    /// Immutable caller-supplied template input, never model-authored authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concrete_input: Option<crate::request_planning::PlanInput>,
    pub proposals: Vec<ProposalRevision>,
    pub answers: Vec<ClarificationAnswer>,
}
impl DraftRecord {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.proposals.is_empty()
            || self.proposals.len() > 17
            || self.answers.len() > 16
            || serde_json::to_vec(self)?.len() > 512 * 1024
        {
            bail!("durable request review exceeds bounds")
        }
        let mut receipts = std::collections::HashSet::new();
        if let Some(input) = &self.concrete_input {
            input.validate()?;
            if !crate::request_planning::METHOD_TEMPLATES.contains(&input.template.as_str()) {
                bail!("Unsupported explicit one-step request template")
            }
            let steps = crate::request_planning::template_steps(&input.template)?;
            if self.proposals[0].proposal.suggested_capabilities != vec![steps[0].0.to_string()] {
                bail!("Explicit input differs from its retained capability")
            }
        }
        for (index, revision) in self.proposals.iter().enumerate() {
            crate::job_commands::validate_operation_id(&revision.operation_id)?;
            if !receipts.insert(&revision.operation_id) {
                bail!("duplicate review receipt")
            }
            crate::interpretation::validate(
                &self.original_text,
                &serde_json::to_vec(&revision.proposal)?,
                &revision.allowed_capabilities,
            )?;
            if let Some(evidence) = &revision.resolution {
                evidence.validate()?;
                if evidence.source_proposal_count != index
                    || evidence.source_answer_count > self.answers.len()
                {
                    bail!("resolution source lineage exceeds retained history")
                }
                let source = Self {
                    schema_version: self.schema_version,
                    original_text: self.original_text.clone(),
                    concrete_input: self.concrete_input.clone(),
                    proposals: self.proposals[..index].to_vec(),
                    answers: self.answers[..evidence.source_answer_count].to_vec(),
                };
                if !evidence.confirms_source(&source) {
                    bail!("resolution source digest or clarification evidence changed")
                }
            }
            if revision.preparation.is_some() {
                crate::request_preparation::validate_question_lineage(self, index)?;
            }
        }
        for answer in &self.answers {
            crate::job_commands::validate_operation_id(&answer.operation_id)?;
            validate_answer(&answer.text)?;
            if !receipts.insert(&answer.operation_id)
                || self
                    .proposals
                    .get(answer.proposal_index)
                    .and_then(|r| r.proposal.clarification.as_deref())
                    != Some(answer.question.as_str())
            {
                bail!("invalid clarification answer receipt or question")
            }
        }
        Ok(())
    }
    pub fn current(&self) -> &Proposal {
        &self.proposals.last().expect("validated draft").proposal
    }
    pub fn resolvable(&self) -> bool {
        self.proposals
            .last()
            .is_some_and(|p| p.proposal.needs_clarification)
            && self
                .answers
                .iter()
                .any(|a| a.proposal_index.checked_add(1) == Some(self.proposals.len()))
    }
    pub fn state(&self) -> &'static str {
        let Some(current) = self.proposals.last().map(|p| &p.proposal) else {
            return "review_invalid";
        };
        if !current.needs_clarification {
            "review_required"
        } else if self
            .answers
            .iter()
            .any(|a| a.proposal_index == self.proposals.len() - 1)
        {
            "answer_recorded"
        } else {
            "clarification_required"
        }
    }
    pub fn same_opening(&self, other: &Self) -> bool {
        self.original_text == other.original_text
            && self.concrete_input == other.concrete_input
            && self.proposals.first().map(|p| &p.proposal)
                == other.proposals.first().map(|p| &p.proposal)
    }
}
pub fn validate_answer(text: &str) -> Result<()> {
    if text.trim().is_empty()
        || text.len() > 8192
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        bail!("clarification answer must contain 1-8192 bytes without unsupported controls")
    }
    Ok(())
}
pub fn validate_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        bail!("draft ID must be a lowercase compact UUID")
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftCommand {
    Get {
        id: String,
    },
    Answer {
        id: String,
        expected_revision: u64,
        text: String,
    },
    Revise {
        id: String,
        expected_revision: u64,
        original_text: String,
        proposal: Proposal,
    },
    Resolve {
        id: String,
        expected_revision: u64,
        original_text: String,
        proposal: Proposal,
        evidence: crate::request_resolution::ResolutionEvidence,
    },
}
impl DraftCommand {
    pub fn id(&self) -> &str {
        match self {
            Self::Get { id }
            | Self::Answer { id, .. }
            | Self::Revise { id, .. }
            | Self::Resolve { id, .. } => id,
        }
    }
    pub fn mutates(&self) -> bool {
        !matches!(self, Self::Get { .. })
    }
    pub fn submission(&self) -> Result<(&'static str, Value)> {
        validate_id(self.id())?;
        match self {
            Self::Get { id } => Ok(("requests.draft_get", json!({"id":id}))),
            Self::Answer {
                id,
                expected_revision,
                text,
            } => {
                validate_answer(text)?;
                Ok((
                    "requests.draft_answer",
                    json!({"id":id,"expected_revision":expected_revision,"text":text}),
                ))
            }
            Self::Revise {
                id,
                expected_revision,
                original_text,
                proposal,
            } => {
                crate::interpretation::validate(
                    original_text,
                    &serde_json::to_vec(proposal)?,
                    &proposal.suggested_capabilities,
                )?;
                Ok((
                    "requests.draft_revise",
                    json!({"id":id,"expected_revision":expected_revision,"original_text":original_text,"proposal":proposal}),
                ))
            }
            Self::Resolve {
                id,
                expected_revision,
                original_text,
                proposal,
                evidence,
            } => {
                evidence.validate()?;
                if evidence.source_revision != *expected_revision {
                    bail!("resolution source revision mismatch")
                }
                crate::interpretation::validate(
                    original_text,
                    &serde_json::to_vec(proposal)?,
                    &proposal.suggested_capabilities,
                )?;
                Ok((
                    "requests.draft_resolve",
                    json!({"id":id,"expected_revision":expected_revision,
                    "original_text":original_text,"proposal":proposal,"evidence":evidence}),
                ))
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub command: DraftCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftReport {
    schema_version: u32,
    report_type: String,
    id: String,
    revision: u64,
    status: String,
    operation_id: String,
    request_context: crate::context::RequestContext,
    draft: DraftRecord,
    review_state: String,
    reused: bool,
    execution_authorized: bool,
    dispatched: bool,
    persisted: bool,
    note: String,
}
pub fn confirmed_report(value: &Value, node: &str, operation_id: &str, id: Option<&str>) -> bool {
    let Ok(report) = serde_json::from_value::<DraftReport>(value.clone()) else {
        return false;
    };
    report.schema_version == 1
        && report.report_type == "coordinator_request_draft"
        && validate_id(&report.id).is_ok()
        && id.is_none_or(|id| id == report.id)
        && report.revision > 0
        && matches!(
            report.status.as_str(),
            "review"
                | "awaiting_approval"
                | "approved"
                | "cancelled"
                | "running"
                | "stopping"
                | "completed"
                | "failed"
                | "interrupted"
                | "abandoned"
        )
        && report.operation_id == operation_id
        && report.request_context.origin_node() == node
        && crate::job_commands::validate_operation_id(report.request_context.request_id()).is_ok()
        && report
            .draft
            .proposals
            .first()
            .is_some_and(|p| p.operation_id == report.request_context.request_id())
        && report.draft.validate().is_ok()
        && report.review_state == report.draft.state()
        && !report.execution_authorized
        && !report.dispatched
        && report.persisted
        && report.note.len() <= 2048
        && value["reused"] == json!(report.reused)
}
pub fn confirmed_command(
    value: &Value,
    node: &str,
    operation_id: &str,
    command: &DraftCommand,
) -> bool {
    if !confirmed_report(value, node, operation_id, Some(command.id())) {
        return false;
    }
    match command {
        DraftCommand::Get { .. } => true,
        DraftCommand::Answer {
            text,
            expected_revision,
            ..
        } => {
            (value["reused"] == true
                || expected_revision
                    .checked_add(1)
                    .is_some_and(|r| value["revision"] == r))
                && value["draft"]["answers"].as_array().is_some_and(|answers| {
                    answers
                        .iter()
                        .any(|a| a["operation_id"] == operation_id && a["text"] == *text)
                })
        }
        DraftCommand::Revise {
            original_text,
            proposal,
            expected_revision,
            ..
        }
        | DraftCommand::Resolve {
            original_text,
            proposal,
            expected_revision,
            ..
        } => {
            (value["reused"] == true
                || expected_revision
                    .checked_add(1)
                    .is_some_and(|r| value["revision"] == r))
                && value["draft"]["original_text"] == *original_text
                && value["draft"]["proposals"]
                    .as_array()
                    .is_some_and(|proposals| {
                        proposals.iter().any(|p| {
                            p["operation_id"] == operation_id
                                && p["proposal"] == json!(proposal)
                                && match command {
                                    DraftCommand::Resolve { evidence, .. } => {
                                        p["resolution"] == json!(evidence)
                                    }
                                    _ => p.get("resolution").is_none_or(Value::is_null),
                                }
                        })
                    })
        }
    }
}

impl crate::NodeService {
    pub async fn draft_command(&self, request: DraftRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("draft command schema or target mismatch")
        }
        crate::job_commands::validate_operation_id(&request.request_id)?;
        let (kind, payload) = request.command.submission()?;
        let mut report = json!({"schema_version":1,"report_type":"request_draft","node_id":self.node_id,
            "request_id":request.request_id,"success":false,"execution_authorized":false,"dispatched":false,
            "automatically_retried":false,"coordinator_acknowledged":false,"outcome_unknown":false});
        match self
            .submissions
            .request_correlated(kind, payload, Some(request.request_id.clone()))
            .await
        {
            Ok(data)
                if confirmed_command(
                    &data,
                    &self.node_id,
                    &request.request_id,
                    &request.command,
                ) =>
            {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = data;
            }
            Err(error)
                if error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                    .is_some() =>
            {
                let not_sent = matches!(
                    error.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                );
                report["error_code"] = json!(if not_sent {
                    "not_sent"
                } else {
                    "draft_rejected"
                });
                report["outcome_unknown"] = json!(!not_sent && request.command.mutates());
                report["error"] = json!(error.to_string());
            }
            _ => {
                report["error_code"] = json!("draft_unconfirmed");
                report["outcome_unknown"] = json!(request.command.mutates());
            }
        }
        report["note"] = json!(
            "Review/answers only, not executable work. No automatic replay. Inspect the draft or jobs.find after an unconfirmed mutation."
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id() -> String {
        uuid::Uuid::new_v4().simple().to_string()
    }
    fn receipt() -> String {
        uuid::Uuid::new_v4().to_string()
    }
    fn report(operation: &str) -> Value {
        json!({"schema_version":1,"report_type":"coordinator_request_draft","id":id(),"revision":2,
            "status":"review","operation_id":operation,"request_context":{"request_id":receipt(),"origin_node":"origin","origin_area":[]},
            "draft":{"schema_version":1,"original_text":"  Check that node\n","proposals":[],"answers":[]},
            "review_state":"answer_recorded","reused":false,"execution_authorized":false,"dispatched":false,"persisted":true,"note":"Review only"})
    }
    fn answered_report(operation: &str) -> Value {
        let mut v = report(operation);
        v["draft"]["proposals"] = json!([{"operation_id":v["request_context"]["request_id"],"at_ms":1,"allowed_capabilities":["system.info"],
            "proposal":{"schema_version":1,"intent":"information","goal":"Check node","context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],
                "complexity":"simple","workload":"one_shot","needs_clarification":true,"clarification":"Which node?"}}]);
        v["draft"]["answers"] = json!([{"operation_id":operation,"proposal_index":0,"question":"Which node?","text":"Bedroom","at_ms":2}]);
        v
    }
    #[test]
    fn confirmation_binds_identity_receipt_delta_revision_and_safety() {
        let operation = receipt();
        let good = answered_report(&operation);
        let command = DraftCommand::Answer {
            id: good["id"].as_str().unwrap().into(),
            expected_revision: 1,
            text: "Bedroom".into(),
        };
        assert!(confirmed_command(&good, "origin", &operation, &command));
        assert!(!confirmed_command(&good, "stranger", &operation, &command));
        assert!(!confirmed_command(&good, "origin", &receipt(), &command));
        for pointer in [
            "/execution_authorized",
            "/dispatched",
            "/persisted",
            "/status",
            "/revision",
            "/operation_id",
            "/request_context/origin_node",
            "/draft/answers/0/operation_id",
            "/draft/answers/0/question",
            "/draft/answers/0/proposal_index",
            "/draft/answers/0/text",
            "/draft/proposals/0/operation_id",
            "/review_state",
        ] {
            let mut bad = good.clone();
            let field = bad.pointer_mut(pointer).unwrap();
            *field = if field.is_boolean() {
                json!(!field.as_bool().unwrap())
            } else if field.is_number() {
                json!(999)
            } else {
                json!("corrupted")
            };
            assert!(
                !confirmed_command(&bad, "origin", &operation, &command),
                "{pointer}"
            );
        }
        let mut bad = good.clone();
        bad["grant"] = json!({});
        assert!(!confirmed_command(&bad, "origin", &operation, &command));
        let mut reused = good.clone();
        reused["reused"] = json!(true);
        reused["revision"] = json!(30);
        reused["status"] = json!("cancelled");
        assert!(
            confirmed_command(&reused, "origin", &operation, &command),
            "a retained receipt acknowledges a past answer, not resumed work"
        );
    }
    #[test]
    fn strict_shapes_bounds_and_question_references_fail_closed() {
        let operation = receipt();
        let good = answered_report(&operation);
        let draft: DraftRecord = serde_json::from_value(good["draft"].clone()).unwrap();
        assert!(draft.validate().is_ok());
        let mut bad = draft.clone();
        bad.proposals.clear();
        assert!(bad.validate().is_err());
        assert_eq!(bad.state(), "review_invalid");
        let mut bad = draft.clone();
        bad.answers[0].proposal_index = usize::MAX;
        assert!(bad.validate().is_err());
        assert_eq!(bad.state(), "clarification_required");
        let mut bad = draft.clone();
        bad.answers[0].operation_id = bad.proposals[0].operation_id.clone();
        assert!(bad.validate().is_err());
        let mut bad = draft.clone();
        bad.answers[0].text = "x".repeat(8193);
        assert!(bad.validate().is_err());
        let mut bad = draft.clone();
        bad.answers = vec![bad.answers[0].clone(); 17];
        assert!(bad.validate().is_err());
        let mut bad = draft.clone();
        bad.proposals = vec![bad.proposals[0].clone(); 18];
        assert!(bad.validate().is_err());
        assert!(validate_answer(" \t ").is_err());
        assert!(validate_answer("a\0b").is_err());
        assert!(validate_answer("line1\nline2\t").is_ok());
        for v in [
            json!({"operation":"answer","id":id(),"expected_revision":1,"text":"answer","permissions":true}),
            json!({"operation":"answer","id":id(),"text":"answer"}),
            json!({"operation":"get","id":id(),"expected_revision":1}),
        ] {
            assert!(serde_json::from_value::<DraftCommand>(v).is_err());
        }
    }
    #[tokio::test]
    async fn mailbox_commands_confirm_once_and_never_replay_bad_or_lost_replies() {
        let dir = tempfile::tempdir().unwrap();
        let node = crate::NodeService::new(dir.path().join("node.json"), "origin".into());
        let mut receiver = node.submissions.connect();
        for mode in ["good", "corrupt", "lost"] {
            let operation = receipt();
            let good = answered_report(&operation);
            let request = DraftRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                request_id: operation.clone(),
                command: DraftCommand::Answer {
                    id: good["id"].as_str().unwrap().into(),
                    expected_revision: 1,
                    text: "Bedroom".into(),
                },
            };
            let local = node.clone();
            let future = tokio::spawn(async move { local.draft_command(request).await.unwrap() });
            let submission = receiver.recv().await.unwrap();
            assert_eq!(submission.kind, "requests.draft_answer");
            assert_eq!(submission.operation_id.as_deref(), Some(operation.as_str()));
            assert_ne!(submission.id, operation);
            assert_eq!(submission.payload["expected_revision"], 1);
            if mode == "lost" {
                drop(submission.reply);
            } else {
                let mut reply = good;
                if mode == "corrupt" {
                    reply["execution_authorized"] = json!(true);
                }
                submission.reply.send(Ok(reply)).unwrap();
            }
            let result = future.await.unwrap();
            assert_eq!(result["success"], mode == "good");
            assert_eq!(result["outcome_unknown"], mode != "good");
            assert_eq!(result["automatically_retried"], false);
            assert!(receiver.try_recv().is_err());
        }
        let invalid = DraftRequest {
            schema_version: 1,
            expected_node_id: "other".into(),
            request_id: receipt(),
            command: DraftCommand::Get { id: id() },
        };
        assert!(node.draft_command(invalid).await.is_err());
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        let _empty = node.submissions.connect();
        drop(_empty);
        let result = node
            .draft_command(DraftRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                request_id: receipt(),
                command: DraftCommand::Answer {
                    id: id(),
                    expected_revision: 1,
                    text: "Bedroom".into(),
                },
            })
            .await
            .unwrap();
        assert_eq!(result["error_code"], "not_sent");
        assert_eq!(result["outcome_unknown"], false);
    }
}
