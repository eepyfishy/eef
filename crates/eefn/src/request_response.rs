//! Bounded answer-triggered workflow over existing source-bound journal commands.
//! No approval, execution, resume, replay or model/provider fallback.
use crate::{
    NodeService,
    request_drafts::{DraftCommand, DraftRecord, DraftRequest},
    request_input::InputPolicy,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespondCommand {
    pub id: String,
    pub expected_revision: u64,
    pub text: String,
    pub operation_id: String,
}
impl RespondCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::request_drafts::validate_answer(&self.text)?;
        if self.expected_revision == 0 || self.expected_revision > u64::MAX - 3 {
            bail!("Response requires a bounded exact source revision")
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespondRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: RespondCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_timeout_seconds: Option<u64>,
}

/// Separate logical receipts; not credentials or execution authority.
pub fn stage_receipt(
    prepare: bool,
    node: &str,
    command: &RespondCommand,
    revision: u64,
) -> Result<String> {
    command.validate()?;
    crate::network::validate_remote_node_id(node)?;
    if revision == 0 || revision == u64::MAX {
        bail!("Invalid response stage revision")
    }
    let hash = Sha256::digest(serde_json::to_vec(&json!([
        if prepare {
            "eef.answer.prepare.v1"
        } else {
            "eef.answer.resolve.v1"
        },
        node,
        command.operation_id,
        command.id,
        revision
    ]))?);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 15) | 128;
    bytes[8] = (bytes[8] & 63) | 128;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}

fn draft_report<'a>(value: &'a Value, node: &str, id: &str) -> Option<&'a Value> {
    let receipt = value["request_id"].as_str()?;
    if value["schema_version"] != 1
        || value["report_type"] != "request_draft"
        || value["node_id"] != node
        || value["success"] != true
        || value["coordinator_acknowledged"] != true
        || value["outcome_unknown"] != false
        || value["execution_authorized"] != false
        || value["dispatched"] != false
        || value["automatically_retried"] != false
        || !crate::request_drafts::confirmed_report(&value["result"], node, receipt, Some(id))
    {
        return None;
    }
    Some(&value["result"])
}
fn retained_answer(data: &Value, command: &RespondCommand) -> bool {
    let Ok(draft) = serde_json::from_value::<DraftRecord>(data["draft"].clone()) else {
        return false;
    };
    draft.answers.iter().any(|a| {
        a.operation_id == command.operation_id
            && a.text == command.text
            && a.source
                .as_ref()
                .is_some_and(|s| s.revision == command.expected_revision)
    })
}
fn matched_answer_source(before: &Value, after: &Value, command: &RespondCommand) -> bool {
    let Ok(a) = serde_json::from_value::<DraftRecord>(before["draft"].clone()) else {
        return false;
    };
    let Ok(b) = serde_json::from_value::<DraftRecord>(after["draft"].clone()) else {
        return false;
    };
    before["id"] == after["id"]
        && before["request_context"] == after["request_context"]
        && b.answers.iter().any(|answer| {
            answer.operation_id == command.operation_id
                && answer.text == command.text
                && answer
                    .source
                    .as_ref()
                    .is_some_and(|s| s.revision == command.expected_revision && s.confirms(&a))
        })
}
fn answer_transition(before: &Value, after: &Value, command: &RespondCommand) -> bool {
    let Ok(a) = serde_json::from_value::<DraftRecord>(before["draft"].clone()) else {
        return false;
    };
    let Ok(b) = serde_json::from_value::<DraftRecord>(after["draft"].clone()) else {
        return false;
    };
    let Some(answer) = b.answers.last() else {
        return false;
    };
    before["status"] == "review"
        && before["revision"] == command.expected_revision
        && after["status"] == "review"
        && after["revision"].as_u64() == command.expected_revision.checked_add(1)
        && after["reused"] == false
        && before["id"] == after["id"]
        && before["request_context"] == after["request_context"]
        && a.original_text == b.original_text
        && json!(a.concrete_input) == json!(b.concrete_input)
        && json!(a.proposals) == json!(b.proposals)
        && b.answers.len() == a.answers.len() + 1
        && json!(&b.answers[..a.answers.len()]) == json!(a.answers)
        && answer.operation_id == command.operation_id
        && answer.text == command.text
        && answer
            .source
            .as_ref()
            .is_some_and(|s| s.revision == command.expected_revision && s.confirms(&a))
}
fn resolution_source(
    resolved: &Value,
    answered: &Value,
    command: &crate::request_resolution::ResolveCommand,
) -> bool {
    let Ok(a) = serde_json::from_value::<DraftRecord>(answered["draft"].clone()) else {
        return false;
    };
    let Ok(b) = serde_json::from_value::<DraftRecord>(resolved["result"]["draft"].clone()) else {
        return false;
    };
    let Some(p) = b
        .proposals
        .iter()
        .find(|p| p.operation_id == command.operation_id)
    else {
        return false;
    };
    p.resolution
        .as_ref()
        .is_some_and(|e| e.source_revision == command.expected_revision && e.confirms_source(&a))
        && resolved["result"]["request_context"] == answered["request_context"]
        && a.original_text == b.original_text
        && b.proposals.len() > a.proposals.len()
        && json!(&b.proposals[..a.proposals.len()]) == json!(a.proposals)
        && b.answers.len() >= a.answers.len()
        && json!(&b.answers[..a.answers.len()]) == json!(a.answers)
}

