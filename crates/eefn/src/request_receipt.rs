//! Read-only recovery evidence. A consumed receipt is never a success/output or retry authorization.
use anyhow::{Context, Result, bail};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedReceipt {
    pub schema_version: u32,
    pub node_id: String,
    pub issuer_id: String,
    pub grant_id: String,
    pub grant_sha256: String,
    pub job_id: String,
    pub task_id: String,
    pub attempt: u32,
    pub state: String,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_progress: Option<crate::pipeline_progress::PipelineProgress>,
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl ConsumedReceipt {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        crate::request_drafts::validate_id(&self.job_id)?;
        crate::request_drafts::validate_id(&self.task_id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if self.schema_version != 1
            || uuid::Uuid::parse_str(&self.grant_id)?.is_nil()
            || !digest(&self.issuer_id)
            || !digest(&self.grant_sha256)
            || !(1..=11).contains(&self.attempt)
            || !matches!(self.state.as_str(), "accepted" | "completed" | "unknown")
            || self.accepted_at_ms == 0
            || self.updated_at_ms < self.accepted_at_ms
            || self.updated_at_ms > i64::MAX as u64
            || self.pipeline_progress.as_ref().is_some_and(|progress| {
                progress.validate().is_err()
                    || progress.recorded_at_ms < self.accepted_at_ms
                    || progress.recorded_at_ms > self.updated_at_ms
            })
        {
            bail!("Damaged consumed receipt; retained without recovery or replay")
        }
        Ok(())
    }
    pub fn confirms(&self, grant: &crate::grants::ExecutionGrant) -> bool {
        self.validate().is_ok()
            && grant.validate().is_ok()
            && self.node_id == grant.executor_node
            && self.issuer_id == grant.issuer_id
            && self.grant_id == grant.grant_id
            && self.job_id == grant.job_id
            && self.task_id == grant.task_id
            && self.attempt == grant.attempt
            && crate::grants::definition_digest(&json!(grant)).is_ok_and(|d| d == self.grant_sha256)
            && self
                .pipeline_progress
                .as_ref()
                .is_none_or(|progress| progress.confirms(grant))
    }
}
/// No writes/repair/pruning. Owner/version and all bounded identity fields are validated even during local inspection.
pub(crate) fn read_consumed(
    db: &rusqlite::Connection,
    node: &str,
    issuer: Option<&str>,
    id: &str,
) -> Result<ConsumedReceipt> {
    crate::job_commands::validate_operation_id(id)?;
    if issuer.is_some_and(|s| !digest(s)) {
        bail!("Invalid receipt issuer")
    }
    validate_owner(db, node)?;
    let (progress, join) = crate::pipeline_progress::projection(db)?;
    let sql = format!(
        "SELECT issuer_id,execution_receipts.grant_digest,job_id,task_id,attempt,state,accepted_ms,updated_ms,{progress} FROM execution_receipts{join} WHERE execution_receipts.grant_id=?1 AND (?2 IS NULL OR issuer_id=?2)"
    );
    let (mut value, progress_digest, progress_payload, progress_checksum) = db
        .query_row(&sql, rusqlite::params![id, issuer], |r| {
            let timestamp = |column| -> rusqlite::Result<u64> {
                r.get::<_, i64>(column)?.try_into().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        column,
                        rusqlite::types::Type::Integer,
                        Box::new(e),
                    )
                })
            };
            Ok((
                ConsumedReceipt {
                    schema_version: 1,
                    node_id: node.into(),
                    grant_id: id.into(),
                    issuer_id: r.get(0)?,
                    grant_sha256: r.get(1)?,
                    job_id: r.get(2)?,
                    task_id: r.get(3)?,
                    attempt: r.get(4)?,
                    state: r.get(5)?,
                    accepted_at_ms: timestamp(6)?,
                    updated_at_ms: timestamp(7)?,
                    pipeline_progress: None,
                },
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<String>>(10)?,
            ))
        })
        .optional()?
        .context("Consumed receipt unavailable for this issuer")?;
    value.pipeline_progress = crate::pipeline_progress::decode(
        progress_digest,
        progress_payload,
        progress_checksum,
        &value.grant_sha256,
    )?;
    value.validate()?;
    Ok(value)
}
pub(crate) fn validate_owner(db: &rusqlite::Connection, node: &str) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let owner: String = db.query_row(
        "SELECT node_id FROM execution_receipt_owner WHERE owner='node'",
        [],
        |r| r.get(0),
    )?;
    if !matches!(version, 1 | 2 | 3) || owner != node {
        bail!("Receipt journal version/owner changed")
    }
    crate::grants::workload::validate_schema(db, version)?;
    Ok(())
}

