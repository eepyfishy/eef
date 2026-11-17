//! Analysis intents and bounded results in the existing consumed-grant journal.
//! No image, model process, lease renewal or replay factory enters this worker.
use super::*;
use crate::workload_leases::definition::{Source, vision::Parameters};

const STATE_SQL: &str = "CREATE TABLE execution_workload_analysis_state(grant_id TEXT NOT NULL PRIMARY KEY REFERENCES execution_receipts(grant_id),grant_digest TEXT NOT NULL,call_count INTEGER NOT NULL,result_count INTEGER NOT NULL,result_bytes INTEGER NOT NULL,reserved_output_tokens INTEGER NOT NULL,prefix_sha256 TEXT NOT NULL)";
const SQL: &str = "CREATE TABLE execution_workload_analyses(grant_id TEXT NOT NULL REFERENCES execution_workload_analysis_state(grant_id),sequence INTEGER NOT NULL CHECK(sequence>0),source_sequence INTEGER NOT NULL CHECK(source_sequence>0),intent TEXT NOT NULL,intent_sha256 TEXT NOT NULL,result TEXT,result_sha256 TEXT,PRIMARY KEY(grant_id,sequence),UNIQUE(grant_id,source_sequence))";
const HEADROOM: usize = 1024;
fn hash(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}
fn parameters(d: &Definition) -> Result<&Parameters> {
    match &d.source {
        Source::CameraVision { parameters, .. } => Ok(parameters),
        _ => bail!("No reviewed vision model scope"),
    }
}
pub(super) fn schema(db: &Connection, version: i64) -> Result<()> {
    let found=db.prepare("SELECT name,type,CASE WHEN length(CAST(sql AS BLOB))<=2048 THEN sql ELSE NULL END FROM sqlite_master WHERE lower(name) IN ('execution_workload_analyses','execution_workload_analysis_state') ORDER BY name")?
        .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        if version == 6 {
            found
                == [
                    (
                        "execution_workload_analyses".into(),
                        "table".into(),
                        Some(SQL.into()),
                    ),
                    (
                        "execution_workload_analysis_state".into(),
                        "table".into(),
                        Some(STATE_SQL.into()),
                    ),
                ]
        } else {
            found.is_empty()
        },
        "Vision journal schema changed or downgraded; retained without repair"
    );
    Ok(())
}
pub(super) fn upgrade(db: &Connection, node: &str) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    schema(db, version)?;
    if version == 6 {
        return Ok(());
    }
    // Uses the same IMMEDIATE admission transaction. Validate every old receipt
    // before adding a table; metadata-only admission never triggers this change.
    super::completion::upgrade(db, node)?;
    db.execute_batch(STATE_SQL)?;
    db.execute_batch(SQL)?;
    db.execute_batch("PRAGMA user_version=6")?;
    super::validate_schema(db, 6)
}
pub(super) fn reservation(db: &Connection, d: &Definition) -> Result<usize> {
    let Ok(p) = parameters(d) else { return Ok(0) };
    p.validate()?;
    let page = usize::try_from(db.query_row::<i64, _, _>("PRAGMA page_size", [], |r| r.get(0))?)?;
    ensure!(
        page > 0 && page.is_power_of_two(),
        "Invalid vision journal page size"
    );
    let row = p
        .inference
        .max_result_bytes
        .checked_add(2 * HEADROOM)
        .context("Vision reservation overflow")?
        .div_ceil(page)
        .checked_add(8)
        .and_then(|n| n.checked_mul(page))
        .context("Vision reservation overflow")?;
    usize::try_from(p.max_calls())?
        .checked_mul(row)
        .and_then(|n| n.checked_add(8 * page))
        .context("Vision reservation overflow")
}
pub(super) fn audit_orphans(db: &Connection) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    schema(db, version)?;
    if version < 6 {
        return Ok(());
    }
    let bad:i64=db.query_row("SELECT (SELECT count(*) FROM execution_workload_analysis_state s LEFT JOIN execution_receipts r ON r.grant_id=s.grant_id WHERE r.grant_id IS NULL OR r.receipt_kind!='workload' OR r.grant_digest!=s.grant_digest)+(SELECT count(*) FROM execution_workload_analyses a LEFT JOIN execution_workload_analysis_state s ON s.grant_id=a.grant_id WHERE s.grant_id IS NULL)",[],|r|r.get(0))?;
    ensure!(bad == 0, "Orphaned or cross-kind vision analysis evidence");
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Intent {
    schema_version: u32,
    grant_sha256: String,
    pub sequence: u64,
    pub source_sequence: u64,
    image_sha256: String,
    image_bytes: usize,
    lease_sequence: u64,
    recorded_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalysisResult {
    schema_version: u32,
    sequence: u64,
    intent_sha256: String,
    pub text: String,
    lease_sequence: u64,
    recorded_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub calls: u64,
    pub completed: u64,
    pub result_bytes: usize,
    pub reserved_output_tokens: u64,
    pub prefix_sha256: String,
    pub last_source_sequence: u64,
    pub last_lease_sequence: u64,
}
impl Summary {
    pub(super) fn validate_complete(&self, d: &Definition, images: u64, lease: u64) -> Result<()> {
        let p = parameters(d)?;
        ensure!(
            self.calls == images
                && self.completed == self.calls
                && self.calls <= p.max_calls()
                && self.reserved_output_tokens
                    == self
                        .calls
                        .checked_mul(p.inference.max_tokens)
                        .context("Vision token reservation overflow")?
                && self.result_bytes
                    <= usize::try_from(self.completed)?
                        .checked_mul(p.inference.max_result_bytes)
                        .context("Vision result reservation overflow")?
                && self.result_bytes >= usize::try_from(self.completed)?.saturating_mul(3)
                && crate::request_result::valid_digest(&self.prefix_sha256)
                && self.last_lease_sequence <= lease
                && if self.calls == 0 {
                    self.last_source_sequence == 0
                        && self.last_lease_sequence == 0
                        && self.prefix_sha256 == hash(d.scope()?.grant_sha256.as_bytes())
                } else {
                    (2..=u64::from(p.capture.max_frames)).contains(&self.last_source_sequence)
                        && self.last_lease_sequence > 0
                },
            "Vision completion differs from reviewed analysis scope"
        );
        Ok(())
    }
}
struct Row {
    intent: Intent,
    checksum: String,
    result: Option<AnalysisResult>,
    result_checksum: Option<String>,
}
fn rows(db: &Connection, receipt: &Receipt) -> Result<Vec<Row>> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    schema(db, version)?;
    if version < 6 {
        ensure!(
            receipt.definition.schema_version == 2,
            "Vision journal is unavailable"
        );
        return Ok(Vec::new());
    }
    let p = parameters(&receipt.definition).ok();
    let maximum = p.map_or(0, |p| p.max_calls());
    let result_bound = p.map_or(0, |p| p.inference.max_result_bytes) + HEADROOM;
    let mut query=db.prepare("SELECT sequence,source_sequence,length(CAST(intent AS BLOB)),CASE WHEN length(CAST(intent AS BLOB))<=?2 THEN intent ELSE NULL END,CASE WHEN length(CAST(intent_sha256 AS BLOB))=64 THEN intent_sha256 ELSE NULL END,length(CAST(result AS BLOB)),CASE WHEN length(CAST(result AS BLOB))<=?3 THEN result ELSE NULL END,result IS NOT NULL,result_sha256 IS NOT NULL,CASE WHEN length(CAST(result_sha256 AS BLOB))=64 THEN result_sha256 ELSE NULL END FROM execution_workload_analyses WHERE grant_id=?1 ORDER BY sequence LIMIT ?4")?;
    let mut records = query.query(params![
        receipt.definition.grant_id,
        i64::try_from(HEADROOM)?,
        i64::try_from(result_bound)?,
        i64::try_from(maximum + 1)?
    ])?;
    let mut output = Vec::new();
    while let Some(row) = records.next()? {
        ensure!(
            (output.len() as u64) < maximum,
            "Vision analysis count exceeds reviewed bound"
        );
        let sequence = u64::try_from(row.get::<_, i64>(0)?)?;
        let position = u64::try_from(row.get::<_, i64>(1)?)?;
        let size: i64 = row.get(2)?;
        ensure!(
            (1..=HEADROOM as i64).contains(&size),
            "Oversized vision intent"
        );
        let raw: String = row
            .get::<_, Option<String>>(3)?
            .context("Vision intent unavailable")?;
        let checksum: String = row
            .get::<_, Option<String>>(4)?
            .context("Vision intent checksum unavailable")?;
        let intent: Intent = serde_json::from_str(&raw)?;
        ensure!(
            serde_json::to_string(&intent)? == raw
                && hash(raw.as_bytes()) == checksum
                && intent.schema_version == 1
                && intent.sequence == sequence
                && intent.source_sequence == position,
            "Damaged vision intent"
        );
        let has_result: bool = row.get(7)?;
        let has_checksum: bool = row.get(8)?;
        ensure!(
            has_result == has_checksum,
            "Incomplete vision result evidence"
        );
        let result_checksum: Option<String> = row.get(9)?;
        let result = if has_result {
            let size: i64 = row.get(5)?;
            ensure!(
                (1..=i64::try_from(result_bound)?).contains(&size),
                "Oversized vision result"
            );
            let raw: String = row
                .get::<_, Option<String>>(6)?
                .context("Vision result unavailable")?;
            let result: AnalysisResult = serde_json::from_str(&raw)?;
            ensure!(
                serde_json::to_string(&result)? == raw
                    && result_checksum.as_deref() == Some(hash(raw.as_bytes()).as_str()),
                "Damaged vision result"
            );
            Some(result)
        } else {
            None
        };
        output.push(Row {
            intent,
            checksum,
            result,
            result_checksum,
        });
    }
    Ok(output)
}
fn calculated(db: &Connection, receipt: &Receipt) -> Result<Summary> {
    let d = &receipt.definition;
    let mut summary = Summary {
        calls: 0,
        completed: 0,
        result_bytes: 0,
        reserved_output_tokens: 0,
        prefix_sha256: hash(d.scope()?.grant_sha256.as_bytes()),
        last_source_sequence: 0,
        last_lease_sequence: 0,
    };
    let p = parameters(d).ok();
    let mut previous_time = receipt.receipt.accepted_at_ms;
    for row in rows(db, receipt)? {
        let p = p.context("Analysis rows on metadata-only workload")?;
        let i = &row.intent;
        ensure!(
            summary.calls == summary.completed
                && i.grant_sha256 == d.scope()?.grant_sha256
                && i.sequence == summary.calls + 1
                && i.source_sequence > summary.last_source_sequence
                && i.source_sequence >= 2
                && i.source_sequence <= u64::from(p.capture.max_frames)
                && (4..=p.images.max_image_bytes).contains(&i.image_bytes)
                && crate::request_result::valid_digest(&i.image_sha256)
                && i.lease_sequence > 0
                && i.lease_sequence >= summary.last_lease_sequence
                && i.recorded_at_ms >= previous_time
                && i.recorded_at_ms <= receipt.receipt.updated_at_ms,
            "Vision intent position, timing or scope changed"
        );
        summary.calls += 1;
        summary.last_source_sequence = i.source_sequence;
        summary.last_lease_sequence = i.lease_sequence;
        previous_time = i.recorded_at_ms;
        summary.reserved_output_tokens = summary
            .reserved_output_tokens
            .checked_add(p.inference.max_tokens)
            .context("Vision token reservation overflow")?;
        if let Some(result) = &row.result {
            ensure!(
                result.schema_version == 1
                    && result.sequence == i.sequence
                    && result.intent_sha256 == row.checksum
                    && !result.text.trim().is_empty()
                    && serde_json::to_vec(&result.text)?.len() <= p.inference.max_result_bytes
                    && !result.text.contains('\0')
                    && result.lease_sequence >= i.lease_sequence
                    && result.recorded_at_ms >= previous_time
                    && result.recorded_at_ms <= receipt.receipt.updated_at_ms,
                "Vision result position, timing or scope changed"
            );
            summary.completed += 1;
            summary.result_bytes += serde_json::to_vec(&result.text)?.len();
            summary.last_lease_sequence = result.lease_sequence;
            previous_time = result.recorded_at_ms;
        }
        summary.prefix_sha256 = hash(
            format!(
                "{}{}{}",
                summary.prefix_sha256,
                row.checksum,
                row.result_checksum.as_deref().unwrap_or("pending")
            )
            .as_bytes(),
        );
    }
    Ok(summary)
}

