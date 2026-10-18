//! Bounded installation observations, not workload authority or transfer resume state.
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

const MAX_RECORD_BYTES: usize = 2048;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Policy {
    pub max_records: usize,
    pub max_bytes: usize,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_records: 256,
            max_bytes: 1024 * 1024,
        }
    }
}
impl Policy {
    fn from_config(config: &Value) -> Result<Self> {
        let policy: Self = config
            .pointer("/models/download_receipts")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        if !(1..=4096).contains(&policy.max_records)
            || !(64 * 1024..=64 * 1024 * 1024).contains(&policy.max_bytes)
        {
            bail!("Invalid model download receipt limits")
        }
        Ok(policy)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Downloading,
    Installed,
    Cancelled,
    Error,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub schema_version: u32,
    pub sequence: u64,
    pub id: String,
    pub model_id: String,
    pub backend: String,
    pub definition_sha256: String,
    pub state: State,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub total_bytes: Option<u64>,
    pub cancel_requested: bool,
}
impl Receipt {
    fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.id)?;
        crate::model_selection::validate_id(&self.model_id)?;
        if self.schema_version != 1
            || !(1..=4096).contains(&self.sequence)
            || uuid::Uuid::parse_str(&self.id)?.is_nil()
            || !matches!(self.backend.as_str(), "ollama" | "llamacpp")
            || !crate::request_result::valid_digest(&self.definition_sha256)
            || self.accepted_at_ms == 0
            || self.updated_at_ms < self.accepted_at_ms
            || self.updated_at_ms > i64::MAX as u64
            || self
                .total_bytes
                .is_some_and(|n| n == 0 || n > i64::MAX as u64)
            || (self.backend == "ollama" && self.total_bytes.is_some())
            || (self.backend == "llamacpp" && self.total_bytes.is_none())
            || (self.state == State::Cancelled && !self.cancel_requested)
        {
            bail!("Damaged model download receipt; retained without repair")
        }
        Ok(())
    }
    pub(crate) fn progress(&self) -> Value {
        let state = serde_json::to_value(&self.state).expect("receipt state");
        json!({"id":self.id,"model_id":self.model_id,"backend":self.backend,"state":state,
            "phase":if self.state==State::Interrupted {"App transfer interrupted"} else {"Retained installation receipt"},
            "completed":null,"total":self.total_bytes,"cancel_requested":self.cancel_requested,
            "progress_available":false,"durable_receipt":self,"termination_confirmed":false,
            "automatically_retried":false,
            "message":"Retained operation outcome, not restored byte progress or a current model inventory. Interrupted work is not resumed; shared provider effects may continue. Inspect installed models before a new explicit install."})
    }
}

