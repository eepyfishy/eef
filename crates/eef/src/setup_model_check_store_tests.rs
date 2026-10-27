use super::*;
use crate::setup_model_check::tests::definition;

#[test]
fn immutable_consent_reuse_never_changes_target_runtime_or_restores_waiting() {
    let store = JobStore::in_memory().unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let (original, created) = store
        .arm_setup_model_check(definition.clone(), &runtime)
        .unwrap();
    assert!(created);
    let (again, created) = store
        .arm_setup_model_check(definition.clone(), &uuid::Uuid::new_v4().to_string())
        .unwrap();
    assert!(!created);
    assert_eq!(original, again);
    let mut changed = definition.clone();
    changed.model.model_id = "other".into();
    assert!(store.arm_setup_model_check(changed, &runtime).is_err());
    assert!(
        store
            .cancel_setup_model_check_wait(&definition.node_id, &definition.operation_id)
            .unwrap()
    );
    assert!(
        !store
            .launch_setup_model_check(&definition, &runtime)
            .unwrap()
    );
    assert!(
        !store
            .arm_setup_model_check(definition.clone(), &runtime)
            .unwrap()
            .1
    );
    assert_eq!(
        store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .unwrap()
            .state,
        State::Cancelled
    );
}
#[test]
fn records_share_count_and_full_update_reservations_with_jobs_and_intents() {
    let store =
        JobStore::initialize(Connection::open_in_memory().unwrap(), None, 1, RECORD_BYTES).unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    store
        .arm_setup_model_check(definition.clone(), &runtime)
        .unwrap();
    let usage = store.usage().unwrap();
    assert_eq!(usage["setup_model_check_count"], 1);
    assert_eq!(usage["setup_model_check_reserved_bytes"], RECORD_BYTES);
    assert_eq!(usage["available_bytes"], 0);
    assert_eq!(usage["over_limit"], false);
    let mut other = definition.clone();
    other.operation_id = uuid::Uuid::new_v4().to_string();
    assert!(store.arm_setup_model_check(other, &runtime).is_err());
    let intent = eefn::model_verification::Definition {
        schema_version: 1,
        node_id: definition.node_id.clone(),
        node_runtime_id: uuid::Uuid::new_v4().to_string(),
        operation_id: definition.operation_id.clone(),
        model: definition.model.clone(),
    };
    assert!(
        store
            .reserve_model_verification_intent(&intent, &runtime)
            .is_err()
    );
    let too_small = JobStore::initialize(
        Connection::open_in_memory().unwrap(),
        None,
        10,
        RECORD_BYTES - 1,
    )
    .unwrap();
    assert!(
        too_small
            .arm_setup_model_check(definition, &runtime)
            .is_err()
    );
    assert_eq!(too_small.usage().unwrap()["setup_model_check_count"], 0);
}
#[test]
fn launch_and_cancel_have_one_atomic_winner() {
    for _ in 0..12 {
        let store = JobStore::in_memory().unwrap();
        let definition = definition();
        let runtime = uuid::Uuid::new_v4().to_string();
        store
            .arm_setup_model_check(definition.clone(), &runtime)
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let launch = {
            let store = store.clone();
            let definition = definition.clone();
            let runtime = runtime.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .launch_setup_model_check(&definition, &runtime)
                    .unwrap()
            })
        };
        let cancel = {
            let store = store.clone();
            let definition = definition.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .cancel_setup_model_check_wait(&definition.node_id, &definition.operation_id)
                    .unwrap()
            })
        };
        assert_ne!(launch.join().unwrap(), cancel.join().unwrap());
        assert!(
            !store
                .launch_setup_model_check(&definition, &runtime)
                .unwrap()
        );
        assert!(!store.arm_setup_model_check(definition, &runtime).unwrap().1);
    }
}
#[test]
fn reopen_preserves_consent_and_never_changes_its_original_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    drop(Connection::open(&path).unwrap());
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let store = JobStore::open(&path, 100, 65536).unwrap();
    store
        .arm_setup_model_check(definition.clone(), &runtime)
        .unwrap();
    drop(store);
    let store = JobStore::open(&path, 100, 65536).unwrap();
    let fresh = uuid::Uuid::new_v4().to_string();
    let (record, created) = store
        .arm_setup_model_check(definition.clone(), &fresh)
        .unwrap();
    assert!(!created);
    assert_eq!(record.coordinator_runtime_id, runtime);
    assert!(store.launch_setup_model_check(&definition, &fresh).is_err());
    assert_eq!(
        crate::setup_model_check::effective_state(&record, &fresh, false),
        State::Interrupted
    );
}
#[test]
fn timeout_before_launch_and_uncertainty_after_launch_are_distinct_and_terminal() {
    for launch in [false, true] {
        let store = JobStore::in_memory().unwrap();
        let definition = definition();
        let runtime = uuid::Uuid::new_v4().to_string();
        store
            .arm_setup_model_check(definition.clone(), &runtime)
            .unwrap();
        if launch {
            assert!(
                store
                    .launch_setup_model_check(&definition, &runtime)
                    .unwrap()
            );
        }
        store
            .finish_setup_model_check(&definition, &runtime, None, true)
            .unwrap();
        let record = store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.state,
            if launch {
                State::Unknown
            } else {
                State::Expired
            }
        );
        assert!(!store.arm_setup_model_check(definition, &runtime).unwrap().1);
    }
}
#[test]
fn malformed_future_oversized_and_conflicting_rows_are_retained_not_repaired() {
    for kind in ["future", "unknown-field", "oversized", "quota"] {
        let store = JobStore::in_memory().unwrap();
        let definition = definition();
        let runtime = uuid::Uuid::new_v4().to_string();
        let (record, _) = store
            .arm_setup_model_check(definition.clone(), &runtime)
            .unwrap();
        let mut value = serde_json::to_value(record).unwrap();
        if kind == "future" {
            value["schema_version"] = json!(999);
        } else if kind == "unknown-field" {
            value["PRIVATE"] = json!("secret");
        }
        let raw = if kind == "oversized" {
            "x".repeat(RECORD_BYTES + 1)
        } else {
            value.to_string()
        };
        store.with_test_journal_lock(|connection| {
            connection
                .execute_batch("PRAGMA ignore_check_constraints=ON")
                .unwrap();
            connection
                .execute(
                    "UPDATE setup_model_checks SET record_json=?1,reserved_bytes=?2",
                    params![
                        raw,
                        if kind == "quota" {
                            0
                        } else {
                            RECORD_BYTES as i64
                        }
                    ],
                )
                .unwrap();
        });
        assert!(
            store
                .setup_model_check(&definition.node_id, &definition.operation_id)
                .is_err()
        );
        if matches!(kind, "oversized" | "quota") {
            assert!(
                store.usage().is_err(),
                "Corrupt records cannot understate shared usage"
            );
        }
        assert!(store.arm_setup_model_check(definition, &runtime).is_err());
        store.with_test_journal_lock(|connection| {
            let retained: String = connection
                .query_row("SELECT record_json FROM setup_model_checks", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(retained, raw);
        });
    }
}
#[test]
fn failed_commit_does_not_create_consent_or_admit_a_worker() {
    let store = JobStore::in_memory().unwrap();
    store.with_test_journal_lock(|connection|connection.execute_batch("CREATE TRIGGER refuse_setup_check BEFORE INSERT ON setup_model_checks BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap());
    let definition = definition();
    assert!(
        store
            .arm_setup_model_check(definition.clone(), &uuid::Uuid::new_v4().to_string())
            .is_err()
    );
    assert!(
        store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .is_none()
    );
}
#[test]
fn existing_model_check_intent_cannot_be_rearmed_as_automatic_consent() {
    let store = JobStore::in_memory().unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let intent = eefn::model_verification::Definition {
        schema_version: 1,
        node_id: definition.node_id.clone(),
        node_runtime_id: uuid::Uuid::new_v4().to_string(),
        operation_id: definition.operation_id.clone(),
        model: definition.model.clone(),
    };
    store
        .reserve_model_verification_intent(&intent, &runtime)
        .unwrap();
    assert!(
        store
            .arm_setup_model_check(definition.clone(), &runtime)
            .is_err()
    );
    assert!(
        store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .is_none()
    );
}
