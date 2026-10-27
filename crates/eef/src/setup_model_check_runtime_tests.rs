use super::*;
use crate::setup_model_check::tests::definition;
fn manager() -> Arc<Manager> {
    Manager::new(&crate::config::Config::load(None).unwrap()).unwrap()
}

#[test]
fn dropped_and_reopened_ownership_never_recreates_a_worker_from_retained_consent() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let work = manager
        .prepare(&store, &runtime, definition.clone())
        .unwrap()
        .unwrap();
    assert_eq!(manager.active.lock().unwrap().len(), 1);
    drop(work);
    assert!(manager.active.lock().unwrap().is_empty());
    assert!(
        manager
            .prepare(&store, &runtime, definition.clone())
            .unwrap()
            .is_none()
    );
    let fresh = Manager::new(&crate::config::Config::load(None).unwrap()).unwrap();
    assert!(
        fresh
            .prepare(
                &store,
                &uuid::Uuid::new_v4().to_string(),
                definition.clone()
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(store.usage().unwrap()["model_verification_intent_count"], 0);
    assert_eq!(
        store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .unwrap()
            .state,
        State::Waiting
    );
}
#[tokio::test]
async fn per_node_capacity_and_shutdown_refuse_fresh_consent_without_a_queue() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    let first = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let a = manager
        .prepare(&store, &runtime, first.clone())
        .unwrap()
        .unwrap();
    let mut same = first.clone();
    same.operation_id = uuid::Uuid::new_v4().to_string();
    assert!(manager.prepare(&store, &runtime, same.clone()).is_err());
    assert!(
        store
            .setup_model_check(&same.node_id, &same.operation_id)
            .unwrap()
            .is_none()
    );
    let mut second = definition();
    second.node_id = "other-node".into();
    let b = manager.prepare(&store, &runtime, second).unwrap().unwrap();
    let mut third = definition();
    third.node_id = "third-node".into();
    assert!(manager.prepare(&store, &runtime, third.clone()).is_err());
    assert!(
        store
            .setup_model_check(&third.node_id, &third.operation_id)
            .unwrap()
            .is_none()
    );
    assert!(manager.prepare(&store, &runtime, first).unwrap().is_none());
    manager.close().await;
    drop((a, b));
    assert!(manager.prepare(&store, &runtime, third).is_err());
    assert_eq!(store.usage().unwrap()["setup_model_check_count"], 2);
}
#[test]
fn waiting_cancellation_notifies_only_the_exact_operation_and_prevents_launch() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let work = manager
        .prepare(&store, &runtime, definition.clone())
        .unwrap()
        .unwrap();
    manager.cancel(&definition.node_id, &uuid::Uuid::new_v4().to_string());
    assert!(!*work.cancelled.borrow());
    assert!(
        store
            .cancel_setup_model_check_wait(&definition.node_id, &definition.operation_id)
            .unwrap()
    );
    manager.cancel(&definition.node_id, &definition.operation_id);
    assert!(*work.cancelled.borrow());
    assert!(
        !store
            .launch_setup_model_check(&definition, &runtime)
            .unwrap()
    );
    drop(work);
}
#[test]
fn failed_durable_admission_creates_no_owned_worker_or_replayable_intent() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    store.with_test_journal_lock(|connection|connection.execute_batch("CREATE TRIGGER refuse_check BEFORE INSERT ON setup_model_checks BEGIN SELECT RAISE(ABORT,'synthetic full disk'); END;").unwrap());
    let definition = definition();
    assert!(
        manager
            .prepare(
                &store,
                &uuid::Uuid::new_v4().to_string(),
                definition.clone()
            )
            .is_err()
    );
    assert!(manager.active.lock().unwrap().is_empty());
    assert!(
        store
            .setup_model_check(&definition.node_id, &definition.operation_id)
            .unwrap()
            .is_none()
    );
}
#[test]
fn snapshots_separate_live_observation_claim_and_unknown_retirement() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    let definition = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let before = Instant::now();
    let work = manager
        .prepare(&store, &runtime, definition.clone())
        .unwrap()
        .unwrap();
    let after = Instant::now();
    let window = Duration::from_millis(definition.max_wait_ms);
    assert!(work.deadline >= before + window && work.deadline <= after + window);
    assert!(
        manager
            .prepare(&store, &runtime, definition.clone())
            .unwrap()
            .is_none()
    );
    let (record, active, reason) = manager
        .snapshot(
            &store,
            &runtime,
            &definition.node_id,
            &definition.operation_id,
        )
        .unwrap();
    assert!(active);
    assert_eq!(reason.as_deref(), Some("waiting_for_setup"));
    assert_eq!(record.unwrap().state, State::Waiting);
    assert!(
        store
            .launch_setup_model_check(&definition, &runtime)
            .unwrap()
    );
    let (record, active, reason) = manager
        .snapshot(
            &store,
            &runtime,
            &definition.node_id,
            &definition.operation_id,
        )
        .unwrap();
    assert!(active);
    assert!(reason.is_none());
    assert_eq!(record.unwrap().state, State::Launching);
    store
        .finish_setup_model_check(&definition, &runtime, None, true)
        .unwrap();
    let (record, active, reason) = manager
        .snapshot(
            &store,
            &runtime,
            &definition.node_id,
            &definition.operation_id,
        )
        .unwrap();
    assert!(!active);
    assert!(reason.is_none());
    assert_eq!(record.unwrap().state, State::Unknown);
    drop(work);
}

#[test]
fn retired_worker_drop_cannot_remove_a_new_exact_operation_on_the_same_node() {
    let manager = manager();
    let store = JobStore::in_memory().unwrap();
    let first = definition();
    let runtime = uuid::Uuid::new_v4().to_string();
    let old = manager
        .prepare(&store, &runtime, first.clone())
        .unwrap()
        .unwrap();
    store
        .cancel_setup_model_check_wait(&first.node_id, &first.operation_id)
        .unwrap();
    let mut second = definition();
    second.node_id = first.node_id.clone();
    let next = manager
        .prepare(&store, &runtime, second.clone())
        .unwrap()
        .unwrap();
    drop(old);
    assert_eq!(
        manager
            .active
            .lock()
            .unwrap()
            .get(&second.node_id)
            .unwrap()
            .operation,
        second.operation_id
    );
    assert!(
        manager
            .snapshot(&store, &runtime, &second.node_id, &second.operation_id)
            .unwrap()
            .1
    );
    drop(next);
    assert!(manager.active.lock().unwrap().is_empty());
}
