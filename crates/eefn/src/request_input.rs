//! One bounded, runtime-owned input workflow over the existing journal/services.
//! Interpretation/preparation only: never approval, execution or an implicit retry.
use crate::{NodeService, request_preview::PreviewRequest};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputPolicy {
    // Missing in older/custom configs stays review-only, never surprise inference.
    #[serde(default)]
    pub prepare_on_input: bool,
    /// Missing in existing configs never enables new answer-triggered inference.
    #[serde(default)]
    pub resolve_on_answer: bool,
}
#[derive(Debug, clap::Subcommand)]
pub enum PolicyAction {
    Show,
    Set {
        #[arg(long,action=clap::ArgAction::Set,required_unless_present="resolve_on_answer")]
        prepare_on_input: Option<bool>,
        #[arg(long,action=clap::ArgAction::Set)]
        resolve_on_answer: Option<bool>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyCommand {
    Show,
    Set {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prepare_on_input: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolve_on_answer: Option<bool>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: PolicyCommand,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    success: bool,
    saved_policy: InputPolicy,
    applied_policy: Option<InputPolicy>,
    changed: bool,
    restart_required: bool,
    restart_requested: bool,
    execution_authorized: bool,
    dispatched: bool,
    note: String,
}
pub fn confirmed_policy(value: &Value, node: &str) -> bool {
    let Ok(r) = serde_json::from_value::<PolicyReport>(value.clone()) else {
        return false;
    };
    let _ = r.changed;
    r.schema_version == 1
        && r.report_type == "node_input_policy"
        && r.node_id == node
        && r.success
        && r.restart_required == (r.applied_policy != Some(r.saved_policy))
        && !r.restart_requested
        && !r.execution_authorized
        && !r.dispatched
        && r.note.len() <= 2048
}

/// Compatibility text only. The actual receipt, source and errors stay present.
pub fn input_reply(report: &Value) -> String {
    let id = report["result"]["id"].as_str().unwrap_or("unknown");
    if report["success"] != true {
        return format!(
            "Input workflow not confirmed ({}). {} Inspect receipt {} and any exact preparation command before retrying. No approval, workload execution or automatic retry was requested. {}",
            report["error_code"].as_str().unwrap_or("input_unconfirmed"),
            if report["source_confirmed"] == true {
                "The source request was saved, but preparation is not confirmed."
            } else {
                "Source save is not confirmed."
            },
            report["request_id"].as_str().unwrap_or("unavailable"),
            report["setup_hint"].as_str().unwrap_or("")
        );
    }
    match report["status"].as_str() {
        Some("historical_snapshot") => format!(
            "Retained request {id} ({}) inspected. No new interpretation, preparation, approval or execution was requested.",
            report["result"]["status"].as_str().unwrap_or("unknown")
        ),
        Some("awaiting_approval") => format!(
            "Prepared request {id}. Owner approval and execution are separate steps. No workload was executed."
        ),
        Some("clarification_required") => {
            let question = report["result"]["draft"]["proposals"]
                .as_array()
                .and_then(|p| p.last())
                .and_then(|p| p["proposal"]["clarification"].as_str())
                .unwrap_or("Review the saved request.");
            format!(
                "Saved request {id}. Clarification required: {question} No workload was executed."
            )
        }
        _ => format!(
            "Saved request {id} for review. Preparation, owner approval and execution are separate steps. No workload was executed."
        ),
    }
}

/// Derived receipt, not a secret/permission. A removed/recreated job differs.
pub fn preparation_receipt(node: &str, input: &str, job: &str) -> Result<String> {
    crate::network::validate_remote_node_id(node)?;
    crate::job_commands::validate_operation_id(input)?;
    crate::request_drafts::validate_id(job)?;
    let digest = Sha256::digest(serde_json::to_vec(&json!([
        "eef.input.prepare.v1",
        node,
        input,
        job
    ]))?);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}

impl NodeService {
    pub fn input_policy_command(&self, request: PolicyRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Input policy schema/node changed")
        }
        let changed = match request.command {
            PolicyCommand::Show => false,
            PolicyCommand::Set {
                prepare_on_input,
                resolve_on_answer,
            } => self.save_input_policy(prepare_on_input, resolve_on_answer)?,
        };
        let saved: InputPolicy = serde_json::from_value(
            self.read_config()?
                .get("input_workflow")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        let applied = self.input_workflow_runtime().map(|(_, p, _)| p);
        Ok(
            json!({"schema_version":1,"report_type":"node_input_policy","node_id":self.node_id,
            "success":true,"saved_policy":saved,"applied_policy":applied,"changed":changed,
            "restart_required":applied!=Some(saved),"restart_requested":false,
            "execution_authorized":false,"dispatched":false,
            "note":"Input/answer interpretation and preparation policy only. Saving does not apply it, infer, approve, execute, retry, resume or change existing requests. Restart the node to apply changes."}),
        )
    }

    pub async fn input_request(&self, request: PreviewRequest) -> Value {
        let mut report = json!({"schema_version":1,"report_type":"node_input_workflow","node_id":self.node_id,
            "request_id":request.request_id,"runtime_id":null,"success":false,"status":"input_not_confirmed",
            "coordinator_acknowledged":false,
            "source_confirmed":false,"creation_attempted":false,"opening_reused":false,"policy":null,
            "opening":null,"inspection":null,"preparation_command":null,"preparation":null,"result":null,
            "error_code":null,"outcome_unknown":false,"execution_authorized":false,"dispatched":false,
            "automatically_retried":false,
            "note":"Input/review/preparation only. Separate owner approval/execution are required. After uncertainty inspect this input receipt and any exact preparation receipt; no automatic retry or continuation."});
        if request.schema_version != 1
            || request.expected_node_id != self.node_id
            || crate::job_commands::validate_operation_id(&request.request_id).is_err()
            || crate::interpretation::messages(&request.text, &[]).is_err()
        {
            report["error_code"] = json!("invalid_input_request");
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
        let result = tokio::select! {
            biased;
            _=generation.changed()=>Err("runtime_changed"),
            result=tokio::time::timeout(Duration::from_secs(135),self.input_request_inner(&request,policy,&mut report))=>result.unwrap_or(Err("input_workflow_timeout")),
        };
        if let Err(code) = result {
            if report["error_code"].is_null() {
                report["error_code"] = json!(code)
            }
            report["success"] = json!(false);
            if matches!(code, "runtime_changed" | "input_workflow_timeout") {
                report["outcome_unknown"] = json!(
                    report["creation_attempted"] == true
                        || !report["preparation"].is_null()
                        || !report["preparation_command"].is_null()
                );
            }
        }
        report["coordinator_acknowledged"] =
            json!(report["success"] == true && report["source_confirmed"] == true);
        if !confirmed(&report, &self.node_id, &request.request_id, &request.text) {
            report["success"] = json!(false);
            report["coordinator_acknowledged"] = json!(false);
            report["error_code"] = json!("input_report_unconfirmed");
            report["outcome_unknown"] = json!(
                report["creation_attempted"] == true || !report["preparation_command"].is_null()
            );
        }
        report
    }

    async fn input_request_inner(
        &self,
        request: &PreviewRequest,
        policy: InputPolicy,
        report: &mut Value,
    ) -> std::result::Result<(), &'static str> {
        // An unconfirmed lookup never licenses another opening. Repeated input
        // receipts inspect current retained history, not reinfer/continue it.
        let found = tokio::time::timeout(
            Duration::from_secs(10),
            self.job_command(crate::job_commands::JobRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                operation_id: None,
                command: crate::job_commands::JobCommand::Find {
                    operation_id: request.request_id.clone(),
                },
            }),
        )
        .await
        .map_err(|_| "input_lookup_unconfirmed")?
        .map_err(|_| "input_lookup_unconfirmed")?;
        if found["success"] != true
            || found["acknowledged"] != true
            || found["outcome_unknown"] != false
            || found["data"]["operation_id"] != request.request_id
            || found["data"]["truncated"] != false
        {
            return Err("input_lookup_unconfirmed");
        }
        let jobs = found["data"]["jobs"]
            .as_array()
            .ok_or("input_lookup_unconfirmed")?;
        if found["data"]["total"].as_u64() != Some(jobs.len() as u64) || jobs.len() > 1 {
            return Err("input_receipt_ambiguous");
        }
        if let Some(job) = jobs.first() {
            let id = job["id"].as_str().ok_or("input_lookup_unconfirmed")?;
            let inspected = self
                .draft_command(crate::request_drafts::DraftRequest {
                    schema_version: 1,
                    expected_node_id: self.node_id.clone(),
                    request_id: request.request_id.clone(),
                    command: crate::request_drafts::DraftCommand::Get { id: id.into() },
                })
                .await
                .map_err(|_| "input_inspection_unconfirmed")?;
            if inspected["success"] != true
                || !crate::request_drafts::confirmed_report(
                    &inspected["result"],
                    &self.node_id,
                    &request.request_id,
                    Some(id),
                )
            {
                return Err("input_inspection_unconfirmed");
            }
            validate_source(&inspected["result"], request)?;
            report["result"] = inspected["result"].clone();
            report["inspection"] = inspected;
            report["source_confirmed"] = json!(true);
            report["opening_reused"] = json!(true);
            report["status"] = json!("historical_snapshot");
            report["success"] = json!(true);
            return Ok(());
        }
        report["creation_attempted"] = json!(true);
        let opened = self
            .open_request(PreviewRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                request_id: request.request_id.clone(),
                text: request.text.clone(),
            })
            .await;
        report["opening"] = opened.clone();
        report["outcome_unknown"] = opened["outcome_unknown"].clone();
        if let Some(hint) = opened.get("setup_hint") {
            report["setup_hint"] = hint.clone();
        }
        if opened["success"] != true {
            report["error_code"] = opened["error_code"].clone();
            return Err("input_open_not_confirmed");
        }
        if opened["coordinator_acknowledged"] != true
            || !crate::request_drafts::confirmed_report(
                &opened["result"],
                &self.node_id,
                &request.request_id,
                None,
            )
        {
            report["outcome_unknown"] = json!(true);
            return Err("input_open_not_confirmed");
        }
        validate_source(&opened["result"], request)?;
        report["source_confirmed"] = json!(true);
        report["result"] = opened["result"].clone();
        let draft: crate::request_drafts::DraftRecord =
            serde_json::from_value(opened["result"]["draft"].clone())
                .map_err(|_| "input_source_unconfirmed")?;
        if opened["result"]["reused"] == true {
            report["opening_reused"] = json!(true);
            report["status"] = json!("historical_snapshot");
        } else if draft.current().needs_clarification {
            report["status"] = json!("clarification_required");
        } else if !policy.prepare_on_input || crate::request_preparation::eligible(&draft).is_err()
        {
            report["status"] = json!("review_only");
        } else {
            let id = opened["result"]["id"]
                .as_str()
                .ok_or("input_source_unconfirmed")?;
            let revision = opened["result"]["revision"]
                .as_u64()
                .ok_or("input_source_unconfirmed")?;
            let command = crate::request_preparation::PrepareCommand {
                id: id.into(),
                expected_revision: revision,
                operation_id: preparation_receipt(&self.node_id, &request.request_id, id)
                    .map_err(|_| "input_source_unconfirmed")?,
            };
            report["preparation_command"] = json!(command);
            report["status"] = json!("preparation_not_confirmed");
            let prepared = self
                .prepare_request(crate::request_preparation::PrepareRequest {
                    schema_version: 1,
                    expected_node_id: self.node_id.clone(),
                    command: command.clone(),
                })
                .await;
            report["preparation"] = prepared.clone();
            report["outcome_unknown"] = prepared["outcome_unknown"].clone();
            if !crate::request_preparation::confirmed(&prepared, &self.node_id, &command) {
                return Err("input_preparation_not_confirmed");
            }
            if !confirms_preparation_source(&prepared, &opened["result"], &command) {
                report["outcome_unknown"] = json!(true);
                return Err("input_preparation_not_confirmed");
            }
            report["result"] = prepared["result"].clone();
            report["status"] = json!(if prepared["result"]["status"] == "awaiting_approval" {
                "awaiting_approval"
            } else {
                "clarification_required"
            });
        }
        report["success"] = json!(true);
        report["outcome_unknown"] = json!(false);
        Ok(())
    }
}

fn validate_source(
    value: &Value,
    request: &PreviewRequest,
) -> std::result::Result<(), &'static str> {
    if value["draft"]["original_text"] != request.text
        || value["request_context"]["request_id"] != request.request_id
        || value["draft"]
            .get("concrete_input")
            .is_some_and(|v| !v.is_null())
    {
        return Err("input_receipt_conflict");
    }
    Ok(())
}