pub(crate) enum Journal {
    Empty,
    Ready(Store),
    Unavailable,
}
pub(crate) struct Store {
    db: Connection,
    _lease: File,
    policy: Policy,
}
fn path(config: &Path) -> PathBuf {
    let mut path = config.as_os_str().to_os_string();
    path.push(".model-downloads.sqlite3");
    PathBuf::from(path)
}
impl Journal {
    pub(crate) fn recover(config: &Path, node: &str) -> (Self, Option<Value>) {
        match path(config).try_exists() {
            Ok(false) => return (Self::Empty, None),
            Err(_) => return (Self::Unavailable, Some(unavailable())),
            Ok(true) => {}
        }
        let result = (|| {
            let config_value: Value = serde_json::from_slice(&std::fs::read(config)?)?;
            Store::open(config, node, Policy::from_config(&config_value)?)
        })();
        match result {
            Ok(store) => match store.latest() {
                Ok(latest) => (Self::Ready(store), latest.map(|r| r.progress())),
                Err(_) => (Self::Unavailable, Some(unavailable())),
            },
            Err(_) => (Self::Unavailable, Some(unavailable())),
        }
    }
    pub(crate) fn begin(
        &mut self,
        config_path: &Path,
        node: &str,
        config: &Value,
        progress: &Value,
    ) -> Result<Receipt> {
        let policy = Policy::from_config(config)?;
        if matches!(self, Self::Empty) {
            *self = Self::Ready(Store::open(
                config_path,
                node,
                Policy::from_config(config)?,
            )?);
        }
        let Self::Ready(store) = self else {
            bail!("Model download receipt journal unavailable; no transfer admitted")
        };
        store.policy = policy;
        store.claim(
            progress,
            progress["definition_sha256"]
                .as_str()
                .context("Missing resolved model definition digest")?
                .to_owned(),
        )
    }
    /// Only a fresh explicit install intent may retry opening a disabled journal.
    /// This validates retained evidence; it cannot resume a transfer or repair data.
    pub(crate) fn reopen_for_install(
        &mut self,
        config: &Path,
        node: &str,
    ) -> Result<Option<Value>> {
        if !matches!(self, Self::Unavailable) {
            return Ok(None);
        }
        let (recovered, latest) = Self::recover(config, node);
        if !matches!(recovered, Self::Ready(_)) {
            bail!("Existing model download receipts remain unavailable; retained without repair")
        }
        *self = recovered;
        Ok(latest)
    }
    pub(crate) fn get(&self, id: &str) -> Result<Option<Receipt>> {
        match self {
            Self::Empty => Ok(None),
            Self::Ready(store) => store.get(id),
            Self::Unavailable => {
                bail!("Model download receipt journal unavailable; no recovery or replay")
            }
        }
    }
    pub(crate) fn availability(&self, config: &Value) -> Result<Value> {
        let policy = Policy::from_config(config)?;
        let (count, blocker) = match self {
            Self::Empty => (Some(0), None),
            Self::Unavailable => (None, Some("receipt_journal_unavailable")),
            Self::Ready(store) => {
                let records = store.records()?;
                let pages: i64 = store.db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
                let size: i64 = store.db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
                let blocker = if records.iter().any(|r| r.state == State::Downloading) {
                    Some("receipt_transfer_unresolved")
                } else if records.len() >= policy.max_records
                    || pages.saturating_add(4).saturating_mul(size)
                        > i64::try_from(policy.max_bytes)?
                {
                    Some("receipt_quota_full")
                } else {
                    None
                };
                (Some(records.len()), blocker)
            }
        };
        Ok(
            json!({"scope":"node_install_receipts","limits":policy,"retained_records":count,"blocker":blocker,"available":blocker.is_none(),"inspection_only":true}),
        )
    }
    pub(crate) fn cancel(&mut self, id: &str) -> Result<()> {
        if let Self::Ready(store) = self {
            store.change(id, None)
        } else {
            bail!("Model download receipt journal unavailable")
        }
    }
    pub(crate) fn finish(&mut self, id: &str, state: State) -> Result<()> {
        if let Self::Ready(store) = self {
            store.change(id, Some(state))
        } else {
            bail!("Model download receipt journal unavailable")
        }
    }
}
fn unavailable() -> Value {
    json!({"state":"unavailable","phase":"Installation receipt unavailable","message":"The existing receipt journal could not be opened or validated. Model installation is blocked; node controls remain usable. No repair, retry or download was performed."})
}
impl Store {
    fn open(config: &Path, node: &str, policy: Policy) -> Result<Self> {
        crate::network::validate_remote_node_id(node)?;
        let db_path = path(config);
        if std::fs::metadata(&db_path).is_ok_and(|m| m.len() > 64 * 1024 * 1024) {
            bail!("Model download receipt journal exceeds supported storage bounds")
        }
        let mut lock_path = db_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(PathBuf::from(lock_path))?;
        lease
            .try_lock_exclusive()
            .context("Model download receipt owner already running")?;
        let mut db = Connection::open(&db_path)?;
        db.busy_timeout(std::time::Duration::from_millis(50))?;
        let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            bail!("Damaged model download receipt journal")
        }
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            let tables: i64 = db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if tables != 0 {
                bail!("Unversioned model download receipt journal retained")
            }
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute_batch("CREATE TABLE receipt_owner(node_id TEXT NOT NULL PRIMARY KEY); CREATE TABLE model_download_receipts(id TEXT NOT NULL PRIMARY KEY,ordinal INTEGER NOT NULL UNIQUE,payload TEXT NOT NULL,payload_sha256 TEXT NOT NULL); PRAGMA user_version=1;")?;
            tx.execute("INSERT INTO receipt_owner VALUES(?1)", [node])?;
            tx.commit()?;
        } else if version != 1 {
            bail!("Unsupported model download receipt journal version")
        }
        let owners = db
            .prepare("SELECT node_id FROM receipt_owner")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if owners != [node] {
            bail!("Model download receipt journal owner changed")
        }
        let mut store = Self {
            db,
            _lease: lease,
            policy,
        };
        let records = store.records()?;
        store
            .db
            .execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
        let size: i64 = store.db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        store.db.pragma_update(
            None,
            "max_page_count",
            i64::try_from(store.policy.max_bytes)? / size,
        )?;
        let tx = store
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for mut record in records {
            if record.state == State::Downloading {
                record.state = State::Interrupted;
                record.updated_at_ms = crate::protocol::now_ms().max(record.updated_at_ms);
                save(&tx, &record, false)?;
            }
        }
        tx.commit()?;
        Ok(store)
    }
    fn records(&self) -> Result<Vec<Receipt>> {
        let mut query=self.db.prepare("SELECT id,CASE WHEN length(CAST(payload AS BLOB))<=2048 THEN payload ELSE NULL END,payload_sha256,ordinal FROM model_download_receipts ORDER BY ordinal")?;
        let mut result = Vec::new();
        for row in query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })? {
            let (id, payload, hash, ordinal) = row?;
            let ordinal = u64::try_from(ordinal)?;
            let payload = payload.context("Oversized model download receipt retained")?;
            let record: Receipt = serde_json::from_str(&payload)?;
            record.validate()?;
            if record.id != id
                || record.sequence != ordinal
                || ordinal != result.len() as u64 + 1
                || crate::grants::definition_digest(&json!(record))? != hash
            {
                bail!("Model download receipt integrity changed")
            }
            result.push(record);
            if result.len() > 4096 {
                bail!("Model download receipt history exceeds supported bounds")
            }
        }
        Ok(result)
    }
    fn latest(&self) -> Result<Option<Receipt>> {
        Ok(self.records()?.pop())
    }
    fn get(&self, id: &str) -> Result<Option<Receipt>> {
        Ok(self.records()?.into_iter().find(|r| r.id == id))
    }
    fn claim(&mut self, progress: &Value, digest: String) -> Result<Receipt> {
        let id = progress["id"]
            .as_str()
            .context("Missing download operation ID")?;
        let records = self.records()?;
        if records.iter().any(|r| r.id == id) {
            bail!("This download ID was already admitted. Inspect its retained receipt; no replay.")
        }
        if records.iter().any(|r| r.state == State::Downloading) {
            bail!("An admitted transfer remains unresolved; no replacement")
        }
        let pages: i64 = self.db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: i64 = self.db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        if records.len() >= self.policy.max_records
            || pages.saturating_add(4).saturating_mul(size) > i64::try_from(self.policy.max_bytes)?
        {
            bail!("Model download receipt quota full; no transfer admitted")
        }
        self.db.pragma_update(
            None,
            "max_page_count",
            i64::try_from(self.policy.max_bytes)? / size,
        )?;
        let now = crate::protocol::now_ms();
        let receipt = Receipt {
            schema_version: 1,
            sequence: records.len() as u64 + 1,
            id: id.into(),
            model_id: progress["model_id"]
                .as_str()
                .context("Missing download model ID")?
                .into(),
            backend: progress["backend"]
                .as_str()
                .context("Missing download backend")?
                .into(),
            definition_sha256: digest,
            state: State::Downloading,
            accepted_at_ms: now,
            updated_at_ms: now,
            total_bytes: progress["total"].as_u64(),
            cancel_requested: false,
        };
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        save(&tx, &receipt, true)?;
        tx.commit()?;
        Ok(receipt)
    }
    fn change(&mut self, id: &str, state: Option<State>) -> Result<()> {
        let mut receipt = self.get(id)?.context("Download receipt missing")?;
        if receipt.state != State::Downloading {
            bail!("Download receipt is no longer active")
        }
        if let Some(state) = state {
            if !matches!(state, State::Installed | State::Cancelled | State::Error) {
                bail!("Invalid download terminal state")
            }
            receipt.state = state;
        } else {
            receipt.cancel_requested = true;
        }
        receipt.updated_at_ms = crate::protocol::now_ms().max(receipt.updated_at_ms);
        save(&self.db, &receipt, false)
    }
}
fn save(db: &Connection, receipt: &Receipt, insert: bool) -> Result<()> {
    receipt.validate()?;
    let payload = serde_json::to_string(receipt)?;
    if payload.len() > MAX_RECORD_BYTES {
        bail!("Model download receipt exceeds reserved size")
    }
    let checksum = crate::grants::definition_digest(&json!(receipt))?;
    let changed = if insert {
        db.execute(
            "INSERT INTO model_download_receipts VALUES(?1,?4,?2,?3)",
            params![
                receipt.id,
                payload,
                checksum,
                i64::try_from(receipt.sequence)?
            ],
        )?
    } else {
        db.execute(
            "UPDATE model_download_receipts SET payload=?2,payload_sha256=?3 WHERE id=?1 AND ordinal=?4",
            params![receipt.id, payload, checksum,i64::try_from(receipt.sequence)?],
        )?
    };
    if changed != 1 {
        bail!("Model download receipt changed")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn progress() -> Value {
        json!({"id":uuid::Uuid::new_v4().to_string(),"model_id":"fixture","backend":"ollama","total":null,"definition_sha256":"a".repeat(64)})
    }
    fn config(path: &Path) -> Value {
        let value = json!({"node_id":"owner","models":{"provider":"ollama"}});
        crate::setup::write_json(path, &value).unwrap();
        value
    }
    #[test]
    fn interrupted_admission_survives_without_progress_resume_or_replay() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        let value = config(&cfg);
        let p = progress();
        let id = p["id"].as_str().unwrap();
        let mut journal = Journal::Empty;
        let accepted = journal.begin(&cfg, "owner", &value, &p).unwrap();
        assert_eq!(accepted.state, State::Downloading);
        let (other, _) = Journal::recover(&cfg, "owner");
        assert!(
            matches!(other, Journal::Unavailable),
            "a second service cannot recover a live owner's transfer"
        );
        assert_eq!(journal.get(id).unwrap().unwrap(), accepted);
        drop(journal);
        let (mut recovered, latest) = Journal::recover(&cfg, "owner");
        let receipt = recovered.get(id).unwrap().unwrap();
        assert_eq!(receipt.state, State::Interrupted);
        assert_eq!(receipt.accepted_at_ms, accepted.accepted_at_ms);
        let latest = latest.unwrap();
        assert!(latest["completed"].is_null());
        assert_eq!(latest["progress_available"], false);
        assert_eq!(latest["termination_confirmed"], false);
        assert_eq!(latest["automatically_retried"], false);
        assert!(recovered.begin(&cfg, "owner", &value, &p).is_err());
        recovered.begin(&cfg, "owner", &value, &progress()).unwrap();
        assert_eq!(recovered.get(id).unwrap().unwrap(), receipt);
    }
    #[test]
    fn terminal_history_and_latest_sequence_survive_wall_clock_ties() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        config(&cfg);
        let mut store = Store::open(&cfg, "owner", Policy::default()).unwrap();
        let mut ids = vec![];
        for state in [State::Installed, State::Cancelled, State::Error] {
            let p = progress();
            let r = store.claim(&p, "a".repeat(64)).unwrap();
            store.change(&r.id, None).unwrap();
            store.change(&r.id, Some(state)).unwrap();
            ids.push(r.id);
        }
        for mut r in store.records().unwrap() {
            r.accepted_at_ms = 1;
            r.updated_at_ms = 1;
            save(&store.db, &r, false).unwrap();
        }
        drop(store);
        let (recovered, latest) = Journal::recover(&cfg, "owner");
        assert_eq!(latest.unwrap()["id"], ids[2]);
        for (i, id) in ids.iter().enumerate() {
            let r = recovered.get(id).unwrap().unwrap();
            assert_ne!(r.state, State::Interrupted);
            assert_eq!(r.sequence, i as u64 + 1);
        }
    }
    #[test]
    fn cancellation_survives_but_never_becomes_termination_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        let value = config(&cfg);
        let p = progress();
        let id = p["id"].as_str().unwrap();
        let mut journal = Journal::Empty;
        journal.begin(&cfg, "owner", &value, &p).unwrap();
        journal.cancel(id).unwrap();
        drop(journal);
        let (mut recovered, latest) = Journal::recover(&cfg, "owner");
        let latest = latest.unwrap();
        assert_eq!(latest["state"], "interrupted");
        assert_eq!(latest["cancel_requested"], true);
        assert_eq!(latest["termination_confirmed"], false);
        assert!(recovered.cancel(id).is_err());
        assert!(recovered.finish(id, State::Installed).is_err());
    }
    #[test]
    fn corrupt_future_or_misowned_journal_is_retained_without_repair() {
        for fault in [
            "checksum", "payload", "oversize", "future", "owner", "ordinal",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let cfg = dir.path().join("node.json");
            config(&cfg);
            let mut store = Store::open(&cfg, "owner", Policy::default()).unwrap();
            store.claim(&progress(), "a".repeat(64)).unwrap();
            match fault {
                "checksum" => {
                    store
                        .db
                        .execute(
                            "UPDATE model_download_receipts SET payload_sha256='bad'",
                            [],
                        )
                        .unwrap();
                }
                "payload" => {
                    store
                        .db
                        .execute("UPDATE model_download_receipts SET payload='{}'", [])
                        .unwrap();
                }
                "oversize" => {
                    store
                        .db
                        .execute(
                            "UPDATE model_download_receipts SET payload=?1",
                            ["x".repeat(MAX_RECORD_BYTES + 1)],
                        )
                        .unwrap();
                }
                "future" => {
                    store.db.pragma_update(None, "user_version", 2).unwrap();
                }
                "owner" => {
                    store
                        .db
                        .execute("UPDATE receipt_owner SET node_id='different'", [])
                        .unwrap();
                }
                "ordinal" => {
                    store
                        .db
                        .execute("UPDATE model_download_receipts SET ordinal=2", [])
                        .unwrap();
                }
                _ => unreachable!(),
            }
            drop(store);
            let before = std::fs::read(path(&cfg)).unwrap();
            let (recovered, latest) = Journal::recover(&cfg, "owner");
            assert!(matches!(recovered, Journal::Unavailable), "{fault}");
            assert_eq!(latest.unwrap()["state"], "unavailable");
            assert_eq!(std::fs::read(path(&cfg)).unwrap(), before, "{fault}");
        }
    }
    #[test]
    fn write_faults_keep_acceptance_and_final_outcomes_uncertain_without_repair() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        config(&cfg);
        let p = progress();
        let mut store = Store::open(&cfg, "owner", Policy::default()).unwrap();
        store.db.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON model_download_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(store.claim(&p, "a".repeat(64)).is_err());
        assert!(store.records().unwrap().is_empty());
        store.db.execute_batch("DROP TRIGGER fail_insert").unwrap();
        let r = store.claim(&p, "a".repeat(64)).unwrap();
        store.db.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON model_download_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(store.change(&r.id, Some(State::Installed)).is_err());
        assert_eq!(store.get(&r.id).unwrap().unwrap().state, State::Downloading);
        drop(store);
        let (failed, _) = Journal::recover(&cfg, "owner");
        assert!(matches!(failed, Journal::Unavailable));
        let db = Connection::open(path(&cfg)).unwrap();
        db.execute_batch("DROP TRIGGER fail_update").unwrap();
        drop(db);
        let (recovered, _) = Journal::recover(&cfg, "owner");
        assert_eq!(
            recovered.get(&r.id).unwrap().unwrap().state,
            State::Interrupted
        );
    }
    #[test]
    fn quota_full_never_evicts_old_ids_or_admits_another_operation() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        config(&cfg);
        let mut store = Store::open(
            &cfg,
            "owner",
            Policy {
                max_records: 1,
                max_bytes: 64 * 1024,
            },
        )
        .unwrap();
        let r = store.claim(&progress(), "a".repeat(64)).unwrap();
        store.change(&r.id, Some(State::Error)).unwrap();
        assert!(store.claim(&progress(), "a".repeat(64)).is_err());
        assert_eq!(store.records().unwrap().len(), 1);
        assert!(store.get(&r.id).unwrap().is_some());
        assert!(std::fs::metadata(path(&cfg)).unwrap().len() <= 64 * 1024);
    }
    #[test]
    fn configured_capacity_increase_applies_only_to_a_new_explicit_admission() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        let mut value = config(&cfg);
        value["models"]["download_receipts"] = json!({"max_records":1,"max_bytes":65536});
        let mut journal = Journal::Empty;
        let first = journal.begin(&cfg, "owner", &value, &progress()).unwrap();
        journal.finish(&first.id, State::Error).unwrap();
        assert_eq!(
            journal.availability(&value).unwrap()["blocker"],
            "receipt_quota_full"
        );
        value["models"]["download_receipts"]["max_records"] = json!(2);
        let before = std::fs::read(path(&cfg)).unwrap();
        assert_eq!(journal.availability(&value).unwrap()["available"], true);
        assert_eq!(std::fs::read(path(&cfg)).unwrap(), before);
        let second = journal.begin(&cfg, "owner", &value, &progress()).unwrap();
        assert_eq!(second.sequence, 2);
        assert!(journal.get(&first.id).unwrap().is_some());
    }
    #[test]
    fn missing_journal_is_read_only_and_invalid_policy_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        let mut value = config(&cfg);
        let (mut journal, latest) = Journal::recover(&cfg, "owner");
        assert!(latest.is_none());
        assert!(
            journal
                .get(&uuid::Uuid::new_v4().to_string())
                .unwrap()
                .is_none()
        );
        assert!(!path(&cfg).exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        value["models"]["download_receipts"] = json!({"max_records":0});
        assert!(journal.begin(&cfg, "owner", &value, &progress()).is_err());
        assert!(!path(&cfg).exists());
    }
    #[test]
    fn later_corruption_blocks_read_and_new_admission_without_exposing_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("node.json");
        config(&cfg);
        let mut store = Store::open(&cfg, "owner", Policy::default()).unwrap();
        let r = store.claim(&progress(), "a".repeat(64)).unwrap();
        let text = serde_json::to_string(&r.progress()).unwrap();
        for forbidden in ["https://", "source_page", "path", "url", "prompt"] {
            assert!(!text.contains(forbidden));
        }
        store
            .db
            .execute(
                "UPDATE model_download_receipts SET payload_sha256='damaged'",
                [],
            )
            .unwrap();
        assert!(store.get(&r.id).is_err());
        assert!(store.claim(&progress(), "a".repeat(64)).is_err());
    }
}
