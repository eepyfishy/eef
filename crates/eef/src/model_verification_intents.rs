//! Immutable pre-dispatch intent, not approval or a resumable workflow.
use super::*;
use eefn::model_verification::{Definition, receipt_id};

const MAX_RECORD_BYTES: usize = 4096;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    definition: Definition,
    coordinator_runtime_id: String,
    created_at_ms: u64,
}
impl Intent {
    fn validate(&self) -> Result<()> {
        self.definition.validate()?;
        eefn::automation_occurrence::uuid(&self.coordinator_runtime_id)?;
        if self.schema_version != 1 || self.created_at_ms == 0 {
            bail!("Unsupported model-check intent; retain without replay")
        }
        Ok(())
    }
}

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS model_verification_intents (source_id TEXT PRIMARY KEY, record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB))<=4096));")?;
    Ok(())
}
pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let n: i64 = connection.query_row(
        "SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM model_verification_intents",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n)?)
}
fn read(connection: &Connection, node: &str, operation: &str) -> Result<Option<Definition>> {
    let source = receipt_id(node, operation, "source", None)?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT record_json FROM model_verification_intents WHERE source_id=?1",
            [&source],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    if raw.len() > MAX_RECORD_BYTES {
        bail!("Oversized model-check intent; retain without replay")
    }
    let intent: Intent = serde_json::from_str(&raw)
        .context("Damaged or future model-check intent; retain without replay")?;
    intent.validate()?;
    if intent.definition.node_id != node || intent.definition.operation_id != operation {
        bail!("Model-check intent row identity changed; retain without replay")
    }
    Ok(Some(intent.definition))
}
impl JobStore {
    pub(crate) fn model_verification_intent(
        &self,
        node: &str,
        operation: &str,
    ) -> Result<Option<Definition>> {
        read(
            &self.connection.lock().expect("job journal"),
            node,
            operation,
        )
    }

