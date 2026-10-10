//! Origin input adapter for explicitly approved plans; no local grant fabrication.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteCommand {
    pub id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub approval_sha256: String,
}
impl ExecuteCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        if uuid::Uuid::parse_str(&self.operation_id)?.is_nil()
            || self.expected_revision == 0
            || self.expected_revision == u64::MAX
            || self.approval_sha256.len() != 64
            || !self
                .approval_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("Invalid exact approval execution command")
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ExecuteCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub operation_id: String,
    pub source_revision: u64,
    pub approval_sha256: String,
    pub revision: u64,
    pub status: String,
    pub request_context: crate::context::RequestContext,
    pub reused: bool,
    pub persisted: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    command: &ExecuteCommand,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<ExecuteReport>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_execution"
        && r.success
        && r.id == command.id
        && r.operation_id == command.operation_id
        && r.source_revision == command.expected_revision
        && r.approval_sha256 == command.approval_sha256
        && r.revision > r.source_revision
        && r.persisted
        && r.execution_requested
        && !r.automatically_retried
        && r.note.len() <= 2048
        && crate::job_commands::validate_operation_id(&r.runtime_id).is_ok()
        && runtime.is_none_or(|id| id == r.runtime_id)
        && origin.is_none_or(|id| id == r.request_context.origin_node())
        && matches!(
            r.status.as_str(),
            "running" | "stopping" | "completed" | "failed" | "cancelled" | "interrupted"
        )
}
impl crate::NodeService {
    pub async fn execute_command(&self, request: ExecuteRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Execution command schema/node changed")
        }
        request.command.validate()?;
        let command = request.command;
        let mut report = json!({"schema_version":1,"report_type":"request_execution","success":false,"node_id":self.node_id,
            "id":command.id,"operation_id":command.operation_id,"coordinator_acknowledged":false,"outcome_unknown":false,"automatically_retried":false});
        match self
            .submissions
            .request_correlated(
                "requests.execute",
                json!(command),
                Some(command.operation_id.clone()),
            )
            .await
        {
            Ok(data) if confirmed(&data, &command, Some(&self.node_id), None) => {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = data;
            }
            Err(e)
                if e.downcast_ref::<crate::submission::SubmissionFailure>()
                    .is_some() =>
            {
                report["error_code"] = json!("execution_not_confirmed");
                report["outcome_unknown"] = json!(!matches!(
                    e.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                ));
            }
            _ => {
                report["error_code"] = json!("execution_unconfirmed");
                report["outcome_unknown"] = json!(true);
            }
        }
        report["note"] = json!(
            "One submission only. Inspect jobs get/request draft before retrying; no fallback, replay or automatic renewal. Acceptance is not completion or cancellation."
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_and_receipt_contracts_reject_widening_unknown_fields_or_identity_changes() {
        let command = ExecuteCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 3,
            operation_id: uuid::Uuid::new_v4().to_string(),
            approval_sha256: "a".repeat(64),
        };
        command.validate().unwrap();
        let context = crate::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin".into(),
            vec![],
        )
        .unwrap();
        let report = json!(ExecuteReport {
            schema_version: 1,
            report_type: "coordinator_request_execution".into(),
            success: true,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            id: command.id.clone(),
            operation_id: command.operation_id.clone(),
            source_revision: 3,
            approval_sha256: command.approval_sha256.clone(),
            revision: 4,
            status: "running".into(),
            request_context: context,
            reused: false,
            persisted: true,
            execution_requested: true,
            automatically_retried: false,
            note: "No replay".into()
        });
        assert!(confirmed(
            &report,
            &command,
            Some("origin"),
            report["runtime_id"].as_str()
        ));
        for (field, bad) in [
            ("persisted", json!(false)),
            ("automatically_retried", json!(true)),
            ("source_revision", json!(4)),
            ("approval_sha256", json!("b".repeat(64))),
            ("operation_id", json!(uuid::Uuid::new_v4().to_string())),
            ("execution_requested", json!(false)),
            ("status", json!("approved")),
        ] {
            let mut bad_report = report.clone();
            bad_report[field] = bad;
            assert!(!confirmed(&bad_report, &command, Some("origin"), None));
        }
        assert!(!confirmed(&report, &command, Some("other"), None));
        assert!(!confirmed(&report, &command, None, Some("other-runtime")));
        let mut extra = report;
        extra["grant"] = json!({"permissions":"all"});
        assert!(!confirmed(&extra, &command, None, None));
        let mut bad = json!(command);
        bad["retry"] = json!(true);
        assert!(serde_json::from_value::<ExecuteCommand>(bad).is_err());
        let mut bad = command;
        bad.operation_id = uuid::Uuid::nil().to_string();
        assert!(bad.validate().is_err());
    }
}
