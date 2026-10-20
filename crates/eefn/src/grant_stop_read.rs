//! Read-only stop evidence. Never signals a watcher, initializes or repairs a store.
use super::*;
pub const NOTE: &str = "Read-only retained stop intent, not executor termination, rollback or proof no effects occurred. Missing evidence never proves no stop/work was accepted. No stop signal, execution, retry, reconciliation, repair or permission change was requested.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    pub schema_version: u32,
    pub available: bool,
    pub guarantee: String,
}
impl Advertisement {
    pub fn new(available: bool) -> Self {
        Self {
            schema_version: 1,
            available,
            guarantee: "read_only_intent_evidence".into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.guarantee != "read_only_intent_evidence" {
            bail!("Unsupported stop inspection advertisement")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub schema_version: u32,
    pub scope: Scope,
    pub recorded_at_ms: u64,
    pub receipt_preexisted: bool,
    pub persisted: bool,
    pub stop_requested: bool,
    pub execution_stopped: bool,
    pub external_effects_stopped: bool,
}
impl Evidence {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        if self.schema_version != 1
            || self.recorded_at_ms == 0
            || self.recorded_at_ms > i64::MAX as u64
            || !self.persisted
            || !self.stop_requested
            || self.execution_stopped
            || self.external_effects_stopped
        {
            bail!("Invalid retained stop evidence")
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Missing,
    Retained,
}
fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub schema_version: u32,
    pub report_type: String,
    pub node_id: String,
    pub grant_id: String,
    #[serde(deserialize_with = "required_option")]
    pub scope: Option<Scope>,
    pub availability: Availability,
    #[serde(deserialize_with = "required_option")]
    pub evidence: Option<Evidence>,
    pub read_only: bool,
    pub execution_requested: bool,
    pub stop_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
impl Inspection {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.node_id)?;
        crate::automation_occurrence::uuid(&self.grant_id)?;
        if self.schema_version != 1
            || self.report_type != "grant_stop_inspection"
            || !self.read_only
            || self.execution_requested
            || self.stop_requested
            || self.automatically_retried
            || self.reconciled
            || self.note != NOTE
            || (self.availability == Availability::Retained) != self.evidence.is_some()
        {
            bail!("Unconfirmed read-only stop inspection")
        }
        if let Some(scope) = &self.scope {
            scope.validate()?;
            if scope.executor_node != self.node_id || scope.grant_id != self.grant_id {
                bail!("Inspection scope changed")
            }
        }
        if let Some(e) = &self.evidence {
            e.validate()?;
            if self.scope.as_ref() != Some(&e.scope) {
                bail!("Stop evidence differs from inspection scope")
            }
        }
        Ok(())
    }
    pub fn confirms(&self, scope: &Scope) -> bool {
        scope.validate().is_ok() && self.validate().is_ok() && self.scope.as_ref() == Some(scope)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub scope: Scope,
}
impl WireRequest {
    pub fn validate(&self, issuer: &str, node: &str) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        self.scope.validate()?;
        // Original session belongs to historical evidence, not current authority.
        // Current TLS must still authenticate the same issuer and exact node.
        if self.kind != "inspect_grant_stop"
            || self.scope.issuer_id != issuer
            || self.scope.executor_node != node
        {
            bail!("Stop inspection requires the original pinned issuer and exact node")
        }
        Ok(())
    }
}
pub fn read_inspection(
    db: &Connection,
    node: &str,
    issuer: Option<&str>,
    id: &str,
    expected: Option<&Scope>,
) -> Result<Inspection> {
    crate::network::validate_remote_node_id(node)?;
    crate::automation_occurrence::uuid(id)?;
    if issuer.is_some_and(|s| !hex_digest(s)) {
        bail!("Invalid stop inspection issuer")
    }
    if let Some(s) = expected {
        s.validate()?;
        if s.executor_node != node || s.grant_id != id || issuer.is_some_and(|i| i != s.issuer_id) {
            bail!("Stop query scope changed")
        }
    }
    let tx = db.unchecked_transaction()?;
    crate::request_receipt::validate_owner(&tx, node)?;
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM execution_receipts WHERE grant_id=?1",
        [id],
        |r| r.get(0),
    )?;
    let consumed = if count == 1 {
        Some(crate::request_receipt::read_consumed(
            &tx, node, issuer, id,
        )?)
    } else if count == 0 {
        None
    } else {
        bail!("Duplicate consumption evidence")
    };
    if let (Some(scope), Some(receipt)) = (expected, &consumed) {
        if scope.issuer_id != receipt.issuer_id
            || scope.grant_sha256 != receipt.grant_sha256
            || scope.job_id != receipt.job_id
            || scope.task_id != receipt.task_id
            || scope.attempt != receipt.attempt
        {
            bail!("Stop query differs from consumed scope")
        }
    }
    let saved = read(&tx, id)?;
    let evidence = if let Some(r) = saved {
        let receipt = consumed
            .as_ref()
            .context("Stop evidence lacks consumed authority")?;
        let scope = Scope {
            grant_id: r.grant_id,
            grant_sha256: r.grant_sha256,
            issuer_id: r.issuer_id,
            session_id: r.session_id,
            executor_node: r.executor_node,
            job_id: r.job_id,
            task_id: r.task_id,
            attempt: r.attempt,
        };
        scope.validate()?;
        if scope.executor_node != node
            || scope.issuer_id != receipt.issuer_id
            || scope.grant_sha256 != receipt.grant_sha256
            || scope.job_id != receipt.job_id
            || scope.task_id != receipt.task_id
            || scope.attempt != receipt.attempt
            || expected.is_some_and(|s| *s != scope)
        {
            bail!("Retained stop evidence scope differs")
        }
        Some(Evidence {
            schema_version: 1,
            scope,
            recorded_at_ms: r.recorded_at_ms,
            receipt_preexisted: r.receipt_preexisted,
            persisted: true,
            stop_requested: true,
            execution_stopped: false,
            external_effects_stopped: false,
        })
    } else {
        None
    };
    let result = Inspection {
        schema_version: 1,
        report_type: "grant_stop_inspection".into(),
        node_id: node.into(),
        grant_id: id.into(),
        scope: expected
            .cloned()
            .or_else(|| evidence.as_ref().map(|e| e.scope.clone())),
        availability: if evidence.is_some() {
            Availability::Retained
        } else {
            Availability::Missing
        },
        evidence,
        read_only: true,
        execution_requested: false,
        stop_requested: false,
        automatically_retried: false,
        reconciled: false,
        note: NOTE.into(),
    };
    result.validate()?;
    tx.commit()?;
    Ok(result)
}
impl GrantLedger {
    pub fn inspect_stop(&self, scope: &Scope, issuer: &str, node: &str) -> Result<Inspection> {
        if node != self.node_id {
            bail!("Stop inspection targets another journal")
        }
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        read_inspection(&db, node, Some(issuer), &scope.grant_id, Some(scope))
    }
}
impl crate::NodeService {
    pub fn local_stop_receipt(&self, request: super::super::ReceiptRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Stop inspection targets another node")
        }
        let db = Connection::open_with_flags(
            super::super::journal_path(&self.config_path),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(Duration::from_millis(200))?;
        Ok(json!(read_inspection(
            &db,
            &self.node_id,
            None,
            &request.grant_id,
            None
        )?))
    }
}

pub fn render(value: &Inspection) -> Result<String> {
    value.validate()?;
    let summary = if value.evidence.is_some() {
        "Stop intent is retained."
    } else {
        "No retained stop record was found; missing evidence does not prove no stop/work occurred."
    };
    Ok(format!(
        "Stop receipt on node {} / grant {}\n{}\n{}\n",
        value.node_id, value.grant_id, summary, NOTE
    ))
}

#[cfg(test)]
#[path = "grant_stop_read_tests.rs"]
mod tests;