pub(super) fn initialize(db: &Connection, d: &Definition) -> Result<()> {
    parameters(d)?;
    let digest = d.scope()?.grant_sha256;
    db.execute(
        "INSERT INTO execution_workload_analysis_state VALUES(?1,?2,0,0,0,0,?3)",
        params![d.grant_id, digest, hash(digest.as_bytes())],
    )?;
    Ok(())
}
fn store_summary(db: &Connection, d: &Definition, summary: &Summary) -> Result<()> {
    let changed=db.execute("UPDATE execution_workload_analysis_state SET call_count=?3,result_count=?4,result_bytes=?5,reserved_output_tokens=?6,prefix_sha256=?7 WHERE grant_id=?1 AND grant_digest=?2",
        params![d.grant_id,d.scope()?.grant_sha256,i64::try_from(summary.calls)?,i64::try_from(summary.completed)?,i64::try_from(summary.result_bytes)?,i64::try_from(summary.reserved_output_tokens)?,summary.prefix_sha256])?;
    ensure!(changed == 1, "Vision accounting scope changed");
    Ok(())
}
pub(super) fn audit(db: &Connection, receipt: &Receipt) -> Result<Summary> {
    let summary = calculated(db, receipt)?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 6 {
        return Ok(summary);
    }
    let d = &receipt.definition;
    let found:Option<(Option<String>,i64,i64,i64,i64,Option<String>)>=db.query_row(
        "SELECT CASE WHEN length(CAST(grant_digest AS BLOB))=64 THEN grant_digest ELSE NULL END,call_count,result_count,result_bytes,reserved_output_tokens,CASE WHEN length(CAST(prefix_sha256 AS BLOB))=64 THEN prefix_sha256 ELSE NULL END FROM execution_workload_analysis_state WHERE grant_id=?1",
        [&d.grant_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    if parameters(d).is_err() {
        ensure!(found.is_none(), "Vision state on metadata-only workload");
    } else {
        let (digest, calls, completed, bytes, tokens, prefix) =
            found.context("Vision accounting state missing")?;
        ensure!(
            digest.as_deref() == Some(d.scope()?.grant_sha256.as_str())
                && u64::try_from(calls)? == summary.calls
                && u64::try_from(completed)? == summary.completed
                && usize::try_from(bytes)? == summary.result_bytes
                && u64::try_from(tokens)? == summary.reserved_output_tokens
                && prefix.as_deref() == Some(summary.prefix_sha256.as_str()),
            "Vision analysis rows, counters or checksum prefix changed"
        );
    }
    Ok(summary)
}

/// Single-use acknowledgment of one saved intent; not model or source authority.
pub(crate) struct AnalysisPermit {
    scope: stop::Scope,
    intent: Intent,
    checksum: String,
}
fn writable(
    db: &Connection,
    ledger: &GrantLedger,
    definition: &Definition,
    checkpoint: &crate::workload_leases::Checkpoint,
    stopped: &tokio::sync::watch::Receiver<bool>,
) -> Result<Receipt> {
    let contract = definition.lease_contract()?;
    checkpoint.check(&contract)?;
    let receipt = super::read_core(
        db,
        &ledger.node_id,
        Some(&definition.issuer_id),
        &definition.grant_id,
    )?;
    ensure!(
        receipt.definition == *definition
            && receipt.receipt.state == "accepted"
            && !*stopped.borrow()
            && !stop::recorded_scope(db, &contract.scope)?,
        "Vision workload changed or stopped"
    );
    Ok(receipt)
}
fn begin(
    ledger: &GrantLedger,
    d: &Definition,
    position: u64,
    image_sha256: String,
    image_bytes: usize,
    checkpoint: &crate::workload_leases::Checkpoint,
    stopped: &tokio::sync::watch::Receiver<bool>,
) -> Result<AnalysisPermit> {
    let p = parameters(d)?;
    let contract = d.lease_contract()?;
    checkpoint.check(&contract)?;
    let mut db = ledger
        .connection
        .lock()
        .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
    checkpoint.check(&contract)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let receipt = writable(&tx, ledger, d, checkpoint, stopped)?;
    let saved = audit(&tx, &receipt)?;
    ensure!(
        saved.calls == saved.completed
            && saved.calls < p.max_calls()
            && position > saved.last_source_sequence
            && position >= 2
            && position <= u64::from(p.capture.max_frames)
            && (4..=p.images.max_image_bytes).contains(&image_bytes)
            && crate::request_result::valid_digest(&image_sha256),
        "Vision analysis budget exhausted or position reused"
    );
    let intent = Intent {
        schema_version: 1,
        grant_sha256: contract.scope.grant_sha256.clone(),
        sequence: saved.calls + 1,
        source_sequence: position,
        image_sha256,
        image_bytes,
        lease_sequence: checkpoint.sequence(),
        recorded_at_ms: crate::protocol::now_ms().max(receipt.receipt.updated_at_ms),
    };
    let raw = serde_json::to_string(&intent)?;
    ensure!(raw.len() <= HEADROOM, "Vision intent exceeds reserved size");
    let checksum = hash(raw.as_bytes());
    tx.execute(
        "INSERT INTO execution_workload_analyses VALUES(?1,?2,?3,?4,?5,NULL,NULL)",
        params![
            d.grant_id,
            i64::try_from(intent.sequence)?,
            i64::try_from(position)?,
            raw,
            checksum
        ],
    )?;
    tx.execute(
        "UPDATE execution_receipts SET updated_ms=max(updated_ms,accepted_ms,?2) WHERE grant_id=?1",
        params![d.grant_id, i64::try_from(intent.recorded_at_ms)?],
    )?;
    let current = writable(&tx, ledger, d, checkpoint, stopped)?;
    let summary = calculated(&tx, &current)?;
    store_summary(&tx, d, &summary)?;
    audit(&tx, &current)?;
    tx.commit()?;
    checkpoint.check(&contract)?;
    ensure!(
        !*stopped.borrow() && !stop::recorded_scope(&db, &contract.scope)?,
        "Vision stopped during intent commit"
    );
    Ok(AnalysisPermit {
        scope: contract.scope,
        intent,
        checksum,
    })
}
fn finish(
    ledger: &GrantLedger,
    d: &Definition,
    permit: AnalysisPermit,
    text: String,
    checkpoint: &crate::workload_leases::Checkpoint,
    stopped: &tokio::sync::watch::Receiver<bool>,
) -> Result<Summary> {
    let p = parameters(d)?;
    let contract = d.lease_contract()?;
    ensure!(
        permit.scope == contract.scope
            && !text.trim().is_empty()
            && serde_json::to_vec(&text)?.len() <= p.inference.max_result_bytes
            && !text.contains('\0'),
        "Vision result differs from reviewed scope"
    );
    checkpoint.check(&contract)?;
    let mut db = ledger
        .connection
        .lock()
        .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
    checkpoint.check(&contract)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let receipt = writable(&tx, ledger, d, checkpoint, stopped)?;
    audit(&tx, &receipt)?;
    let old = rows(&tx, &receipt)?
        .pop()
        .context("Vision intent is unavailable")?;
    ensure!(
        old.intent == permit.intent && old.checksum == permit.checksum && old.result.is_none(),
        "Vision intent changed or was already completed"
    );
    let result = AnalysisResult {
        schema_version: 1,
        sequence: old.intent.sequence,
        intent_sha256: old.checksum,
        text,
        lease_sequence: checkpoint.sequence(),
        recorded_at_ms: crate::protocol::now_ms().max(receipt.receipt.updated_at_ms),
    };
    let raw = serde_json::to_string(&result)?;
    ensure!(
        raw.len() <= p.inference.max_result_bytes + HEADROOM,
        "Vision result exceeds reserved size"
    );
    let changed=tx.execute("UPDATE execution_workload_analyses SET result=?3,result_sha256=?4 WHERE grant_id=?1 AND sequence=?2 AND result IS NULL AND result_sha256 IS NULL",
        params![d.grant_id,i64::try_from(result.sequence)?,raw,hash(raw.as_bytes())])?;
    ensure!(changed == 1, "Vision completion lost its pending intent");
    tx.execute(
        "UPDATE execution_receipts SET updated_ms=max(updated_ms,accepted_ms,?2) WHERE grant_id=?1",
        params![d.grant_id, i64::try_from(result.recorded_at_ms)?],
    )?;
    let current = writable(&tx, ledger, d, checkpoint, stopped)?;
    let summary = calculated(&tx, &current)?;
    store_summary(&tx, d, &summary)?;
    audit(&tx, &current)?;
    tx.commit()?;
    checkpoint.check(&contract)?;
    ensure!(
        !*stopped.borrow() && !stop::recorded_scope(&db, &contract.scope)?,
        "Vision stopped during result commit"
    );
    Ok(summary)
}
impl EventWriter {
    async fn analysis_write<T, F>(
        &self,
        checkpoint: Arc<crate::workload_leases::Checkpoint>,
        write: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(
                &GrantLedger,
                &Definition,
                &crate::workload_leases::Checkpoint,
                &tokio::sync::watch::Receiver<bool>,
            ) -> Result<T>
            + Send
            + 'static,
    {
        let contract = self.definition.lease_contract()?;
        checkpoint.check(&contract)?;
        let slot = self
            .ledger
            .event_workers
            .clone()
            .try_acquire_owned()
            .context("Vision journal worker busy; no retry")?;
        let ledger = self.ledger.clone();
        let d = self.definition.clone();
        let stopped = self.stop.clone();
        let authority = checkpoint.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.ledger.submit_workload_job(Box::new(move || {
            let result = write(&ledger, &d, &authority, &stopped);
            drop(slot);
            let _ = sender.send(result);
        }))?;
        checkpoint
            .supervise(&contract, async {
                receiver
                    .await
                    .context("Vision journal worker unavailable")?
            })
            .await
    }
    pub(crate) async fn begin_analysis(
        &self,
        position: u64,
        image: &crate::python::vision::PrivateJpeg,
        checkpoint: Arc<crate::workload_leases::Checkpoint>,
    ) -> Result<AnalysisPermit> {
        let digest = image.digest().to_owned();
        let bytes = image.bytes();
        self.analysis_write(checkpoint, move |ledger, d, c, stop| {
            begin(ledger, d, position, digest, bytes, c, stop)
        })
        .await
    }
    pub(crate) async fn finish_analysis(
        &self,
        permit: AnalysisPermit,
        text: String,
        checkpoint: Arc<crate::workload_leases::Checkpoint>,
    ) -> Result<Summary> {
        self.analysis_write(checkpoint, move |ledger, d, c, stop| {
            finish(ledger, d, permit, text, c, stop)
        })
        .await
    }
}

impl GrantLedger {
    /// Historical analysis only, bound to the authenticated issuer. This cannot
    /// recover an AnalysisPermit or re-run an incomplete model call.
    pub fn workload_analyses(
        &self,
        issuer: &str,
        id: &str,
        after: u64,
        limit: usize,
    ) -> Result<Value> {
        ensure!(
            (1..=100).contains(&limit),
            "Invalid vision inspection page bound"
        );
        let db = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
        let tx = db.unchecked_transaction()?;
        let receipt = super::read(&tx, &self.node_id, Some(issuer), id)?;
        parameters(&receipt.definition)?;
        let summary = audit(&tx, &receipt)?;
        ensure!(
            after <= summary.calls,
            "Vision inspection cursor outside retained prefix"
        );
        let end = summary.calls.min(after.saturating_add(limit as u64));
        let records:Vec<_>=rows(&tx,&receipt)?.into_iter().filter(|r|r.intent.sequence>after && r.intent.sequence<=end)
            .map(|r|json!({"intent":r.intent,"intent_sha256":r.checksum,"result":r.result,"result_sha256":r.result_checksum})).collect();
        let result = json!({"schema_version":1,"grant_id":id,"grant_sha256":receipt.definition.scope()?.grant_sha256,
            "state":receipt.receipt.state,"summary":summary,"analyses":records,"next_sequence":end,"more":end<summary.calls,
            "execution_authorized":false,"restart_authorized":false,"model_replay_authorized":false});
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "workload_vision_journal_tests.rs"]
mod tests;