impl NodeService {
    pub async fn respond_request(&self, request: RespondRequest) -> Value {
        let mut report = json!({"schema_version":1,"report_type":"node_answer_workflow","node_id":self.node_id,"runtime_id":null,
            "command":request.command,"success":false,"status":"answer_not_confirmed","coordinator_acknowledged":false,
            "source_confirmed":false,"answer_attempted":false,"answer_confirmed":false,"answer_reused":false,"policy":null,
            "source":null,"answer":null,"resolution_command":null,"resolution":null,"preparation_command":null,"preparation":null,
            "result":null,"error_code":null,"outcome_unknown":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false,
            "note":"Answer/resolution/preparation only. Preserve this command and derived receipts after uncertainty; reuse only inspects history. No approval, execution, automatic retry or continuation."});
        if request.schema_version != 1
            || crate::inference_budget::InferenceBudget::new(request.inference_timeout_seconds)
                .is_err()
            || request.expected_node_id != self.node_id
            || request.command.validate().is_err()
        {
            report["error_code"] = json!("invalid_response_request");
            return report;
        }
        let Some((runtime, policy, mut generation)) = self.input_workflow_runtime() else {
            report["error_code"] = json!("runtime_unavailable");
            return report;
        };
        report["runtime_id"] = json!(runtime);
        report["policy"] = json!(policy);
        if generation.borrow_and_update().as_ref() != Some(&runtime) {
            report["error_code"] = json!("runtime_changed");
            return report;
        }
        let _permit = match self.input_workflow_permit() {
            Ok(p) => p,
            Err(code) => {
                report["error_code"] = json!(code);
                return report;
            }
        };
        let budget =
            crate::inference_budget::InferenceBudget::new(request.inference_timeout_seconds)
                .expect("validated inference budget");
        let result = tokio::select! {
            biased;
            _=generation.changed()=>Err("runtime_changed"),
            result=tokio::time::timeout(Duration::from_secs(budget.extend_seconds(180,2)),self.respond_inner(&request.command,policy,request.inference_timeout_seconds,&mut report))=>result.unwrap_or(Err("answer_workflow_timeout")),
        };
        if let Err(code) = result {
            report["success"] = json!(false);
            report["error_code"] = json!(code);
            if matches!(code, "runtime_changed" | "answer_workflow_timeout") {
                report["outcome_unknown"] = json!(
                    report["answer_attempted"] == true
                        || !report["resolution_command"].is_null()
                        || !report["preparation_command"].is_null()
                );
            }
        }
        report["coordinator_acknowledged"] = json!(report["success"] == true);
        if !confirmed(&report, &self.node_id, &request.command) {
            report["success"] = json!(false);
            report["coordinator_acknowledged"] = json!(false);
            report["error_code"] = json!("answer_report_unconfirmed");
            report["outcome_unknown"] = json!(
                report["answer_attempted"] == true
                    || !report["resolution_command"].is_null()
                    || !report["preparation_command"].is_null()
            );
        }
        report
    }