    /// Commit before the first effectful remote request. False means inspect only.
    /// No deletion/update path: removing a finished source cannot restore authority.
    pub(crate) fn reserve_model_verification_intent(
        &self,
        definition: &Definition,
        coordinator_runtime_id: &str,
    ) -> Result<bool> {
        let intent = Intent {
            schema_version: 1,
            definition: definition.clone(),
            coordinator_runtime_id: coordinator_runtime_id.into(),
            created_at_ms: eefn::protocol::now_ms(),
        };
        intent.validate()?;
        let connection = self.connection.lock().expect("job journal");
        if let Some(existing) = read(&connection, &definition.node_id, &definition.operation_id)? {
            if existing != *definition {
                bail!("Retained model-check intent conflicts; no replacement or retry")
            }
            return Ok(false);
        }
        let raw = serde_json::to_string(&intent)?;
        if raw.len() > MAX_RECORD_BYTES || automations::count(&connection)? >= self.max_count {
            bail!("Model-check intent history is full; no remote preparation requested")
        }
        let jobs: i64 = connection.query_row(
            "SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs",
            [], |r| r.get(0),
        )?;
        let used = usize::try_from(jobs)?
            .checked_add(automations::definition_bytes(&connection)?)
            .context("Model-check intent storage overflow")?;
        if raw.len() > self.max_bytes.saturating_sub(used) {
            bail!("Model-check intent storage is full; no remote preparation requested")
        }
        // The shared journal lock spans lookup, quota admission and the SQLite
        // autocommit (synchronous=FULL). An uncertain commit cannot admit a send.
        connection.execute(
            "INSERT INTO model_verification_intents(source_id,record_json) VALUES (?1,?2)",
            params![definition.receipt("source", None)?, raw],
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> Definition {
        Definition {
            schema_version: 1,
            node_id: "node".into(),
            node_runtime_id: uuid::Uuid::new_v4().to_string(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            model: eefn::setup_check::ModelRef {
                backend: eefn::model_selection::Backend::Ollama,
                model_id: "chosen".into(),
            },
        }
    }
    #[test]
    fn immutable_intent_retains_exact_binding_without_a_job() {
        let store = JobStore::in_memory().unwrap();
        let d = definition();
        assert!(
            store
                .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                .unwrap()
        );
        assert_eq!(
            store
                .model_verification_intent(&d.node_id, &d.operation_id)
                .unwrap(),
            Some(d.clone())
        );
        assert!(
            !store
                .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                .unwrap()
        );
        for field in ["runtime", "model", "backend"] {
            let mut changed = d.clone();
            match field {
                "runtime" => changed.node_runtime_id = uuid::Uuid::new_v4().to_string(),
                "model" => changed.model.model_id = "other".into(),
                _ => changed.model.backend = eefn::model_selection::Backend::Llamacpp,
            }
            assert!(
                store
                    .reserve_model_verification_intent(&changed, &uuid::Uuid::new_v4().to_string())
                    .is_err()
            );
        }
        assert!(store.list().unwrap().is_empty());
        let usage = store.usage().unwrap();
        assert_eq!(usage["model_verification_intent_count"], 1);
        assert!(usage["model_verification_intent_bytes"].as_u64().unwrap() > 0);
        assert_eq!(usage["count"], 0);
    }
    #[test]
    fn reopened_intent_is_history_not_a_new_start() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let d = definition();
        let store = JobStore::open(&path, 20, 65536).unwrap();
        assert!(
            store
                .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                .unwrap()
        );
        drop(store);
        let store = JobStore::open(&path, 20, 65536).unwrap();
        assert_eq!(
            store
                .model_verification_intent(&d.node_id, &d.operation_id)
                .unwrap(),
            Some(d.clone())
        );
        assert!(
            !store
                .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                .unwrap()
        );
    }
    #[test]
    fn concurrent_admission_has_one_owner() {
        let store = JobStore::in_memory().unwrap();
        let d = definition();
        let wins: usize = std::thread::scope(|scope| {
            (0..16)
                .map(|_| {
                    let store = store.clone();
                    let d = d.clone();
                    scope.spawn(move || {
                        usize::from(
                            store
                                .reserve_model_verification_intent(
                                    &d,
                                    &uuid::Uuid::new_v4().to_string(),
                                )
                                .unwrap(),
                        )
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|t| t.join().unwrap())
                .sum()
        });
        assert_eq!(wins, 1);
        assert_eq!(store.usage().unwrap()["model_verification_intent_count"], 1);
    }
    #[test]
    fn damaged_future_and_conflicting_records_are_not_replaced() {
        for change in [
            "future",
            "runtime",
            "timestamp",
            "node",
            "operation",
            "unknown",
        ] {
            let store = JobStore::in_memory().unwrap();
            let d = definition();
            let mut record = json!(Intent {
                schema_version: 1,
                definition: d.clone(),
                coordinator_runtime_id: uuid::Uuid::new_v4().to_string(),
                created_at_ms: 1
            });
            match change {
                "future" => record["schema_version"] = json!(2),
                "runtime" => record["coordinator_runtime_id"] = json!("invalid"),
                "timestamp" => record["created_at_ms"] = json!(0),
                "node" => record["definition"]["node_id"] = json!("other"),
                "operation" => {
                    record["definition"]["operation_id"] = json!(uuid::Uuid::new_v4().to_string())
                }
                _ => record["automatic_retry"] = json!(true),
            }
            let raw = record.to_string();
            store
                .connection
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO model_verification_intents(source_id,record_json) VALUES (?1,?2)",
                    params![d.receipt("source", None).unwrap(), raw],
                )
                .unwrap();
            assert!(
                store
                    .model_verification_intent(&d.node_id, &d.operation_id)
                    .is_err(),
                "{change}"
            );
            assert!(
                store
                    .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                    .is_err(),
                "{change}"
            );
            let retained: String = store
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT record_json FROM model_verification_intents",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(retained, raw);
        }
    }
    #[test]
    fn limits_and_failed_writes_do_not_admit_intent() {
        for failure in ["count", "bytes", "write"] {
            let mut store = JobStore::in_memory().unwrap();
            match failure {
                "count" => Arc::get_mut(&mut store).unwrap().max_count = 0,
                "bytes" => Arc::get_mut(&mut store).unwrap().max_bytes = 1,
                _ => store.connection.lock().unwrap().execute_batch("CREATE TRIGGER refuse_model_intent BEFORE INSERT ON model_verification_intents BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap(),
            }
            let d = definition();
            assert!(
                store
                    .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                    .is_err()
            );
            assert!(
                store
                    .model_verification_intent(&d.node_id, &d.operation_id)
                    .unwrap()
                    .is_none()
            );
        }
    }
    #[test]
    fn intent_bytes_are_in_all_shared_quota_aggregates() {
        let store = JobStore::in_memory().unwrap();
        let d = definition();
        store
            .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        let connection = store.connection.lock().unwrap();
        let cost = bytes(&connection).unwrap();
        assert!(cost > 0);
        assert_eq!(automations::count(&connection).unwrap(), 1);
        assert_eq!(automations::definition_bytes(&connection).unwrap(), cost);
        drop(connection);
        let usage = store.usage().unwrap();
        assert_eq!(
            usage["available_bytes"].as_u64().unwrap(),
            (store.max_bytes - cost) as u64
        );
    }
    #[test]
    fn intent_and_job_admission_share_count_and_byte_limits_both_directions() {
        let plan = || {
            crate::task::TaskPlanner.plan(serde_json::from_value::<crate::task::Goal>(json!({"description":"quota fixture", "template":"write_file", "params":{"path":"fixture","content":"synthetic"}})).unwrap()).unwrap()
        };
        let mut store = JobStore::in_memory().unwrap();
        Arc::get_mut(&mut store).unwrap().max_count = 1;
        let d = definition();
        store
            .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        assert!(store.insert(plan()).is_err());
        assert!(store.list().unwrap().is_empty());
        let mut store = JobStore::in_memory().unwrap();
        Arc::get_mut(&mut store).unwrap().max_count = 1;
        store.insert(plan()).unwrap();
        assert!(
            store
                .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                .is_err()
        );
        assert_eq!(store.usage().unwrap()["model_verification_intent_count"], 0);
        for intent_first in [true, false] {
            let mut store = JobStore::in_memory().unwrap();
            if intent_first {
                store
                    .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                    .unwrap();
            } else {
                store.insert(plan()).unwrap();
            }
            let used = store.max_bytes
                - store.usage().unwrap()["available_bytes"].as_u64().unwrap() as usize;
            Arc::get_mut(&mut store).unwrap().max_bytes = used;
            if intent_first {
                assert!(store.insert(plan()).is_err());
            } else {
                assert!(
                    store
                        .reserve_model_verification_intent(&d, &uuid::Uuid::new_v4().to_string())
                        .is_err()
                );
            }
        }
    }
}