pub(crate) fn confirms_preparation_source(
    prepared: &Value,
    source: &Value,
    command: &crate::request_preparation::PrepareCommand,
) -> bool {
    let Ok(view) =
        serde_json::from_value::<crate::request_planning::PlanningView>(prepared["result"].clone())
    else {
        return false;
    };
    let Ok(draft) =
        serde_json::from_value::<crate::request_drafts::DraftRecord>(source["draft"].clone())
    else {
        return false;
    };
    let Ok(context) =
        serde_json::from_value::<crate::context::RequestContext>(source["request_context"].clone())
    else {
        return false;
    };
    let Ok(Some(checkpoint)) = crate::request_preparation::saved_checkpoint(&view, command) else {
        return false;
    };
    checkpoint
        .source
        .confirms(&command.id, command.expected_revision, &draft, &context)
        && view.draft.original_text == draft.original_text
        && view.request_context == context
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    request_id: String,
    runtime_id: Option<String>,
    success: bool,
    status: String,
    coordinator_acknowledged: bool,
    source_confirmed: bool,
    creation_attempted: bool,
    opening_reused: bool,
    policy: Option<InputPolicy>,
    opening: Option<Value>,
    inspection: Option<Value>,
    preparation_command: Option<crate::request_preparation::PrepareCommand>,
    preparation: Option<Value>,
    result: Option<Value>,
    error_code: Option<String>,
    outcome_unknown: bool,
    execution_authorized: bool,
    dispatched: bool,
    automatically_retried: bool,
    note: String,
    #[serde(default)]
    setup_hint: Option<String>,
}
/// Both local CLI and core confirm the same complete workflow contract.
/// A source-save ACK is not promoted to confirmed preparation or authority.
pub fn confirmed(value: &Value, node: &str, input: &str, text: &str) -> bool {
    let Ok(r) = serde_json::from_value::<InputReport>(value.clone()) else {
        return false;
    };
    if r.schema_version != 1
        || r.report_type != "node_input_workflow"
        || r.node_id != node
        || r.request_id != input
        || crate::job_commands::validate_operation_id(input).is_err()
        || r.execution_authorized
        || r.dispatched
        || r.automatically_retried
        || r.note.len() > 2048
        || r.setup_hint.as_ref().is_some_and(|h| h.len() > 2048)
        || !matches!(
            r.status.as_str(),
            "input_not_confirmed"
                | "review_only"
                | "clarification_required"
                | "historical_snapshot"
                | "preparation_not_confirmed"
                | "awaiting_approval"
        )
    {
        return false;
    }
    if !r.success {
        let source_valid = !r.source_confirmed
            || [&r.opening, &r.inspection]
                .iter()
                .filter_map(|p| p.as_ref())
                .any(|p| {
                    p["success"] == true
                        && p["coordinator_acknowledged"] == true
                        && p["node_id"] == node
                        && p["request_id"] == input
                        && crate::request_drafts::confirmed_report(&p["result"], node, input, None)
                        && validate_source(
                            &p["result"],
                            &PreviewRequest {
                                schema_version: 1,
                                expected_node_id: node.into(),
                                request_id: input.into(),
                                text: text.into(),
                            },
                        )
                        .is_ok()
                });
        return source_valid
            && r.error_code
                .as_ref()
                .is_some_and(|c| !c.is_empty() && c.len() <= 128)
            && !r.coordinator_acknowledged;
    }
    if !r.coordinator_acknowledged
        || !r.source_confirmed
        || r.outcome_unknown
        || r.error_code.is_some()
        || r.runtime_id
            .as_ref()
            .is_none_or(|id| crate::job_commands::validate_operation_id(id).is_err())
        || r.policy.is_none()
    {
        return false;
    }
    let (source, inspection) = match (&r.opening, &r.inspection) {
        (Some(opened), None) => {
            if !r.creation_attempted
                || opened["report_type"] != "request_draft"
                || opened["runtime_id"].as_str() != r.runtime_id.as_deref()
                || opened["interpretation_context"] != "coordinator_disclosed_network"
                || opened["result"]["reused"] != r.opening_reused
            {
                return false;
            }
            (opened, false)
        }
        (None, Some(inspected)) => {
            if r.creation_attempted
                || !r.opening_reused
                || inspected["report_type"] != "request_draft"
            {
                return false;
            }
            (inspected, true)
        }
        _ => return false,
    };
    if source["schema_version"] != 1
        || source["node_id"] != node
        || source["request_id"] != input
        || source["success"] != true
        || source["coordinator_acknowledged"] != true
        || source["outcome_unknown"] != false
        || source["execution_authorized"] != false
        || source["dispatched"] != false
        || source["automatically_retried"] != false
        || !crate::request_drafts::confirmed_report(&source["result"], node, input, None)
        || source["result"]["draft"]["original_text"] != text
        || source["result"]["request_context"]["request_id"] != input
        || source["result"]["draft"]
            .get("concrete_input")
            .is_some_and(|v| !v.is_null())
    {
        return false;
    }
    if r.opening_reused {
        return r.status == "historical_snapshot"
            && r.preparation.is_none()
            && r.preparation_command.is_none()
            && r.result.as_ref() == Some(&source["result"]);
    }
    if inspection || source["result"]["status"] != "review" {
        return false;
    }
    let Ok(draft) = serde_json::from_value::<crate::request_drafts::DraftRecord>(
        source["result"]["draft"].clone(),
    ) else {
        return false;
    };
    let Some(data) = &r.result else { return false };
    if let (Some(command), Some(prepared)) = (&r.preparation_command, &r.preparation) {
        r.policy.unwrap().prepare_on_input
            && crate::request_preparation::eligible(&draft).is_ok()
            && Some(command.id.as_str()) == source["result"]["id"].as_str()
            && Some(command.expected_revision) == source["result"]["revision"].as_u64()
            && preparation_receipt(node, input, &command.id)
                .is_ok_and(|id| id == command.operation_id)
            && crate::request_preparation::confirmed(prepared, node, command)
            && confirms_preparation_source(prepared, &source["result"], command)
            && prepared["runtime_id"].as_str() == r.runtime_id.as_deref()
            && data == &prepared["result"]
            && data["request_context"] == source["result"]["request_context"]
            && data["draft"]["original_text"] == text
            && r.status
                == if data["status"] == "awaiting_approval" {
                    "awaiting_approval"
                } else {
                    "clarification_required"
                }
    } else {
        r.preparation_command.is_none()
            && r.preparation.is_none()
            && data == &source["result"]
            && if draft.current().needs_clarification {
                r.status == "clarification_required"
            } else {
                r.status == "review_only"
                    && (!r.policy.unwrap().prepare_on_input
                        || crate::request_preparation::eligible(&draft).is_err())
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn request() -> PreviewRequest {
        PreviewRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "  Inspect status\n".into(),
        }
    }
    fn draft(req: &PreviewRequest) -> Value {
        json!({"schema_version":1,"report_type":"coordinator_request_draft","id":"00112233445566778899aabbccddeeff",
            "revision":1,"status":"review","operation_id":req.request_id,
            "request_context":{"request_id":req.request_id,"origin_node":"node","origin_area":["Home","Office"]},
            "draft":{"schema_version":1,"original_text":req.text,"proposals":[{"operation_id":req.request_id,"at_ms":1,
                "allowed_capabilities":["system.info"],"proposal":{"schema_version":1,"intent":"information","goal":"Inspect status",
                "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot",
                "needs_clarification":false,"clarification":null}}],"answers":[]},"review_state":"review_required",
            "reused":false,"execution_authorized":false,"dispatched":false,"persisted":true,"note":"Retained review only."})
    }
    #[test]
    fn receipt_is_canonical_domain_separated_and_bound_to_source_job() {
        let input = uuid::Uuid::new_v4().to_string();
        let job = "00112233445566778899aabbccddeeff";
        let receipt = preparation_receipt("node", &input, job).unwrap();
        assert_eq!(receipt, preparation_receipt("node", &input, job).unwrap());
        assert_ne!(receipt, input);
        assert!(crate::job_commands::validate_operation_id(&receipt).is_ok());
        assert_ne!(receipt, preparation_receipt("other", &input, job).unwrap());
        assert_ne!(
            receipt,
            preparation_receipt("node", &uuid::Uuid::new_v4().to_string(), job).unwrap()
        );
        assert_ne!(
            receipt,
            preparation_receipt("node", &input, "10112233445566778899aabbccddeeff").unwrap()
        );
        assert!(preparation_receipt("node", "invalid", job).is_err());
        assert!(preparation_receipt("node", &input, "invalid").is_err());
    }
    #[tokio::test]
    async fn saved_policy_is_not_applied_and_older_configs_are_review_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        crate::setup::write_json(&path, &json!({"node_id":"node","name":"Fixture"})).unwrap();
        let service = NodeService::new(path.clone(), "node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let policy = |command| {
            service
                .input_policy_command(PolicyRequest {
                    schema_version: 1,
                    expected_node_id: "node".into(),
                    command,
                })
                .unwrap()
        };
        let shown = policy(PolicyCommand::Show);
        assert!(confirmed_policy(&shown, "node"));
        assert_eq!(shown["saved_policy"]["prepare_on_input"], false);
        assert_eq!(shown["saved_policy"]["resolve_on_answer"], false);
        assert_eq!(shown["restart_required"], false);
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            policy(PolicyCommand::Set {
                prepare_on_input: Some(false),
                resolve_on_answer: None,
            })["changed"],
            false
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let saved = policy(PolicyCommand::Set {
            prepare_on_input: Some(true),
            resolve_on_answer: None,
        });
        assert!(confirmed_policy(&saved, "node"));
        assert_eq!(saved["applied_policy"]["prepare_on_input"], false);
        assert_eq!(saved["restart_required"], true);
        assert_eq!(saved["restart_requested"], false);
        assert_eq!(
            policy(PolicyCommand::Set {
                prepare_on_input: Some(true),
                resolve_on_answer: None,
            })["changed"],
            false
        );
        assert_eq!(service.read_config().unwrap()["name"], "Fixture");
        let answer_policy = policy(PolicyCommand::Set {
            prepare_on_input: None,
            resolve_on_answer: Some(true),
        });
        assert_eq!(answer_policy["saved_policy"]["prepare_on_input"], true);
        assert_eq!(answer_policy["saved_policy"]["resolve_on_answer"], true);
        assert_eq!(answer_policy["applied_policy"]["resolve_on_answer"], false);
        policy(PolicyCommand::Set {
            prepare_on_input: None,
            resolve_on_answer: Some(false),
        });
        assert!(
            service
                .input_policy_command(PolicyRequest {
                    schema_version: 1,
                    expected_node_id: "node".into(),
                    command: PolicyCommand::Set {
                        prepare_on_input: None,
                        resolve_on_answer: None
                    }
                })
                .is_err()
        );
        drop(guard);
        let _next = service.attach_preview_runtime_with_policy(
            &engine,
            uuid::Uuid::new_v4().to_string(),
            InputPolicy {
                prepare_on_input: true,
                ..Default::default()
            },
        );
        assert_eq!(policy(PolicyCommand::Show)["restart_required"], false);
        assert!(serde_json::from_value::<InputPolicy>(json!({"prepare_on_input":"true"})).is_err());
        assert!(
            serde_json::from_value::<InputPolicy>(json!({"prepare_on_input":true,"approve":true}))
                .is_err()
        );
        assert!(
            service
                .input_policy_command(PolicyRequest {
                    schema_version: 1,
                    expected_node_id: "other".into(),
                    command: PolicyCommand::Set {
                        prepare_on_input: Some(false),
                        resolve_on_answer: None,
                    }
                })
                .is_err()
        );
        assert_eq!(
            crate::setup::defaults()["input_workflow"]["prepare_on_input"],
            true
        );
    }
    #[tokio::test]
    async fn input_validates_before_lookup_and_busy_requests_are_not_queued() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let mut receiver = service.submissions.connect();
        let mut req = request();
        req.expected_node_id = "other".into();
        assert_eq!(
            service.input_request(req).await["error_code"],
            "invalid_input_request"
        );
        assert_eq!(
            service.input_request(request()).await["error_code"],
            "runtime_unavailable"
        );
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let _permit = service.input_workflow_permit().unwrap();
        let report = service.input_request(request()).await;
        assert_eq!(report["error_code"], "input_workflow_busy");
        assert_eq!(report["creation_attempted"], false);
        assert!(receiver.try_recv().is_err());
        assert!(!dir.path().join("config.json").exists());
    }
    #[tokio::test]
    async fn runtime_replacement_cancels_lookup_without_creating_or_retrying() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let mut receiver = service.submissions.connect();
        let local = service.clone();
        let req = request();
        let pending = tokio::spawn(async move { local.input_request(req).await });
        let lookup = receiver.recv().await.unwrap();
        assert_eq!(lookup.kind, "jobs.find");
        let _replacement =
            service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        drop(guard);
        let report = pending.await.unwrap();
        assert_eq!(report["error_code"], "runtime_changed");
        assert_eq!(report["outcome_unknown"], false);
        assert_eq!(report["creation_attempted"], false);
        assert!(receiver.try_recv().is_err());
    }
    #[tokio::test]
    async fn malformed_or_ambiguous_lookup_cannot_license_opening() {
        for data in [
            json!({}),
            json!({"total":0,"jobs":[],"truncated":true}),
            json!({"total":1,"jobs":[],"truncated":false}),
            json!({"total":2,"jobs":[{},{}],"truncated":false}),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let service = NodeService::new(dir.path().join("config.json"), "node".into());
            let engine =
                Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
            let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
            let mut receiver = service.submissions.connect();
            let local = service.clone();
            let req = request();
            let id = req.request_id.clone();
            let pending = tokio::spawn(async move { local.input_request(req).await });
            let lookup = receiver.recv().await.unwrap();
            let mut data = data;
            data["operation_id"] = json!(id);
            lookup.reply.send(Ok(data)).unwrap();
            let report = pending.await.unwrap();
            assert_eq!(report["success"], false);
            assert_eq!(report["creation_attempted"], false);
            assert!(receiver.try_recv().is_err());
        }
    }
    #[tokio::test]
    async fn retained_history_is_read_only_without_any_interpreter_even_with_policy_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime_with_policy(
            &engine,
            uuid::Uuid::new_v4().to_string(),
            InputPolicy {
                prepare_on_input: true,
                ..Default::default()
            },
        );
        let mut receiver = service.submissions.connect();
        let req = request();
        let expected = draft(&req);
        let expected_id = expected["id"].clone();
        let local = service.clone();
        let cloned = PreviewRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            request_id: req.request_id.clone(),
            text: req.text.clone(),
        };
        let pending = tokio::spawn(async move { local.input_request(cloned).await });
        let lookup = receiver.recv().await.unwrap();
        assert_eq!(lookup.kind, "jobs.find");
        lookup.reply.send(Ok(json!({"operation_id":req.request_id,"total":1,"truncated":false,"jobs":[{"id":expected_id}]}))).unwrap();
        let get = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(get.kind, "requests.draft_get");
        get.reply.send(Ok(expected.clone())).unwrap();
        let report = pending.await.unwrap();
        assert!(
            confirmed(&report, "node", &req.request_id, &req.text),
            "{report}"
        );
        assert_eq!(report["success"], true);
        assert_eq!(report["status"], "historical_snapshot");
        assert_eq!(report["result"], expected);
        assert_eq!(report["preparation"], Value::Null);
        assert_eq!(report["creation_attempted"], false);
        assert!(receiver.try_recv().is_err());
        for (pointer, value) in [
            ("/dispatched", json!(true)),
            ("/policy/approve", json!(true)),
            ("/result/draft/original_text", json!("changed")),
            (
                "/inspection/result/request_context/origin_node",
                json!("other"),
            ),
            ("/status", json!("awaiting_approval")),
            ("/runtime_id", json!("invalid")),
        ] {
            let mut bad = report.clone();
            if pointer == "/policy/approve" {
                bad["policy"]["approve"] = value;
            } else {
                *bad.pointer_mut(pointer).unwrap() = value;
            }
            assert!(
                !confirmed(&bad, "node", &req.request_id, &req.text),
                "accepted {pointer}"
            );
        }
    }
    #[test]
    fn compatibility_text_uses_current_question_and_preserves_partial_failure() {
        let report = json!({"success":true,"status":"clarification_required","result":{"id":"job","draft":{"proposals":[{"proposal":{"clarification":"Old question"}},{"proposal":{"clarification":"Current question"}}]}}});
        let reply = input_reply(&report);
        assert!(reply.contains("Current question"));
        assert!(!reply.contains("Old question"));
        let partial = json!({"success":false,"source_confirmed":true,"error_code":"input_preparation_not_confirmed","request_id":"receipt"});
        assert!(input_reply(&partial).contains("source request was saved"));
    }
}