    async fn respond_inner(
        &self,
        command: &RespondCommand,
        policy: InputPolicy,
        inference_timeout_seconds: Option<u64>,
        report: &mut Value,
    ) -> std::result::Result<(), &'static str> {
        let source = self
            .draft_command(DraftRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                request_id: uuid::Uuid::new_v4().to_string(),
                command: DraftCommand::Get {
                    id: command.id.clone(),
                },
            })
            .await
            .map_err(|_| "answer_source_unconfirmed")?;
        let before = draft_report(&source, &self.node_id, &command.id)
            .ok_or("answer_source_unconfirmed")?
            .clone();
        let draft: DraftRecord = serde_json::from_value(before["draft"].clone())
            .map_err(|_| "answer_source_unconfirmed")?;
        report["source"] = source;
        report["source_confirmed"] = json!(true);
        report["result"] = before.clone();
        if draft
            .answers
            .iter()
            .any(|a| a.operation_id == command.operation_id)
        {
            if !retained_answer(&before, command) {
                return Err("answer_receipt_conflict");
            }
            report["answer_confirmed"] = json!(true);
            report["answer_reused"] = json!(true);
            report["status"] = json!("historical_snapshot");
            report["success"] = json!(true);
            return Ok(());
        }
        if draft
            .proposals
            .iter()
            .any(|p| p.operation_id == command.operation_id)
        {
            return Err("answer_receipt_conflict");
        }
        if before["status"] != "review" || before["revision"] != command.expected_revision {
            return Err("answer_source_revision_changed");
        }
        if !draft.current().needs_clarification {
            return Err("clarification_question_required");
        }
        if draft.answers.len() >= 16 {
            return Err("answer_history_full");
        }
        report["answer_attempted"] = json!(true);
        let answer = self
            .draft_command(DraftRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                request_id: command.operation_id.clone(),
                command: DraftCommand::Answer {
                    id: command.id.clone(),
                    expected_revision: command.expected_revision,
                    text: command.text.clone(),
                },
            })
            .await
            .map_err(|_| "answer_save_unconfirmed")?;
        report["answer"] = answer.clone();
        report["outcome_unknown"] = answer["outcome_unknown"].clone();
        if answer["success"] == true {
            report["outcome_unknown"] = json!(true);
        }
        let answered = draft_report(&answer, &self.node_id, &command.id)
            .ok_or("answer_save_unconfirmed")?
            .clone();
        if answer["request_id"] != command.operation_id
            || !retained_answer(&answered, command)
            || answered["request_context"] != before["request_context"]
        {
            return Err("answer_save_unconfirmed");
        }
        if answered["reused"] == true {
            if !matched_answer_source(&before, &answered, command) {
                return Err("answer_save_unconfirmed");
            }
            report["answer_confirmed"] = json!(true);
            report["answer_reused"] = json!(true);
            report["result"] = answered;
            report["status"] = json!("historical_snapshot");
            report["success"] = json!(true);
            report["outcome_unknown"] = json!(false);
            return Ok(());
        }
        if !answer_transition(&before, &answered, command) {
            report["outcome_unknown"] = json!(true);
            return Err("answer_save_unconfirmed");
        }
        report["answer_confirmed"] = json!(true);
        report["result"] = answered.clone();
        report["status"] = json!("answer_recorded");
        report["outcome_unknown"] = json!(false);
        if policy.resolve_on_answer {
            let revision = answered["revision"]
                .as_u64()
                .ok_or("answer_save_unconfirmed")?;
            let resolution_command = crate::request_resolution::ResolveCommand {
                id: command.id.clone(),
                expected_revision: revision,
                operation_id: stage_receipt(false, &self.node_id, command, revision)
                    .map_err(|_| "answer_save_unconfirmed")?,
            };
            report["resolution_command"] = json!(resolution_command);
            report["status"] = json!("resolution_not_confirmed");
            let resolved = self
                .resolve_request(crate::request_resolution::ResolveRequest {
                    schema_version: 1,
                    expected_node_id: self.node_id.clone(),
                    command: resolution_command.clone(),
                    inference_timeout_seconds,
                })
                .await;
            report["resolution"] = resolved.clone();
            report["outcome_unknown"] = resolved["outcome_unknown"].clone();
            if resolved["success"] == true {
                report["outcome_unknown"] = json!(true);
            }
            if !crate::request_resolution::confirmed(&resolved, &self.node_id, &resolution_command)
                || !resolution_source(&resolved, &answered, &resolution_command)
            {
                return Err("answer_resolution_not_confirmed");
            }
            report["result"] = resolved["result"].clone();
            report["outcome_unknown"] = json!(false);
            let current: DraftRecord = serde_json::from_value(resolved["result"]["draft"].clone())
                .map_err(|_| "answer_resolution_not_confirmed")?;
            if resolved["reused"] == true {
                report["status"] = json!("historical_snapshot");
            } else if current.current().needs_clarification {
                report["status"] = json!("clarification_required");
            } else if !policy.prepare_on_input
                || crate::request_preparation::eligible(&current).is_err()
            {
                report["status"] = json!("review_only");
            } else {
                let revision = resolved["result"]["revision"]
                    .as_u64()
                    .ok_or("answer_resolution_not_confirmed")?;
                let preparation_command = crate::request_preparation::PrepareCommand {
                    id: command.id.clone(),
                    expected_revision: revision,
                    operation_id: stage_receipt(true, &self.node_id, command, revision)
                        .map_err(|_| "answer_resolution_not_confirmed")?,
                };
                report["preparation_command"] = json!(preparation_command);
                report["status"] = json!("preparation_not_confirmed");
                let prepared = self
                    .prepare_request(crate::request_preparation::PrepareRequest {
                        schema_version: 1,
                        expected_node_id: self.node_id.clone(),
                        command: preparation_command.clone(),
                        inference_timeout_seconds,
                    })
                    .await;
                report["preparation"] = prepared.clone();
                report["outcome_unknown"] = prepared["outcome_unknown"].clone();
                if prepared["success"] == true {
                    report["outcome_unknown"] = json!(true);
                }
                if !crate::request_preparation::confirmed(
                    &prepared,
                    &self.node_id,
                    &preparation_command,
                ) || !crate::request_input::confirms_preparation_source(
                    &prepared,
                    &resolved["result"],
                    &preparation_command,
                ) {
                    return Err("answer_preparation_not_confirmed");
                }
                report["result"] = prepared["result"].clone();
                report["status"] = json!(if prepared["inspection_only"] == true {
                    "historical_snapshot"
                } else if prepared["result"]["status"] == "awaiting_approval" {
                    "awaiting_approval"
                } else {
                    "clarification_required"
                });
            }
        }
        report["success"] = json!(true);
        report["outcome_unknown"] = json!(false);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RespondReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: Option<String>,
    command: RespondCommand,
    success: bool,
    status: String,
    coordinator_acknowledged: bool,
    source_confirmed: bool,
    answer_attempted: bool,
    answer_confirmed: bool,
    answer_reused: bool,
    policy: Option<InputPolicy>,
    source: Option<Value>,
    answer: Option<Value>,
    resolution_command: Option<crate::request_resolution::ResolveCommand>,
    resolution: Option<Value>,
    preparation_command: Option<crate::request_preparation::PrepareCommand>,
    preparation: Option<Value>,
    result: Option<Value>,
    error_code: Option<String>,
    outcome_unknown: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, node: &str, command: &RespondCommand) -> bool {
    let Ok(r) = serde_json::from_value::<RespondReport>(value.clone()) else {
        return false;
    };
    if r.schema_version != 1
        || r.report_type != "node_answer_workflow"
        || r.node_id != node
        || r.command != *command
        || command.validate().is_err()
        || r.execution_authorized
        || r.dispatched
        || r.automatically_retried
        || r.note.len() > 2048
        || !matches!(
            r.status.as_str(),
            "answer_not_confirmed"
                | "answer_recorded"
                | "historical_snapshot"
                | "resolution_not_confirmed"
                | "preparation_not_confirmed"
                | "clarification_required"
                | "review_only"
                | "awaiting_approval"
        )
    {
        return false;
    }
    let source = r
        .source
        .as_ref()
        .and_then(|s| draft_report(s, node, &command.id));
    if r.source_confirmed != source.is_some() {
        return false;
    }
    let answer = r
        .answer
        .as_ref()
        .and_then(|a| draft_report(a, node, &command.id));
    if r.answer_confirmed {
        let Some(before) = source else { return false };
        if r.answer_reused {
            let retained = if r.answer_attempted {
                let Some(after) = answer else { return false };
                if r.answer.as_ref().unwrap()["request_id"] != command.operation_id
                    || after["reused"] != true
                    || !matched_answer_source(before, after, command)
                {
                    return false;
                }
                after
            } else {
                if r.answer.is_some() {
                    return false;
                }
                before
            };
            if !retained_answer(retained, command) || r.result.as_ref() != Some(retained) {
                return false;
            }
        } else {
            let Some(after) = answer else { return false };
            if !r.answer_attempted
                || r.answer.as_ref().unwrap()["request_id"] != command.operation_id
                || !answer_transition(before, after, command)
            {
                return false;
            }
        }
    } else if r.answer_reused {
        return false;
    }
    // A failed outer workflow may still carry confirmed earlier checkpoints.
    // Validate every claimed checkpoint and bind result to the last proven one.
    if let Some(c) = &r.resolution_command {
        if !r.answer_confirmed
            || r.answer_reused
            || !r.policy.is_some_and(|p| p.resolve_on_answer)
            || answer.is_none_or(|a| a["revision"] != c.expected_revision)
            || c.id != command.id
            || !stage_receipt(false, node, command, c.expected_revision)
                .is_ok_and(|id| id == c.operation_id)
        {
            return false;
        }
    } else if r.resolution.is_some() {
        return false;
    }
    let valid_resolution = match (&r.resolution_command, &r.resolution, answer) {
        (Some(c), Some(v), Some(a)) => {
            r.answer_confirmed
                && !r.answer_reused
                && r.policy.is_some_and(|p| p.resolve_on_answer)
                && c.id == command.id
                && a["revision"] == c.expected_revision
                && stage_receipt(false, node, command, c.expected_revision)
                    .is_ok_and(|id| id == c.operation_id)
                && crate::request_resolution::confirmed(v, node, c)
                && resolution_source(v, a, c)
                && v["runtime_id"].as_str() == r.runtime_id.as_deref()
        }
        _ => false,
    };
    if let Some(c) = &r.preparation_command {
        let Some(resolved) = &r.resolution else {
            return false;
        };
        if !valid_resolution
            || resolved["reused"] == true
            || !r.policy.is_some_and(|p| p.prepare_on_input)
            || resolved["result"]["revision"] != c.expected_revision
            || c.id != command.id
            || !stage_receipt(true, node, command, c.expected_revision)
                .is_ok_and(|id| id == c.operation_id)
            || !serde_json::from_value::<DraftRecord>(resolved["result"]["draft"].clone())
                .is_ok_and(|d| crate::request_preparation::eligible(&d).is_ok())
        {
            return false;
        }
    } else if r.preparation.is_some() {
        return false;
    }
    let valid_preparation = match (&r.preparation_command, &r.preparation, &r.resolution) {
        (Some(c), Some(v), Some(resolved)) => {
            valid_resolution
                && resolved["reused"] == false
                && r.policy.is_some_and(|p| p.prepare_on_input)
                && serde_json::from_value::<DraftRecord>(resolved["result"]["draft"].clone())
                    .is_ok_and(|d| crate::request_preparation::eligible(&d).is_ok())
                && c.id == command.id
                && resolved["result"]["revision"] == c.expected_revision
                && stage_receipt(true, node, command, c.expected_revision)
                    .is_ok_and(|id| id == c.operation_id)
                && crate::request_preparation::confirmed(v, node, c)
                && v["runtime_id"].as_str() == r.runtime_id.as_deref()
                && crate::request_input::confirms_preparation_source(v, &resolved["result"], c)
        }
        _ => false,
    };
    if !r.success {
        if r.resolution.as_ref().is_some_and(|v| v["success"] == true) && !valid_resolution
            || r.preparation.as_ref().is_some_and(|v| v["success"] == true) && !valid_preparation
        {
            return false;
        }
        let last = if valid_preparation {
            r.preparation.as_ref().map(|v| &v["result"])
        } else if valid_resolution {
            r.resolution.as_ref().map(|v| &v["result"])
        } else if r.answer_confirmed && r.answer_attempted {
            answer
        } else {
            source
        };
        return r.result.as_ref() == last
            && !r.coordinator_acknowledged
            && r.error_code
                .as_ref()
                .is_some_and(|e| !e.is_empty() && e.len() <= 128);
    }
    if !r.coordinator_acknowledged
        || !r.answer_confirmed
        || r.outcome_unknown
        || r.error_code.is_some()
        || r.runtime_id
            .as_ref()
            .is_none_or(|id| crate::job_commands::validate_operation_id(id).is_err())
    {
        return false;
    }
    let Some(policy) = r.policy else { return false };
    if r.answer_reused {
        return r.status == "historical_snapshot"
            && r.resolution.is_none()
            && r.resolution_command.is_none()
            && r.preparation.is_none()
            && r.preparation_command.is_none();
    }
    let after = answer.unwrap();
    let Some(_) = &r.resolution_command else {
        return !policy.resolve_on_answer
            && r.resolution.is_none()
            && r.preparation.is_none()
            && r.preparation_command.is_none()
            && r.status == "answer_recorded"
            && r.result.as_ref() == Some(after);
    };
    let Some(resolved) = &r.resolution else {
        return false;
    };
    if !valid_resolution {
        return false;
    }
    if resolved["reused"] == true {
        return r.status == "historical_snapshot"
            && r.result.as_ref() == Some(&resolved["result"])
            && r.preparation.is_none()
            && r.preparation_command.is_none();
    }
    let Ok(current) = serde_json::from_value::<DraftRecord>(resolved["result"]["draft"].clone())
    else {
        return false;
    };
    if let (Some(_), Some(prepared)) = (&r.preparation_command, &r.preparation) {
        valid_preparation
            && r.result.as_ref() == Some(&prepared["result"])
            && r.status
                == if prepared["inspection_only"] == true {
                    "historical_snapshot"
                } else if prepared["result"]["status"] == "awaiting_approval" {
                    "awaiting_approval"
                } else {
                    "clarification_required"
                }
    } else {
        r.preparation_command.is_none()
            && r.preparation.is_none()
            && r.result.as_ref() == Some(&resolved["result"])
            && if current.current().needs_clarification {
                r.status == "clarification_required"
            } else {
                r.status == "review_only"
                    && (!policy.prepare_on_input
                        || crate::request_preparation::eligible(&current).is_err())
            }
    }
}

