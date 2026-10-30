//! Bounded event evidence in the existing grant journal, never source authority.
use super::*;
use crate::workload_events::Event;
use anyhow::ensure;

const MIN_EVENT_BYTES: usize = 128;
const ROW_HEADROOM: usize = 512;
const STATE_SQL: &str = "CREATE TABLE execution_workload_event_state(grant_id TEXT NOT NULL PRIMARY KEY REFERENCES execution_receipts(grant_id),grant_digest TEXT NOT NULL,event_count INTEGER NOT NULL CHECK(event_count>=0),event_bytes INTEGER NOT NULL CHECK(event_bytes>=0),reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes>=0),prefix_sha256 TEXT NOT NULL)";
const EVENT_SQL: &str = "CREATE TABLE execution_workload_events(grant_id TEXT NOT NULL REFERENCES execution_workload_event_state(grant_id),sequence INTEGER NOT NULL CHECK(sequence>0),encoded_bytes INTEGER NOT NULL CHECK(encoded_bytes>0),data TEXT NOT NULL,data_sha256 TEXT NOT NULL,prefix_sha256 TEXT NOT NULL,PRIMARY KEY(grant_id,sequence))";

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn seed(definition: &Definition) -> Result<String> {
    Ok(hash(definition.scope()?.grant_sha256.as_bytes()))
}
fn next_prefix(previous: &str, row_hash: &str) -> String {
    hash(format!("{previous}{row_hash}").as_bytes())
}

pub(super) fn schema(db: &Connection, version: i64) -> Result<()> {
    let found = db.prepare("SELECT name,type,CASE WHEN length(CAST(sql AS BLOB))<=2048 THEN sql ELSE NULL END FROM sqlite_master WHERE lower(name) IN ('execution_workload_event_state','execution_workload_events') ORDER BY name")?
        .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let expected: Vec<(String, String, Option<String>)> = vec![
        (
            "execution_workload_event_state".into(),
            "table".into(),
            Some(STATE_SQL.into()),
        ),
        (
            "execution_workload_events".into(),
            "table".into(),
            Some(EVENT_SQL.into()),
        ),
    ];
    ensure!(
        if matches!(version, 4 | 5) {
            found == expected
        } else {
            found.is_empty()
        },
        "Workload event schema changed or downgraded; retained without repair"
    );
    Ok(())
}
pub(super) fn reservation(db: &Connection, definition: &Definition) -> Result<usize> {
    let page = usize::try_from(db.query_row::<i64, _, _>("PRAGMA page_size", [], |r| r.get(0))?)?;
    ensure!(
        page > 0 && page.is_power_of_two(),
        "Invalid journal page size"
    );
    let rows = usize::try_from(definition.events.max_events)?
        .min(definition.events.max_total_bytes / MIN_EVENT_BYTES);
    // Payload bytes already have the ordinary result reservation. Include
    // conservative row/index/split/counter overhead without allocating it.
    let row = definition
        .events
        .max_event_bytes
        .checked_add(ROW_HEADROOM)
        .context("Event reservation overflow")?
        .div_ceil(page)
        .checked_add(8)
        .and_then(|n| n.checked_mul(page))
        .context("Event reservation overflow")?;
    rows.checked_mul(row)
        .and_then(|n| n.checked_add(8 * page))
        .context("Event reservation overflow")
}
pub(super) fn reserved(db: &Connection) -> Result<usize> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    schema(db, version)?;
    if version < 4 {
        return Ok(0);
    }
    usize::try_from(db.query_row::<i64,_,_>("SELECT coalesce(sum(s.reserved_bytes),0) FROM execution_workload_event_state s JOIN execution_receipts r ON r.grant_id=s.grant_id WHERE r.state='accepted'", [], |r| r.get(0))?).context("Invalid workload reservation")
}
pub(super) fn initialize(db: &Connection, definition: &Definition) -> Result<()> {
    let scope = definition.scope()?;
    db.execute(
        "INSERT INTO execution_workload_event_state VALUES(?1,?2,0,0,?3,?4)",
        params![
            scope.grant_id,
            scope.grant_sha256,
            i64::try_from(reservation(db, definition)?)?,
            seed(definition)?
        ],
    )?;
    Ok(())
}
pub(super) fn upgrade(db: &Connection, node: &str) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    schema(db, version)?;
    if matches!(version, 4 | 5) {
        return Ok(());
    }
    ensure!(
        version == 3,
        "Event journal requires validated workload consumption schema"
    );
    let ids = db
        .prepare("SELECT grant_id FROM execution_receipts WHERE receipt_kind='workload'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in &ids {
        super::read(db, node, None, id)?;
    }
    db.execute_batch(STATE_SQL)?;
    db.execute_batch(EVENT_SQL)?;
    // The containing IMMEDIATE transaction keeps the validated rows stable.
    // Re-read one definition at a time rather than retaining every parsed plan.
    db.execute_batch("PRAGMA user_version=4")?;
    for id in ids {
        let definition = super::read_core(db, node, None, &id)?.definition;
        initialize(db, &definition)?;
    }
    schema(db, 4)
}

