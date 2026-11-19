use super::*;
use std::sync::atomic::AtomicUsize;

fn engine() -> Arc<TaskEngine> {
    TaskEngine::new(EventBus::new(20))
}
fn approved(engine: &TaskEngine) -> Plan {
    let (plan, _) = crate::workload_jobs::tests::planned(engine);
    crate::workload_jobs::approve(
        engine,
        &crate::workload_jobs::ApprovalCommand {
            id: plan.plan_id.clone(),
            expected_revision: plan.revision,
            operation_id: Uuid::new_v4().to_string(),
            plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
            valid_for_ms: 120000,
        },
        &"a".repeat(64),
    )
    .unwrap()
}

#[test]
fn pending_workload_registration_excludes_duplicate_and_legacy_claims_without_creating_history() {
    let engine = engine();
    let id = Uuid::new_v4().simple().to_string();
    let owner = engine.claim_workload(&id).unwrap();
    assert!(engine.claim_workload(&id).is_err());
    assert!(engine.claim(&id, || Ok(())).is_err());
    assert!(owner.check(&Uuid::new_v4().simple().to_string()).is_err());
    owner.check(&id).unwrap();
    assert!(
        engine.store.get(&id).is_err(),
        "Registration is not a persisted job or issuer"
    );
    drop(owner);
    let next = engine.claim_workload(&id).unwrap();
    next.check(&id).unwrap();
}

#[test]
fn engine_workload_capacity_reuses_original_journal_final_reporting_bound() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    drop(rusqlite::Connection::open(&path).unwrap());
    let store = crate::jobs::JobStore::open(&path, 1, 1024 * 1024).unwrap();
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let one = Uuid::new_v4().simple().to_string();
    let two = Uuid::new_v4().simple().to_string();
    let owner = engine.claim_workload(&one).unwrap();
    assert!(engine.claim_workload(&two).is_err());
    drop(owner);
    engine.claim_workload(&two).unwrap();
}

#[test]
fn stopping_preparing_workload_cancels_unissued_plan_without_creating_an_execution() {
    let engine = engine();
    let plan = approved(&engine);
    let owner = engine.claim_workload(&plan.plan_id).unwrap();
    let stopped = engine.cancel(&plan.plan_id).unwrap();
    assert_eq!(stopped.status, PlanStatus::Cancelled);
    assert!(stopped.execution.is_none());
    assert!(
        stopped
            .tasks
            .iter()
            .all(|task| task.status == TaskStatus::Cancelled)
    );
    assert!(owner.check(&plan.plan_id).is_err());
    assert!(
        owner
            .deliver(&plan.plan_id, || panic!("Stop cannot enqueue control"))
            .is_err()
    );
    assert!(
        engine.claim_workload(&plan.plan_id).is_err(),
        "Original guard stays tracked until dropped"
    );
}

#[tokio::test]
async fn public_stop_holds_only_the_target_claim_gate_during_a_contended_sqlite_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    let blocker = rusqlite::Connection::open(&path).unwrap();
    let store = crate::jobs::JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let plan = approved(&engine);
    let id = &plan.plan_id;
    let mut changes = engine.stop_changes.subscribe();
    let original = engine.claim_workload(id).unwrap();
    let mut retired = original.stop_receiver();

    // Another connection reserves the real database writer while allowing reads.
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut stop = Box::pin(engine.stop_job_request(id, None));
    tokio::select! {
        biased;
        result = &mut stop => panic!("Stop completed while the SQLite writer was held: {result:?}"),
        result = timeout(Duration::from_secs(2), async {
            // Wait for the worker's actual revocation before probing the mutex.
            // Earlier probes could cause the intentional busy refusal themselves.
            retired.changed().await.unwrap();
            assert!(*retired.borrow());
            loop {
                if engine.active.try_lock().is_ok_and(|active| active.pending_stop_intents.contains(id)) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        }) => result.expect("Stop must release the active-job lock before writing"),
    }
    drop(original);
    assert!(engine.claim_workload(id).is_err());
    assert!(
        engine
            .claim(id, || panic!("A pending stop must exclude admission"))
            .is_err()
    );
    let unrelated = Uuid::new_v4().simple().to_string();
    engine
        .claim_workload(&unrelated)
        .unwrap()
        .check(&unrelated)
        .unwrap();
    assert!(!changes.has_changed().unwrap());

    blocker.execute_batch("ROLLBACK").unwrap();
    let stopped = stop.await.unwrap();
    assert_eq!(stopped["status"], "cancelled");
    changes.changed().await.unwrap();
    assert!(
        !engine
            .active
            .lock()
            .unwrap()
            .pending_stop_intents
            .contains(id)
    );
    assert!(engine.store.get(id).unwrap().execution.is_none());
}