/// Adapter selection only; every path remains a distinct read-only service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    Receipt,
    Result,
    Stop,
}
impl QueryKind {
    pub fn path(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Result => "result",
            Self::Stop => "stop-receipt",
        }
    }
    pub fn report_type(self) -> &'static str {
        match self {
            Self::Receipt => "request_receipt",
            Self::Result => "request_result",
            Self::Stop => "request_stop_receipt",
        }
    }
    pub fn error_code(self) -> &'static str {
        match self {
            Self::Receipt => "receipt_unavailable",
            Self::Result => "result_unavailable",
            Self::Stop => "stop_receipt_unavailable",
        }
    }
    pub fn confirmed(
        self,
        value: &Value,
        cmd: &ReceiptCommand,
        origin: Option<&str>,
        runtime: Option<&str>,
    ) -> bool {
        match self {
            Self::Receipt => confirmed(value, cmd, origin, runtime),
            Self::Result => crate::request_result::confirmed(value, cmd, origin, runtime),
            Self::Stop => crate::request_stop_receipt::confirmed(value, cmd, origin, runtime),
        }
    }
    pub fn confirmed_node(
        self,
        value: &Value,
        cmd: &ReceiptCommand,
        node: &str,
        request: &str,
    ) -> bool {
        match self {
            Self::Receipt => confirmed_node(value, cmd, node, request),
            Self::Result => crate::request_result::confirmed_node(value, cmd, node, request),
            Self::Stop => crate::request_stop_receipt::confirmed_node(value, cmd, node, request),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub expected_node_id: String,
    pub expected_issuer_id: String,
    pub grant_id: String,
}
impl WireRequest {
    pub fn validate(&self, node: &str, issuer: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if self.kind != "grant_receipt"
            || self.expected_node_id != node
            || self.expected_issuer_id != issuer
            || !digest(issuer)
        {
            bail!("Receipt query requires this pinned issuer and exact node")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptCommand {
    pub id: String,
    pub grant_id: String,
}
impl ReceiptCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if uuid::Uuid::parse_str(&self.grant_id)?.is_nil() {
            bail!("Invalid grant ID")
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub command: ReceiptCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub request_context: crate::context::RequestContext,
    pub coordinator_attempt_state: String,
    pub receipt: ConsumedReceipt,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    cmd: &ReceiptCommand,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<ReceiptReport>(value.clone()) else {
        return false;
    };
    cmd.validate().is_ok()
        && r.receipt.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_receipt"
        && r.success
        && r.id == cmd.id
        && r.receipt.job_id == cmd.id
        && r.receipt.grant_id == cmd.grant_id
        && r.revision > 0
        && r.note.len() <= 2048
        && crate::job_commands::validate_operation_id(&r.runtime_id).is_ok()
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
pub fn confirmed_node(value: &Value, cmd: &ReceiptCommand, node: &str, request_id: &str) -> bool {
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
        && r.report_type == "request_receipt"
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
            r.result.is_none() && r.error_code.as_deref() == Some("receipt_unavailable")
        }
}
impl crate::NodeService {
    pub async fn request_receipt(&self, req: ReceiptRequest) -> Result<Value> {
        if req.schema_version != 1 || req.expected_node_id != self.node_id {
            bail!("Receipt query schema/node changed")
        }
        req.command.validate()?;
        crate::job_commands::validate_operation_id(&req.request_id)?;
        let result = self
            .submissions
            .request_correlated(
                "requests.receipt",
                json!(req.command),
                Some(req.request_id.clone()),
            )
            .await;
        let mut report = json!({"schema_version":1,"report_type":"request_receipt","node_id":self.node_id,
            "request_id":req.request_id,"id":req.command.id,"grant_id":req.command.grant_id,"success":false,
            "execution_requested":false,"automatically_retried":false,"reconciled":false});
        match result {
            Ok(value) if confirmed(&value, &req.command, Some(&self.node_id), None) => {
                report["success"] = json!(true);
                report["result"] = value;
            }
            _ => {
                report["error_code"] = json!("receipt_unavailable");
            }
        }
        report["note"] = json!(
            "Read-only evidence, not output/success or proof effects stopped. No job change, reconciliation, retry or fallback."
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn consumed() -> (crate::grants::ExecutionGrant, ConsumedReceipt) {
        let g = crate::grants::tests::grant();
        let r = ConsumedReceipt {
            schema_version: 1,
            node_id: g.executor_node.clone(),
            issuer_id: g.issuer_id.clone(),
            grant_id: g.grant_id.clone(),
            grant_sha256: crate::grants::definition_digest(&json!(g)).unwrap(),
            job_id: g.job_id.clone(),
            task_id: g.task_id.clone(),
            attempt: g.attempt,
            state: "accepted".into(),
            accepted_at_ms: g.issued_at_ms + 1,
            updated_at_ms: g.issued_at_ms + 1,
            pipeline_progress: None,
        };
        (g, r)
    }
    #[test]
    fn exact_scope_and_bounded_receipt_fields_reject_corruption_and_authority() {
        let (g, r) = consumed();
        assert!(r.confirms(&g));
        for (field, bad) in [
            ("schema_version", json!(2)),
            ("node_id", json!("other")),
            ("issuer_id", json!("c".repeat(64))),
            ("grant_sha256", json!("d".repeat(64))),
            ("job_id", json!(uuid::Uuid::new_v4().simple().to_string())),
            ("task_id", json!(uuid::Uuid::new_v4().simple().to_string())),
            ("attempt", json!(12)),
            ("state", json!("rejected")),
            ("accepted_at_ms", json!(0)),
            ("updated_at_ms", json!(r.accepted_at_ms - 1)),
        ] {
            let mut bad_r = json!(r);
            bad_r[field] = bad;
            assert!(
                !serde_json::from_value::<ConsumedReceipt>(bad_r).is_ok_and(|r| r.confirms(&g)),
                "{field}"
            );
        }
        let mut raw = json!(r);
        raw["success"] = json!(true);
        assert!(serde_json::from_value::<ConsumedReceipt>(raw).is_err());
        let mut changed = g;
        changed.priority += 1;
        assert!(!r.confirms(&changed));
    }
    #[test]
    fn reads_validate_owner_issuer_and_damaged_rows_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.sqlite3");
        let (g, _) = consumed();
        let ledger = crate::grants::GrantLedger::open(&path, Default::default(), "owner").unwrap();
        ledger.claim(&g).unwrap();
        drop(ledger);
        let before = std::fs::read(&path).unwrap();
        let db = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let r = read_consumed(&db, "owner", Some(&g.issuer_id), &g.grant_id).unwrap();
        assert!(r.confirms(&g));
        assert!(read_consumed(&db, "other", Some(&g.issuer_id), &g.grant_id).is_err());
        assert!(read_consumed(&db, "owner", Some(&"e".repeat(64)), &g.grant_id).is_err());
        assert!(read_consumed(&db, "owner", None, &uuid::Uuid::new_v4().to_string()).is_err());
        drop(db);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let db = rusqlite::Connection::open(&path).unwrap();
        for (field, bad, good) in [
            ("job_id", json!("not-a-job"), json!(r.job_id)),
            ("grant_digest", json!("z".repeat(64)), json!(r.grant_sha256)),
            ("attempt", json!(0), json!(r.attempt)),
            ("accepted_ms", json!(0), json!(r.accepted_at_ms)),
            (
                "updated_ms",
                json!(r.accepted_at_ms - 1),
                json!(r.updated_at_ms),
            ),
        ] {
            let sql = format!("UPDATE execution_receipts SET {field}=?1 WHERE grant_id=?2");
            let raw = |v: &Value| {
                if let Some(s) = v.as_str() {
                    rusqlite::types::Value::Text(s.into())
                } else {
                    rusqlite::types::Value::Integer(v.as_i64().unwrap())
                }
            };
            db.execute(&sql, rusqlite::params![raw(&bad), g.grant_id])
                .unwrap();
            assert!(
                read_consumed(&db, "owner", None, &g.grant_id).is_err(),
                "{field}"
            );
            db.execute(&sql, rusqlite::params![raw(&good), g.grant_id])
                .unwrap();
        }
        assert_eq!(read_consumed(&db, "owner", None, &g.grant_id).unwrap(), r);
    }
    #[test]
    fn confirmed_reports_never_claim_execution_reconciliation_success_output_or_replay() {
        let (g, r) = consumed();
        let cmd = ReceiptCommand {
            id: g.job_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        let report = json!(ReceiptReport {
            schema_version: 1,
            report_type: "coordinator_request_receipt".into(),
            success: true,
            runtime_id: uuid::Uuid::new_v4().to_string(),
            id: g.job_id.clone(),
            revision: 6,
            request_context: g.request_context.clone(),
            coordinator_attempt_state: "unknown".into(),
            receipt: r,
            execution_requested: false,
            automatically_retried: false,
            reconciled: false,
            note: "Evidence only".into()
        });
        assert!(confirmed(
            &report,
            &cmd,
            Some("owner"),
            report["runtime_id"].as_str()
        ));
        for (field, bad) in [
            ("reconciled", json!(true)),
            ("execution_requested", json!(true)),
            ("automatically_retried", json!(true)),
            ("revision", json!(0)),
            ("coordinator_attempt_state", json!("recovered")),
            ("note", json!("x".repeat(2049))),
        ] {
            let mut raw = report.clone();
            raw[field] = bad;
            assert!(!confirmed(&raw, &cmd, None, None));
        }
        assert!(!confirmed(&report, &cmd, Some("other"), None));
        assert!(!confirmed(&report, &cmd, None, Some("other-runtime")));
        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = json!({"schema_version":1,"report_type":"request_receipt","node_id":"owner","request_id":request_id,
            "id":cmd.id,"grant_id":cmd.grant_id,"success":true,"execution_requested":false,"automatically_retried":false,
            "reconciled":false,"result":report,"note":"Read only"});
        assert!(confirmed_node(&envelope, &cmd, "owner", &request_id));
        let mut forged = envelope.clone();
        forged["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed_node(&forged, &cmd, "owner", &request_id));
        let mut forged = envelope;
        forged["output"] = json!("fake");
        assert!(!confirmed_node(&forged, &cmd, "owner", &request_id));
        let mut raw = json!(cmd);
        raw["retry"] = json!(true);
        assert!(serde_json::from_value::<ReceiptCommand>(raw).is_err());
    }
    #[test]
    fn distinct_wire_query_requires_exact_current_pinned_issuer_node_and_strict_shape() {
        let issuer = "a".repeat(64);
        let query = json!({"type":"grant_receipt","id":uuid::Uuid::new_v4().simple().to_string(),
            "expected_node_id":"owner","expected_issuer_id":issuer,"grant_id":uuid::Uuid::new_v4().to_string()});
        let wire: WireRequest = serde_json::from_value(query.clone()).unwrap();
        wire.validate("owner", &issuer).unwrap();
        assert!(wire.validate("other", &issuer).is_err());
        assert!(wire.validate("owner", &"b".repeat(64)).is_err());
        let mut raw = query;
        raw["params"] = json!({});
        assert!(serde_json::from_value::<WireRequest>(raw).is_err());
    }
}