struct State {
    count: u64,
    bytes: usize,
    prefix: String,
}
fn state(db: &Connection, definition: &Definition) -> Result<State> {
    let (digest,count,bytes,reserved,prefix): (Option<String>,i64,i64,i64,Option<String>) = db.query_row(
        "SELECT CASE WHEN length(CAST(grant_digest AS BLOB))=64 THEN grant_digest ELSE NULL END,event_count,event_bytes,reserved_bytes,CASE WHEN length(CAST(prefix_sha256 AS BLOB))=64 THEN prefix_sha256 ELSE NULL END FROM execution_workload_event_state WHERE grant_id=?1", [&definition.grant_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let digest = digest.context("Invalid event scope digest size")?;
    let prefix = prefix.context("Invalid event prefix digest size")?;
    let count = u64::try_from(count)?;
    let bytes = usize::try_from(bytes)?;
    ensure!(
        digest == definition.scope()?.grant_sha256
            && count <= definition.events.max_events
            && bytes <= definition.events.max_total_bytes
            && bytes >= usize::try_from(count)?.saturating_mul(MIN_EVENT_BYTES)
            && usize::try_from(reserved)? == reservation(db, definition)?
            && crate::request_result::valid_digest(&prefix)
            && (count != 0 || (bytes == 0 && prefix == seed(definition)?)),
        "Damaged workload event counters/reservation"
    );
    Ok(State {
        count,
        bytes,
        prefix,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    schema_version: u32,
    lease_sequence: u64,
    recorded_at_ms: u64,
    event: Event,
}
struct Row {
    record: Record,
    bytes: usize,
    checksum: String,
    prefix: String,
}
fn row(db: &Connection, receipt: &Receipt, sequence: u64) -> Result<Row> {
    let bound = receipt.definition.events.max_event_bytes + ROW_HEADROOM;
    let (size,bytes,raw,checksum,prefix): (i64,i64,Option<String>,Option<String>,Option<String>) = db.query_row(
        "SELECT length(CAST(data AS BLOB)),encoded_bytes,CASE WHEN length(CAST(data AS BLOB))<=?3 THEN data ELSE NULL END,CASE WHEN length(CAST(data_sha256 AS BLOB))=64 THEN data_sha256 ELSE NULL END,CASE WHEN length(CAST(prefix_sha256 AS BLOB))=64 THEN prefix_sha256 ELSE NULL END FROM execution_workload_events WHERE grant_id=?1 AND sequence=?2",
        params![receipt.definition.grant_id,i64::try_from(sequence)?,i64::try_from(bound)?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    ensure!(
        (1..=i64::try_from(bound)?).contains(&size),
        "Oversized workload event row"
    );
    let raw = raw.context("Workload event row unavailable")?;
    let checksum = checksum.context("Invalid event checksum size")?;
    let prefix = prefix.context("Invalid event prefix size")?;
    let record: Record = serde_json::from_str(&raw)?;
    let bytes = usize::try_from(bytes)?;
    record.event.validate(&receipt.definition)?;
    ensure!(
        record.schema_version == 1
            && record.lease_sequence > 0
            && record.event.sequence == sequence
            && record.recorded_at_ms >= receipt.receipt.accepted_at_ms
            && record.recorded_at_ms <= receipt.receipt.updated_at_ms
            && serde_json::to_string(&record)? == raw
            && hash(raw.as_bytes()) == checksum
            && crate::request_result::valid_digest(&prefix)
            && (MIN_EVENT_BYTES..=receipt.definition.events.max_event_bytes).contains(&bytes)
            && serde_json::to_vec(&record.event)?.len() == bytes,
        "Damaged workload event row"
    );
    Ok(Row {
        record,
        bytes,
        checksum,
        prefix,
    })
}
pub(super) fn audit(db: &Connection, receipt: &Receipt) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 4 {
        return Ok(());
    }
    let saved = state(db, &receipt.definition)?;
    let (count,total): (i64,i64) = db.query_row("SELECT count(*),coalesce(sum(encoded_bytes),0) FROM execution_workload_events WHERE grant_id=?1", [&receipt.definition.grant_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
    ensure!(
        u64::try_from(count)? == saved.count && usize::try_from(total)? == saved.bytes,
        "Workload event prefix cardinality changed"
    );
    let mut previous: Option<Record> = None;
    let mut prefix = seed(&receipt.definition)?;
    let mut bytes = 0usize;
    for sequence in 1..=saved.count {
        let row = row(db, receipt, sequence)?;
        row.record
            .event
            .follows(&receipt.definition, previous.as_ref().map(|r| &r.event))?;
        ensure!(
            previous
                .as_ref()
                .is_none_or(|r| r.lease_sequence <= row.record.lease_sequence
                    && r.recorded_at_ms <= row.record.recorded_at_ms),
            "Workload event timing/lease prefix changed"
        );
        prefix = next_prefix(&prefix, &row.checksum);
        ensure!(
            prefix == row.prefix,
            "Workload event checksum prefix changed"
        );
        bytes = bytes
            .checked_add(row.bytes)
            .context("Event size overflow")?;
        previous = Some(row.record);
    }
    ensure!(
        prefix == saved.prefix && bytes == saved.bytes,
        "Workload event summary changed"
    );
    super::completion::audit_prefix(db, receipt)
}
pub(super) fn completion_prefix(
    db: &Connection,
    receipt: &Receipt,
) -> Result<(u64, usize, String, Event, u64)> {
    let saved = state(db, &receipt.definition)?;
    ensure!(
        saved.count > 0,
        "Finished workload has no retained initial event"
    );
    let last = row(db, receipt, saved.count)?;
    ensure!(
        last.prefix == saved.prefix,
        "Finished workload event tail changed"
    );
    Ok((
        saved.count,
        saved.bytes,
        saved.prefix,
        last.record.event,
        last.record.lease_sequence,
    ))
}
pub(super) fn audit_orphans(db: &Connection) -> Result<()> {
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 4 {
        return Ok(());
    }
    let bad:i64=db.query_row("SELECT (SELECT count(*) FROM execution_workload_event_state s LEFT JOIN execution_receipts r ON r.grant_id=s.grant_id WHERE r.grant_id IS NULL OR r.receipt_kind!='workload' OR r.grant_digest!=s.grant_digest)+(SELECT count(*) FROM execution_workload_events e LEFT JOIN execution_workload_event_state s ON s.grant_id=e.grant_id WHERE s.grant_id IS NULL)",[],|r|r.get(0))?;
    ensure!(bad == 0, "Orphaned or cross-kind workload event evidence");
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AppendReceipt {
    pub sequence: u64,
    pub data_sha256: String,
    pub reused: bool,
}
pub(super) fn append(
    ledger: &GrantLedger,
    definition: &Definition,
    event: Event,
    checkpoint: &crate::workload_leases::Checkpoint,
    stop: &tokio::sync::watch::Receiver<bool>,
) -> Result<AppendReceipt> {
    let contract = definition.lease_contract()?;
    checkpoint.check(&contract)?;
    ensure!(!*stop.borrow(), "Workload stop requested before event save");
    let mut db = ledger
        .connection
        .lock()
        .map_err(|_| anyhow::anyhow!("Grant journal unavailable"))?;
    checkpoint.check(&contract)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    checkpoint.check(&contract)?;
    let receipt = super::read_core(
        &tx,
        &ledger.node_id,
        Some(&definition.issuer_id),
        &definition.grant_id,
    )?;
    ensure!(
        receipt.definition == *definition
            && receipt.receipt.state == "accepted"
            && !*stop.borrow()
            && !stop::recorded_scope(&tx, &contract.scope)?,
        "Workload changed or stopped before event save"
    );
    let saved = state(&tx, definition)?;
    if event.sequence <= saved.count {
        // Duplicate inspection is uncommon and must validate the entire retained
        // prefix before returning its original evidence. New appends stay O(1).
        audit(&tx, &receipt)?;
        let old = row(&tx, &receipt, event.sequence)?;
        ensure!(
            old.record.event == event,
            "Conflicting workload event sequence"
        );
        checkpoint.check(&contract)?;
        tx.commit()?;
        checkpoint.check(&contract)?;
        ensure!(!*stop.borrow(), "Workload stopped during event read");
        return Ok(AppendReceipt {
            sequence: event.sequence,
            data_sha256: old.checksum,
            reused: true,
        });
    }
    let previous = if saved.count == 0 {
        None
    } else {
        Some(row(&tx, &receipt, saved.count)?)
    };
    if let Some(previous) = &previous {
        ensure!(
            previous.prefix == saved.prefix
                && previous.record.lease_sequence <= checkpoint.sequence(),
            "Workload event tail changed"
        );
    }
    event.follows(definition, previous.as_ref().map(|r| &r.record.event))?;
    let bytes = serde_json::to_vec(&event)?.len();
    ensure!(
        (MIN_EVENT_BYTES..=definition.events.max_event_bytes).contains(&bytes)
            && bytes
                <= definition
                    .events
                    .max_total_bytes
                    .saturating_sub(saved.bytes),
        "Workload event reporting budget exceeded"
    );
    let record = Record {
        schema_version: 1,
        lease_sequence: checkpoint.sequence(),
        recorded_at_ms: crate::protocol::now_ms().max(receipt.receipt.updated_at_ms),
        event,
    };
    let raw = serde_json::to_string(&record)?;
    ensure!(
        raw.len() <= definition.events.max_event_bytes + ROW_HEADROOM,
        "Workload event envelope oversized"
    );
    let checksum = hash(raw.as_bytes());
    let prefix = next_prefix(&saved.prefix, &checksum);
    checkpoint.check(&contract)?;
    ensure!(
        !*stop.borrow(),
        "Workload stop requested before event commit"
    );
    tx.execute(
        "INSERT INTO execution_workload_events VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            definition.grant_id,
            i64::try_from(record.event.sequence)?,
            i64::try_from(bytes)?,
            raw,
            checksum,
            prefix
        ],
    )?;
    tx.execute("UPDATE execution_workload_event_state SET event_count=?2,event_bytes=?3,prefix_sha256=?4 WHERE grant_id=?1",params![definition.grant_id,i64::try_from(record.event.sequence)?,i64::try_from(saved.bytes+bytes)?,prefix])?;
    tx.execute("UPDATE execution_receipts SET updated_ms=max(updated_ms,accepted_ms,?2) WHERE grant_id=?1 AND state='accepted'",params![definition.grant_id,i64::try_from(record.recorded_at_ms)?])?;
    checkpoint.check(&contract)?;
    tx.commit()?;
    checkpoint.check(&contract)?;
    ensure!(
        !*stop.borrow() && !stop::recorded_scope(&db, &contract.scope)?,
        "Workload stopped during event commit"
    );
    Ok(AppendReceipt {
        sequence: record.event.sequence,
        data_sha256: checksum,
        reused: false,
    })
}
pub(super) fn inspect(
    db: &Connection,
    receipt: &Receipt,
    after: u64,
    limit: usize,
) -> Result<Value> {
    ensure!(
        (1..=128).contains(&limit),
        "Invalid event inspection page bound"
    );
    audit(db, receipt)?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let saved = if version >= 4 {
        Some(state(db, &receipt.definition)?)
    } else {
        None
    };
    let count = saved.as_ref().map_or(0, |s| s.count);
    ensure!(
        after <= count,
        "Event inspection cursor outside retained prefix"
    );
    let end = count.min(after.saturating_add(u64::try_from(limit)?));
    let mut records = Vec::new();
    for sequence in after + 1..=end {
        records.push(row(db, receipt, sequence)?.record);
    }
    Ok(
        json!({"schema_version":1,"grant_id":receipt.definition.grant_id,"grant_sha256":receipt.definition.scope()?.grant_sha256,
        "state":receipt.receipt.state,"durable_events":records,"event_count":count,"event_bytes":saved.as_ref().map_or(0,|s|s.bytes),
        "next_sequence":end,"more":end<count,"execution_authorized":false,"restart_authorized":false,"source_termination_confirmed":false,
        "owned_process_exit_observed":receipt.completion.as_ref().is_some_and(|report|report.owned_process_exit_observed)}),
    )
}