pub fn reply(report: &Value) -> String {
    let id = report["command"]["id"].as_str().unwrap_or("unknown");
    if report["success"] != true {
        return format!(
            "Answer workflow not confirmed ({}). Answer save {}. Inspect request {id}, the exact response command and derived receipts before retrying. No approval, execution or automatic replay was requested.",
            report["error_code"]
                .as_str()
                .unwrap_or("answer_unconfirmed"),
            if report["answer_confirmed"] == true {
                "was confirmed, but continuation was not"
            } else {
                "is not confirmed"
            }
        );
    }
    match report["status"].as_str() {
        Some("answer_recorded") => format!(
            "Answer saved for request {id}. Automatic resolution is disabled; resolution, preparation, approval and execution remain separate steps."
        ),
        Some("historical_snapshot") => format!(
            "Retained response/history for request {id} inspected. No approval, execution or automatic replay was requested."
        ),
        _ => crate::request_input::input_reply(report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn command() -> RespondCommand {
        RespondCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 1,
            text: "  Bedroom node\n".into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn request(command: RespondCommand) -> RespondRequest {
        RespondRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            command,
            inference_timeout_seconds: None,
        }
    }
    fn source(command: &RespondCommand) -> Value {
        let input = uuid::Uuid::new_v4().to_string();
        json!({"schema_version":1,"report_type":"coordinator_request_draft","id":command.id,"revision":1,"status":"review",
            "operation_id":input,"request_context":{"request_id":input,"origin_node":"node","origin_area":["Home","Bedroom"]},
            "draft":{"schema_version":1,"original_text":"  Inspect that node\n","proposals":[{"operation_id":input,"at_ms":1,
                "allowed_capabilities":["system.info"],"proposal":{"schema_version":1,"intent":"information","goal":"Inspect node status",
                    "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot",
                    "needs_clarification":true,"clarification":"Which node?"}}],"answers":[]},
            "review_state":"clarification_required","reused":false,"execution_authorized":false,"dispatched":false,"persisted":true,"note":"Review only"})
    }
    fn answered(before: &Value, command: &RespondCommand) -> Value {
        let draft: DraftRecord = serde_json::from_value(before["draft"].clone()).unwrap();
        let proof =
            crate::request_drafts::AnswerSource::from_draft(command.expected_revision, &draft)
                .unwrap();
        let mut after = before.clone();
        after["operation_id"] = json!(command.operation_id);
        after["revision"] = json!(2);
        after["review_state"] = json!("answer_recorded");
        after["draft"]["answers"] = json!([{"operation_id":command.operation_id,"proposal_index":0,"question":"Which node?","text":command.text,"at_ms":2,"source":proof}]);
        after
    }
    fn envelope(data: Value) -> Value {
        json!({"schema_version":1,"report_type":"request_draft","node_id":"node","request_id":data["operation_id"],
        "success":true,"coordinator_acknowledged":true,"outcome_unknown":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false,"result":data})
    }
    async fn next(
        receiver: &mut tokio::sync::mpsc::Receiver<crate::submission::Submission>,
    ) -> crate::submission::Submission {
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap()
    }
    #[test]
    fn stage_receipts_are_separate_canonical_and_source_scoped() {
        let command = command();
        let resolution = stage_receipt(false, "node", &command, 2).unwrap();
        let preparation = stage_receipt(true, "node", &command, 3).unwrap();
        assert_ne!(resolution, preparation);
        assert_ne!(resolution, command.operation_id);
        assert!(crate::job_commands::validate_operation_id(&resolution).is_ok());
        assert_eq!(
            resolution,
            stage_receipt(false, "node", &command, 2).unwrap()
        );
        assert_ne!(
            resolution,
            stage_receipt(true, "node", &command, 2).unwrap()
        );
        assert_ne!(
            resolution,
            stage_receipt(false, "other", &command, 2).unwrap()
        );
        assert_ne!(
            resolution,
            stage_receipt(false, "node", &command, 3).unwrap()
        );
        assert!(stage_receipt(false, "node", &command, 0).is_err());
    }
    #[test]
    fn answer_source_digest_binds_complete_prefix_without_reconstructing_legacy() {
        let command = command();
        let before = source(&command);
        let after = answered(&before, &command);
        let retained: DraftRecord = serde_json::from_value(after["draft"].clone()).unwrap();
        assert!(retained.validate().is_ok());
        assert!(answer_transition(&before, &after, &command));
        for (pointer, value) in [
            ("/proposals/0/proposal/goal", json!("Changed")),
            ("/answers/0/source/sha256", json!("a".repeat(64))),
            ("/answers/0/source/answer_count", json!(1)),
            ("/answers/0/source/proposal_count", json!(0)),
            ("/answers/0/source/revision", json!(0)),
            ("/original_text", json!("Changed original")),
        ] {
            let mut bad = after["draft"].clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                serde_json::from_value::<DraftRecord>(bad)
                    .unwrap()
                    .validate()
                    .is_err(),
                "accepted {pointer}"
            );
        }
        let mut legacy = after.clone();
        legacy["draft"]["answers"][0]
            .as_object_mut()
            .unwrap()
            .remove("source");
        assert!(
            serde_json::from_value::<DraftRecord>(legacy["draft"].clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        assert!(!retained_answer(&legacy, &command));
        assert!(!answer_transition(&before, &legacy, &command));
        let wrapped = envelope(legacy);
        assert!(draft_report(&wrapped, "node", &command.id).is_some());
    }
    #[tokio::test]
    async fn disabled_workflow_records_exact_answer_without_model_or_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let command = command();
        let mut before = source(&command);
        let after = answered(&before, &command);
        let mut receiver = service.submissions.connect();
        let local = service.clone();
        let c = command.clone();
        let pending = tokio::spawn(async move { local.respond_request(request(c)).await });
        let get = next(&mut receiver).await;
        assert_eq!(get.kind, "requests.draft_get");
        before["operation_id"] = json!(get.operation_id);
        get.reply.send(Ok(before.clone())).unwrap();
        let save = next(&mut receiver).await;
        assert_eq!(save.kind, "requests.draft_answer");
        assert_eq!(
            save.operation_id.as_deref(),
            Some(command.operation_id.as_str())
        );
        save.reply.send(Ok(after.clone())).unwrap();
        let report = pending.await.unwrap();
        assert!(confirmed(&report, "node", &command), "{report}");
        assert_eq!(report["success"], true);
        assert_eq!(report["status"], "answer_recorded");
        assert_eq!(report["result"], after);
        assert_eq!(report["resolution"], Value::Null);
        assert!(receiver.try_recv().is_err());
        for (pointer, value) in [
            ("/command/expected_revision", json!(2)),
            (
                "/answer/result/draft/answers/0/source/sha256",
                json!("b".repeat(64)),
            ),
            (
                "/source/result/draft/proposals/0/proposal/goal",
                json!("Different valid goal"),
            ),
            (
                "/answer/result/request_context/origin_area",
                json!(["Other"]),
            ),
            ("/result/revision", json!(3)),
            ("/execution_authorized", json!(true)),
            ("/policy/resolve_on_answer", json!(true)),
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(!confirmed(&bad, "node", &command), "accepted {pointer}");
        }
    }
    #[tokio::test]
    async fn response_reuse_inspects_with_no_interpreter_even_if_policy_now_enabled() {
        for legacy in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let service = NodeService::new(dir.path().join("config.json"), "node".into());
            let engine =
                Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
            let _guard = service.attach_preview_runtime_with_policy(
                &engine,
                uuid::Uuid::new_v4().to_string(),
                InputPolicy {
                    prepare_on_input: true,
                    resolve_on_answer: true,
                },
            );
            let command = command();
            let mut retained = answered(&source(&command), &command);
            if legacy {
                retained["draft"]["answers"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("source");
            }
            let mut receiver = service.submissions.connect();
            let local = service.clone();
            let c = command.clone();
            let pending = tokio::spawn(async move { local.respond_request(request(c)).await });
            let get = next(&mut receiver).await;
            retained["operation_id"] = json!(get.operation_id);
            get.reply.send(Ok(retained)).unwrap();
            let report = pending.await.unwrap();
            assert!(confirmed(&report, "node", &command), "{report}");
            assert_eq!(report["success"], !legacy);
            assert_eq!(report["answer_attempted"], false);
            if legacy {
                assert_eq!(report["error_code"], "answer_receipt_conflict");
            } else {
                assert_eq!(report["status"], "historical_snapshot");
            }
            assert!(receiver.try_recv().is_err());
        }
    }
    #[tokio::test]
    async fn invalid_source_and_stale_revision_fail_before_answer_or_inference() {
        for malformed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let service = NodeService::new(dir.path().join("config.json"), "node".into());
            let engine =
                Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
            let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
            let mut command = command();
            command.expected_revision = 2;
            let mut data = source(&command);
            if malformed {
                data["request_context"]["origin_node"] = json!("other");
            }
            let mut receiver = service.submissions.connect();
            let local = service.clone();
            let c = command.clone();
            let pending = tokio::spawn(async move { local.respond_request(request(c)).await });
            let get = next(&mut receiver).await;
            data["operation_id"] = json!(get.operation_id);
            get.reply.send(Ok(data)).unwrap();
            let report = pending.await.unwrap();
            assert_eq!(report["success"], false);
            assert_eq!(report["answer_attempted"], false);
            assert_eq!(report["outcome_unknown"], false);
            assert_eq!(
                report["error_code"],
                if malformed {
                    "answer_source_unconfirmed"
                } else {
                    "answer_source_revision_changed"
                }
            );
            assert!(receiver.try_recv().is_err());
        }
    }
    #[tokio::test]
    async fn runtime_change_after_answer_ack_preserves_partial_receipts_without_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let policy = InputPolicy {
            prepare_on_input: true,
            resolve_on_answer: true,
        };
        let guard = service.attach_preview_runtime_with_policy(
            &engine,
            uuid::Uuid::new_v4().to_string(),
            policy,
        );
        let command = command();
        let mut before = source(&command);
        let after = answered(&before, &command);
        let mut receiver = service.submissions.connect();
        let local = service.clone();
        let c = command.clone();
        let pending = tokio::spawn(async move { local.respond_request(request(c)).await });
        let get = next(&mut receiver).await;
        before["operation_id"] = json!(get.operation_id);
        get.reply.send(Ok(before)).unwrap();
        let save = next(&mut receiver).await;
        save.reply.send(Ok(after.clone())).unwrap();
        let inspect = next(&mut receiver).await;
        assert_eq!(inspect.kind, "requests.draft_get");
        let _replacement = service.attach_preview_runtime_with_policy(
            &engine,
            uuid::Uuid::new_v4().to_string(),
            policy,
        );
        drop(guard);
        let report = pending.await.unwrap();
        assert!(confirmed(&report, "node", &command), "{report}");
        assert_eq!(report["success"], false);
        assert_eq!(report["error_code"], "runtime_changed");
        assert_eq!(report["answer_confirmed"], true);
        assert_eq!(report["result"], after);
        assert_eq!(report["outcome_unknown"], true);
        assert_eq!(report["preparation_command"], Value::Null);
        assert_eq!(
            report["resolution_command"]["operation_id"],
            stage_receipt(false, "node", &command, 2).unwrap()
        );
        let mut corrupt = report.clone();
        corrupt["resolution_command"]["operation_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed(&corrupt, "node", &command));
        let mut corrupt = report.clone();
        corrupt["result"]["revision"] = json!(3);
        assert!(!confirmed(&corrupt, "node", &command));
        assert!(receiver.try_recv().is_err());
    }
}
