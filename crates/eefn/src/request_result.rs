//! Explicit read-only terminal evidence/output. Never renews or replays a grant.
use crate::{
    grants::{GrantReport, REPORT_HEADROOM, ReceiptState},
    request_receipt::ConsumedReceipt,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::request_receipt::{
    ReceiptCommand as ResultCommand, ReceiptRequest as ResultRequest,
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub request_context: crate::context::RequestContext,
    pub coordinator_attempt_state: String,
    pub result: RetainedResult,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    cmd: &ResultCommand,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<ResultReport>(value.clone()) else {
        return false;
    };
    cmd.validate().is_ok()
        && r.result.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_result"
        && r.success
        && r.id == cmd.id
        && r.result.receipt.job_id == cmd.id
        && r.result.receipt.grant_id == cmd.grant_id
        && r.revision > 0
        && r.note.len() <= 2048
        && crate::job_commands::validate_operation_id(&r.runtime_id).is_ok()
        && crate::job_commands::validate_operation_id(r.request_context.request_id()).is_ok()
        && crate::network::validate_remote_node_id(r.request_context.origin_node()).is_ok()
        && runtime.is_none_or(|s| s == r.runtime_id)
        && origin.is_none_or(|s| s == r.request_context.origin_node())
        && matches!(
            r.coordinator_attempt_state.as_str(),
            "issued" | "accepted" | "completed" | "rejected" | "unknown"
        )
        && !r.execution_requested
        && !r.automatically_retried
        && !r.reconciled
}
pub fn confirmed_node(value: &Value, cmd: &ResultCommand, node: &str, request_id: &str) -> bool {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NodeReport {
        schema_version: u32,
        report_type: String,
        node_id: String,
        request_id: String,
        id: String,
        grant_id: String,
        success: bool,
        execution_requested: bool,
        automatically_retried: bool,
        reconciled: bool,
        note: String,
        result: Option<Value>,
        error_code: Option<String>,
    }
    let Ok(r) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    cmd.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "request_result"
        && r.node_id == node
        && r.request_id == request_id
        && r.id == cmd.id
        && r.grant_id == cmd.grant_id
        && !r.execution_requested
        && !r.automatically_retried
        && !r.reconciled
        && r.note.len() <= 2048
        && if r.success {
            r.error_code.is_none()
                && r.result
                    .as_ref()
                    .is_some_and(|v| confirmed(v, cmd, Some(node), None))
        } else {
            r.result.is_none() && r.error_code.as_deref() == Some("result_unavailable")
        }
}
impl crate::NodeService {
    pub async fn request_result(&self, req: ResultRequest) -> Result<Value> {
        if req.schema_version != 1 || req.expected_node_id != self.node_id {
            bail!("Result query schema/node changed")
        }
        req.command.validate()?;
        crate::job_commands::validate_operation_id(&req.request_id)?;
        let result = self
            .submissions
            .request_correlated(
                "requests.result",
                serde_json::json!(req.command),
                Some(req.request_id.clone()),
            )
            .await;
        let mut report = serde_json::json!({"schema_version":1,"report_type":"request_result","node_id":self.node_id,
            "request_id":req.request_id,"id":req.command.id,"grant_id":req.command.grant_id,"success":false,
            "execution_requested":false,"automatically_retried":false,"reconciled":false,
            "note":"Explicit read-only final report/output, not proof external effects stopped. No job change, reconciliation, replay or fallback."});
        match result {
            Ok(value) if confirmed(&value, &req.command, Some(&self.node_id), None) => {
                report["success"] = serde_json::json!(true);
                report["result"] = value;
            }
            _ => report["error_code"] = serde_json::json!("result_unavailable"),
        }
        Ok(report)
    }
}

pub(crate) fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub expected_node_id: String,
    pub expected_issuer_id: String,
    pub expected_grant_sha256: String,
    pub grant_id: String,
}
impl WireRequest {
    pub fn receipt_query(&self) -> Result<crate::request_receipt::WireRequest> {
        if self.kind != "grant_result" || !valid_digest(&self.expected_grant_sha256) {
            bail!("Invalid result query type/scope")
        }
        Ok(crate::request_receipt::WireRequest {
            kind: "grant_receipt".into(),
            id: self.id.clone(),
            expected_node_id: self.expected_node_id.clone(),
            expected_issuer_id: self.expected_issuer_id.clone(),
            grant_id: self.grant_id.clone(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedResult {
    pub schema_version: u32,
    pub receipt: ConsumedReceipt,
    pub max_result_bytes: usize,
    pub availability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<GrantReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_sha256: Option<String>,
}
impl RetainedResult {
    pub fn confirms(&self, grant: &crate::grants::ExecutionGrant) -> bool {
        self.validate().is_ok()
            && self.receipt.confirms(grant)
            && (self.max_result_bytes == 0
                || self.max_result_bytes == grant.limits.max_result_bytes)
            && self.report.as_ref().is_none_or(|r| r.confirms(grant))
    }
    pub fn validate(&self) -> Result<()> {
        self.receipt.validate()?;
        if self.schema_version != 1 {
            bail!("Unsupported retained result schema")
        }
        if self.max_result_bytes == 0 {
            if self.availability != "legacy_metadata_only"
                || self.report.is_some()
                || self.report_sha256.is_some()
            {
                bail!("Legacy receipt cannot claim retained output")
            }
            return Ok(());
        }
        if !(1024..=1024 * 1024).contains(&self.max_result_bytes) {
            bail!("Invalid retained result bound")
        }
        if self.receipt.state == "accepted" {
            if self.availability != "no_final_checkpoint"
                || self.report.is_some()
                || self.report_sha256.is_some()
            {
                bail!("Pending/uncertain receipt cannot claim a final report")
            }
            return Ok(());
        }
        let r = self.report.as_ref().context("Final report unavailable")?;
        crate::job_commands::validate_operation_id(&r.session_id)?;
        let valid_identifier = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        };
        if self.availability != "retained_final_report"
            || !self.report_sha256.as_deref().is_some_and(|s| {
                valid_digest(s)
                    && crate::grants::definition_digest(&serde_json::json!(r)).is_ok_and(|d| d == s)
            })
            || r.schema_version != 1
            || r.grant_id != self.receipt.grant_id
            || r.grant_sha256 != self.receipt.grant_sha256
            || r.issuer_id != self.receipt.issuer_id
            || r.executor_node != self.receipt.node_id
            || r.job_id != self.receipt.job_id
            || r.task_id != self.receipt.task_id
            || r.attempt != self.receipt.attempt
            || uuid::Uuid::parse_str(&r.session_id)?.is_nil()
            || r.plan_sha256.len() != 64
            || !r
                .plan_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !r.accepted
            || !r.persisted
            || !r.execution_may_have_started
            || !matches!(r.state, ReceiptState::Completed | ReceiptState::Unknown)
            || self.receipt.state
                != if r.state == ReceiptState::Completed {
                    "completed"
                } else {
                    "unknown"
                }
            || r.success != (r.state == ReceiptState::Completed && r.error_code.is_none())
            || r.error_code
                .as_deref()
                .is_some_and(|s| !valid_identifier(s))
            || (r.state == ReceiptState::Unknown && (!r.data.is_null() || r.error_code.is_none()))
            || serde_json::to_vec(&r.data)?.len() > self.max_result_bytes
            || serde_json::to_vec(r)?.len() > self.max_result_bytes + REPORT_HEADROOM
        {
            bail!("Damaged retained final report; no recovery or replay")
        }
        Ok(())
    }
}

pub(crate) fn read_result(
    db: &rusqlite::Connection,
    node: &str,
    issuer: Option<&str>,
    id: &str,
) -> Result<RetainedResult> {
    // Consistent read snapshot: a concurrent final checkpoint must not mix an
    // accepted metadata row with the subsequently completed output.
    let tx = db.unchecked_transaction()?;
    let receipt = crate::request_receipt::read_consumed(&tx, node, issuer, id)?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let mut result = RetainedResult {
        schema_version: 1,
        receipt,
        max_result_bytes: 0,
        availability: "legacy_metadata_only".into(),
        report: None,
        report_sha256: None,
    };
    if version == 2 {
        // Check storage size before materializing/parsing untrusted report text.
        let (limit, size, raw, checksum): (i64, Option<i64>, Option<String>, Option<String>) = tx.query_row(
            "SELECT max_result_bytes,length(CAST(report AS BLOB)),CASE WHEN length(CAST(report AS BLOB))<=?2 THEN report ELSE NULL END,report_sha256 FROM execution_receipts WHERE grant_id=?1",
            rusqlite::params![id,i64::try_from(1024*1024+REPORT_HEADROOM)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        )?;
        result.max_result_bytes = usize::try_from(limit)?;
        if size.is_some_and(|s| s < 0 || s as u64 > limit.max(0) as u64 + REPORT_HEADROOM as u64) {
            bail!("Retained report exceeds its stored bound")
        }
        result.report = raw.map(|r| serde_json::from_str(&r)).transpose()?;
        result.report_sha256 = checksum;
        if result.max_result_bytes > 0 {
            result.availability = if result.receipt.state == "accepted" {
                "no_final_checkpoint"
            } else {
                "retained_final_report"
            }
            .into();
        }
    }
    result.validate()?;
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(state: ReceiptState) -> (crate::grants::ExecutionGrant, RetainedResult) {
        let g = crate::grants::tests::grant();
        let error = if state == ReceiptState::Unknown {
            Some("execution_failed_effects_unknown")
        } else {
            None
        };
        let report = GrantReport::new(
            &g,
            state,
            if error.is_some() {
                Value::Null
            } else {
                serde_json::json!({"content":"bounded text"})
            },
            error,
        );
        let receipt = ConsumedReceipt {
            schema_version: 1,
            node_id: g.executor_node.clone(),
            issuer_id: g.issuer_id.clone(),
            grant_id: g.grant_id.clone(),
            grant_sha256: report.grant_sha256.clone(),
            job_id: g.job_id.clone(),
            task_id: g.task_id.clone(),
            attempt: 1,
            state: if state == ReceiptState::Completed {
                "completed"
            } else {
                "unknown"
            }
            .into(),
            accepted_at_ms: 1,
            updated_at_ms: 2,
            pipeline_progress: None,
        };
        let result = RetainedResult {
            schema_version: 1,
            receipt,
            max_result_bytes: g.limits.max_result_bytes,
            availability: "retained_final_report".into(),
            report_sha256: Some(
                crate::grants::definition_digest(&serde_json::json!(report)).unwrap(),
            ),
            report: Some(report),
        };
        (g, result)
    }
    #[test]
    fn final_report_correlates_consumption_state_identity_output_bound_and_uncertainty() {
        for state in [ReceiptState::Completed, ReceiptState::Unknown] {
            let (g, good) = fixture(state);
            good.validate().unwrap();
            assert!(good.report.as_ref().unwrap().confirms(&g));
            for (pointer, bad) in [
                ("/report/grant_sha256", serde_json::json!("f".repeat(64))),
                (
                    "/report/session_id",
                    serde_json::json!(uuid::Uuid::nil().to_string()),
                ),
                ("/report/executor_node", serde_json::json!("other")),
                ("/report/persisted", serde_json::json!(false)),
                ("/report/accepted", serde_json::json!(false)),
                (
                    "/report/success",
                    serde_json::json!(state != ReceiptState::Completed),
                ),
                ("/report/data", serde_json::json!("x".repeat(65537))),
                ("/receipt/state", serde_json::json!("accepted")),
                ("/availability", serde_json::json!("recovered")),
                ("/max_result_bytes", serde_json::json!(0)),
            ] {
                let mut value = serde_json::json!(good);
                *value.pointer_mut(pointer).unwrap() = bad;
                // Scope/state/bounds must reject even if a responding node can
                // recompute a checksum; it is not a signature or attestation.
                value["report_sha256"] =
                    serde_json::json!(crate::grants::definition_digest(&value["report"]).unwrap());
                assert!(
                    serde_json::from_value::<RetainedResult>(value)
                        .unwrap()
                        .validate()
                        .is_err(),
                    "{pointer}"
                );
            }
        }
        let (_, mut result) = fixture(ReceiptState::Completed);
        result.report = None;
        result.report_sha256 = None;
        result.receipt.state = "accepted".into();
        result.availability = "no_final_checkpoint".into();
        result.validate().unwrap();
        result.max_result_bytes = 0;
        result.availability = "legacy_metadata_only".into();
        result.validate().unwrap();
        let mut raw = serde_json::json!(result);
        raw["retry"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RetainedResult>(raw).is_err());
    }
    #[test]
    fn result_reports_require_origin_receipts_and_never_claim_reconciliation_or_replay() {
        let (g, result) = fixture(ReceiptState::Completed);
        let cmd = ResultCommand {
            id: g.job_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        let runtime = uuid::Uuid::new_v4().to_string();
        let report = serde_json::json!(ResultReport {
            schema_version: 1,
            report_type: "coordinator_request_result".into(),
            success: true,
            runtime_id: runtime.clone(),
            id: cmd.id.clone(),
            revision: 5,
            request_context: g.request_context.clone(),
            coordinator_attempt_state: "unknown".into(),
            result,
            execution_requested: false,
            automatically_retried: false,
            reconciled: false,
            note: "Read-only evidence".into()
        });
        assert!(confirmed(&report, &cmd, Some("owner"), Some(&runtime)));
        assert!(!confirmed(&report, &cmd, Some("other"), Some(&runtime)));
        assert!(!confirmed(&report, &cmd, None, Some("other-runtime")));
        for (field, bad) in [
            ("reconciled", serde_json::json!(true)),
            ("execution_requested", serde_json::json!(true)),
            ("automatically_retried", serde_json::json!(true)),
            ("revision", serde_json::json!(0)),
            ("coordinator_attempt_state", serde_json::json!("recovered")),
            ("note", serde_json::json!("x".repeat(2049))),
        ] {
            let mut value = report.clone();
            value[field] = bad;
            assert!(!confirmed(&value, &cmd, None, None), "{field}");
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = serde_json::json!({"schema_version":1,"report_type":"request_result","node_id":"owner","request_id":request_id,
            "id":cmd.id,"grant_id":cmd.grant_id,"success":true,"execution_requested":false,"automatically_retried":false,"reconciled":false,
            "result":report,"note":"Explicit read"});
        assert!(confirmed_node(&envelope, &cmd, "owner", &request_id));
        assert!(!confirmed_node(&envelope, &cmd, "other", &request_id));
        let mut forged = envelope.clone();
        forged["request_id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed_node(&forged, &cmd, "owner", &request_id));
        let mut forged = envelope;
        forged["retry"] = serde_json::json!(true);
        assert!(!confirmed_node(&forged, &cmd, "owner", &request_id));
    }
    #[test]
    fn result_wire_scope_is_separate_strict_and_bound_to_current_issuer_and_node() {
        let query = serde_json::json!({"type":"grant_result","id":uuid::Uuid::new_v4().simple().to_string(),
            "expected_node_id":"owner","expected_issuer_id":"a".repeat(64),"expected_grant_sha256":"b".repeat(64),
            "grant_id":uuid::Uuid::new_v4().to_string()});
        let good: WireRequest = serde_json::from_value(query.clone()).unwrap();
        let receipt = good.receipt_query().unwrap();
        receipt.validate("owner", &"a".repeat(64)).unwrap();
        assert!(receipt.validate("other", &"a".repeat(64)).is_err());
        assert!(receipt.validate("owner", &"c".repeat(64)).is_err());
        for (field, bad) in [
            ("type", serde_json::json!("grant_receipt")),
            ("expected_grant_sha256", serde_json::json!("z".repeat(64))),
        ] {
            let mut raw = query.clone();
            raw[field] = bad;
            assert!(
                serde_json::from_value::<WireRequest>(raw)
                    .unwrap()
                    .receipt_query()
                    .is_err()
            );
        }
        let mut raw = query;
        raw["execute"] = serde_json::json!(true);
        assert!(serde_json::from_value::<WireRequest>(raw).is_err());
    }
}
