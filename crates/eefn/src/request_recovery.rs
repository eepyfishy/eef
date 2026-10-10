//! Explicit evidence reconciliation, never execution, renewal or remote-stop authority.
use crate::{request_abandon::AbandonActor, request_result::RetainedResult};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use crate::request_abandon::AbandonActor as RecoveryActor;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverCommand {
    pub id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub grant_id: String,
}
impl RecoverCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if self.expected_revision == 0
            || self.expected_revision == u64::MAX
            || uuid::Uuid::parse_str(&self.operation_id)?.is_nil()
            || uuid::Uuid::parse_str(&self.grant_id)?.is_nil()
            || self.operation_id == self.grant_id
        {
            bail!("Exact source revision and distinct recovery receipt required")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRecord {
    pub schema_version: u32,
    pub command: RecoverCommand,
    pub actor: RecoveryActor,
    pub decided_at_ms: u64,
    pub result: RetainedResult,
}
impl RecoveryRecord {
    pub fn validate(&self) -> Result<()> {
        self.command.validate()?;
        self.result.validate()?;
        if self.schema_version != 1
            || self.decided_at_ms == 0
            || self.result.receipt.job_id != self.command.id
            || self.result.receipt.grant_id != self.command.grant_id
            || self
                .result
                .report
                .as_ref()
                .is_none_or(|r| r.state != crate::grants::ReceiptState::Completed)
        {
            bail!("Only retained completed evidence can reconcile tracking")
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: RecoverCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub status: String,
    pub request_context: crate::context::RequestContext,
    pub command: RecoverCommand,
    pub actor: RecoveryActor,
    pub applied_revision: u64,
    pub result_sha256: String,
    pub reused: bool,
    pub persisted: bool,
    pub reconciled: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub remote_stop_confirmed: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    command: &RecoverCommand,
    actor: RecoveryActor,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<RecoverReport>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_recovery"
        && r.success
        && r.id == command.id
        && r.command == *command
        && r.actor == actor
        && r.applied_revision == command.expected_revision + 1
        && r.revision >= r.applied_revision
        && matches!(
            r.status.as_str(),
            "completed" | "failed" | "interrupted" | "cancelled"
        )
        && crate::request_result::valid_digest(&r.result_sha256)
        && crate::job_commands::validate_operation_id(&r.runtime_id).is_ok()
        && crate::job_commands::validate_operation_id(r.request_context.request_id()).is_ok()
        && crate::network::validate_remote_node_id(r.request_context.origin_node()).is_ok()
        && origin.is_none_or(|s| s == r.request_context.origin_node())
        && runtime.is_none_or(|s| s == r.runtime_id)
        && r.note.len() <= 2048
        && r.persisted
        && r.reconciled
        && !r.execution_requested
        && !r.automatically_retried
        && !r.remote_stop_confirmed
}
pub fn confirmed_node(value: &Value, command: &RecoverCommand, node: &str) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NodeReport {
        schema_version: u32,
        report_type: String,
        node_id: String,
        command: RecoverCommand,
        success: bool,
        coordinator_acknowledged: bool,
        outcome_unknown: bool,
        execution_requested: bool,
        automatically_retried: bool,
        remote_stop_confirmed: bool,
        note: String,
        result: Option<Value>,
        error_code: Option<String>,
    }
    let Ok(r) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    r.schema_version == 1
        && r.report_type == "request_recovery"
        && r.node_id == node
        && r.command == *command
        && command.validate().is_ok()
        && !r.execution_requested
        && !r.automatically_retried
        && !r.remote_stop_confirmed
        && r.note.len() <= 2048
        && if r.success {
            r.coordinator_acknowledged
                && !r.outcome_unknown
                && r.error_code.is_none()
                && r.result.as_ref().is_some_and(|v| {
                    confirmed(v, command, AbandonActor::OriginNode, Some(node), None)
                })
        } else {
            !r.coordinator_acknowledged
                && r.result.is_none()
                && r.error_code.as_deref() == Some("recovery_unconfirmed")
        }
}
impl crate::NodeService {
    pub async fn recover_request_command(&self, req: RecoverRequest) -> Result<Value> {
        if req.schema_version != 1 || req.expected_node_id != self.node_id {
            bail!("Recovery schema/node changed")
        }
        req.command.validate()?;
        let mut report = json!({"schema_version":1,"report_type":"request_recovery","node_id":self.node_id,
            "command":req.command,"success":false,"coordinator_acknowledged":false,"outcome_unknown":false,
            "execution_requested":false,"automatically_retried":false,"remote_stop_confirmed":false,
            "note":"One evidence-reconciliation submission only. Inspect the request after uncertainty and reuse only this exact receipt. No replay, remaining-step dispatch, lease renewal or remote stop."});
        let result = self
            .submissions
            .request_correlated(
                "requests.recover_result",
                json!(req.command),
                Some(req.command.operation_id.clone()),
            )
            .await;
        match result {
            Ok(data)
                if confirmed(
                    &data,
                    &req.command,
                    RecoveryActor::OriginNode,
                    Some(&self.node_id),
                    None,
                ) =>
            {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = data;
            }
            Err(e) => {
                report["error_code"] = json!("recovery_unconfirmed");
                report["outcome_unknown"] = json!(!matches!(
                    e.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                ));
            }
            _ => {
                report["error_code"] = json!("recovery_unconfirmed");
                report["outcome_unknown"] = json!(true);
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_confirmation_pins_receipt_actor_revision_and_never_confirms_execution_or_stop() {
        let command = RecoverCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 8,
            operation_id: uuid::Uuid::new_v4().to_string(),
            grant_id: uuid::Uuid::new_v4().to_string(),
        };
        let runtime = uuid::Uuid::new_v4().to_string();
        let report = json!(RecoverReport {
            schema_version: 1,
            report_type: "coordinator_request_recovery".into(),
            success: true,
            runtime_id: runtime.clone(),
            id: command.id.clone(),
            revision: 9,
            status: "completed".into(),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                "owner".into(),
                vec![]
            )
            .unwrap(),
            command: command.clone(),
            actor: RecoveryActor::OriginNode,
            applied_revision: 9,
            result_sha256: "a".repeat(64),
            reused: false,
            persisted: true,
            reconciled: true,
            execution_requested: false,
            automatically_retried: false,
            remote_stop_confirmed: false,
            note: "Retained evidence, not replay.".into(),
        });
        assert!(confirmed(
            &report,
            &command,
            RecoveryActor::OriginNode,
            Some("owner"),
            Some(&runtime)
        ));
        assert!(!confirmed(
            &report,
            &command,
            RecoveryActor::CoordinatorOwner,
            None,
            None
        ));
        assert!(!confirmed(
            &report,
            &command,
            RecoveryActor::OriginNode,
            Some("other"),
            None
        ));
        for (field, value) in [
            ("execution_requested", json!(true)),
            ("automatically_retried", json!(true)),
            ("remote_stop_confirmed", json!(true)),
            ("persisted", json!(false)),
            ("reconciled", json!(false)),
            ("applied_revision", json!(10)),
            ("revision", json!(8)),
            ("result_sha256", json!("invalid")),
            ("status", json!("running")),
            ("runtime_id", json!("invalid")),
            ("retry_authorized", json!(true)),
        ] {
            let mut bad = report.clone();
            bad[field] = value;
            assert!(
                !confirmed(&bad, &command, RecoveryActor::OriginNode, None, None),
                "{field}"
            );
        }
        let mut wrapper = json!({"schema_version":1,"report_type":"request_recovery","node_id":"owner","command":command,
            "success":true,"coordinator_acknowledged":true,"outcome_unknown":false,"execution_requested":false,
            "automatically_retried":false,"remote_stop_confirmed":false,"result":report,"note":"Explicit evidence"});
        assert!(confirmed_node(&wrapper, &command, "owner"));
        wrapper["outcome_unknown"] = json!(true);
        assert!(!confirmed_node(&wrapper, &command, "owner"));
        wrapper["success"] = json!(false);
        wrapper["coordinator_acknowledged"] = json!(false);
        wrapper.as_object_mut().unwrap().remove("result");
        wrapper["error_code"] = json!("recovery_unconfirmed");
        assert!(confirmed_node(&wrapper, &command, "owner"));
        wrapper["replay_authorized"] = json!(true);
        assert!(!confirmed_node(&wrapper, &command, "owner"));
    }
    #[test]
    fn recovery_command_never_accepts_caller_fabricated_evidence_or_authority() {
        let command = RecoverCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 8,
            operation_id: uuid::Uuid::new_v4().to_string(),
            grant_id: uuid::Uuid::new_v4().to_string(),
        };
        for field in [
            "report",
            "result",
            "retry",
            "completed",
            "expected_issuer_id",
            "approval_sha256",
            "actor",
        ] {
            let mut value = json!(command);
            value[field] = json!(true);
            assert!(serde_json::from_value::<RecoverCommand>(value).is_err());
        }
        for revision in [0, u64::MAX] {
            let mut bad = command.clone();
            bad.expected_revision = revision;
            assert!(bad.validate().is_err());
        }
        let mut bad = command.clone();
        bad.operation_id = bad.grant_id.clone();
        assert!(bad.validate().is_err());
        let mut bad = command;
        bad.operation_id = uuid::Uuid::nil().to_string();
        assert!(bad.validate().is_err());
    }
}