#[tokio::test]
async fn public_stop_refuses_a_contended_active_gate_without_reversing_lock_order() {
    let engine = engine();
    let plan = approved(&engine);
    let owner = engine.claim_workload(&plan.plan_id).unwrap();
    let active = engine.active.lock().unwrap();
    let error = engine
        .stop_job_request(&plan.plan_id, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Active job control is busy"));
    drop(active);
    owner.check(&plan.plan_id).unwrap();
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(plan));
}

#[tokio::test]
async fn shutdown_revokes_control_before_waiting_for_original_workload_lifetime() {
    let engine = engine();
    let id = Uuid::new_v4().simple().to_string();
    let owner = engine.claim_workload(&id).unwrap();
    let mut stopping = Box::pin(engine.shutdown());
    tokio::select! {
        biased;
        _=&mut stopping=>panic!("Shutdown returned while original lifetime was held"),
        _=sleep(Duration::from_millis(20))=>{}
    }
    assert!(owner.check(&id).is_err());
    assert!(
        engine
            .claim_workload(&Uuid::new_v4().simple().to_string())
            .is_err()
    );
    drop(owner);
    timeout(Duration::from_secs(1), stopping).await.unwrap();
}

#[tokio::test]
async fn failed_stop_checkpoint_still_revokes_original_control_and_notifies_its_runner() {
    let engine = engine();
    let plan = approved(&engine);
    let owner = engine.claim_workload(&plan.plan_id).unwrap();
    let mut changes = engine.stop_changes.subscribe();
    engine
        .store
        .with_test_journal_lock(|db| db.execute_batch("PRAGMA query_only=ON"))
        .unwrap();
    assert!(engine.cancel(&plan.plan_id).is_err());
    assert!(owner.check(&plan.plan_id).is_err());
    assert!(
        owner
            .deliver(&plan.plan_id, || panic!(
                "Failed persistence cannot reopen control"
            ))
            .is_err()
    );
    timeout(Duration::from_secs(1), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(plan));
    engine
        .store
        .with_test_journal_lock(|db| db.execute_batch("PRAGMA query_only=OFF"))
        .unwrap();
}

#[tokio::test]
async fn registration_drop_and_engine_quiescence_do_not_wait_on_a_blocked_journal() {
    let engine = engine();
    let plan = approved(&engine);
    let owner = engine.claim_workload(&plan.plan_id).unwrap();
    let store = engine.store.clone();
    let id = plan.plan_id.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let (locked, ready) = tokio::sync::oneshot::channel();
    let blocker = std::thread::spawn(move || {
        store
            .edit(&id, "Held real journal; no mutation", |_| {
                let _ = locked.send(());
                assert!(wait.recv_timeout(Duration::from_secs(8)).is_ok());
                Ok(false)
            })
            .unwrap();
    });
    ready.await.unwrap();
    let start = Instant::now();
    drop(owner);
    assert!(start.elapsed() < Duration::from_secs(1));
    timeout(Duration::from_secs(1), engine.shutdown())
        .await
        .unwrap();
    release.send(()).unwrap();
    blocker.join().unwrap();
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(plan));
}

#[test]
fn stop_and_control_delivery_have_one_in_memory_order_without_a_storage_lock() {
    let engine = engine();
    let id = Uuid::new_v4().simple().to_string();
    let owner = engine.claim_workload(&id).unwrap();
    let state = owner.state.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let sent = count.clone();
    let delivery_id = id.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let (entered, ready) = std::sync::mpsc::channel();
    let delivery = std::thread::spawn(move || {
        owner
            .deliver(&delivery_id, || {
                entered.send(()).unwrap();
                assert!(wait.recv_timeout(Duration::from_secs(5)).is_ok());
                sent.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        owner
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let (started, stopping) = std::sync::mpsc::channel();
    let cancel = std::thread::spawn(move || {
        started.send(()).unwrap();
        state.cancel();
    });
    stopping.recv_timeout(Duration::from_secs(2)).unwrap();
    release.send(()).unwrap();
    let owner = delivery.join().unwrap();
    cancel.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(
        owner
            .deliver(&id, || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
