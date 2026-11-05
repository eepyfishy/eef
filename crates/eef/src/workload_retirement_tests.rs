// Included with the existing workload fixture helpers. These never launch a
// source, connect a node, infer a model or change the system clock.
#[tokio::test]
async fn insufficient_completion_quota_refuses_before_waiting_for_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(&dir.path().join("small-journal.db"), 1024);
    assert_eq!(store.workload_finalization_available(), 0);
    let command = ExecuteCommand {
        id: uuid::Uuid::new_v4().simple().to_string(),
        expected_revision: 1,
        operation_id: uuid::Uuid::new_v4().to_string(),
        approval_sha256: "a".repeat(64),
    };
    let (held, ready) = hold_workload_journal(store.clone());
    ready.await.unwrap();
    let started = Instant::now();
    let result = issue(
        &store,
        &command,
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let error = match result {
        Ok(_) => panic!("Insufficient completion quota must refuse before issuance"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("reporting capacity unavailable"));
    assert!(
        store.require_workload_reporting().is_ok(),
        "Quota refusal is not a failed checkpoint"
    );
    drop(held);
    assert!(store.list().unwrap().is_empty());
}

struct HeldWorkloadJournal {
    release: Option<std::sync::mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for HeldWorkloadJournal {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .expect("Synthetic workload lock holder failed");
        }
    }
}
fn hold_workload_journal(
    store: Arc<JobStore>,
) -> (HeldWorkloadJournal, tokio::sync::oneshot::Receiver<()>) {
    let (ready, seen) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        store.with_test_journal_lock(|_| {
            let _ = ready.send(());
            let _ = held.recv_timeout(Duration::from_secs(15));
        });
    });
    (
        HeldWorkloadJournal {
            release: Some(release),
            thread: Some(thread),
        },
        seen,
    )
}
fn unused_issuer(engine: &TaskEngine) -> (Plan, Issued) {
    let (plan, _) = approved(engine);
    let issued = issue(
        &engine.store,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    (plan, issued)
}

#[tokio::test]
async fn issuer_retirement_keeps_the_runtime_responsive_while_its_journal_is_held() {
    let engine = TaskEngine::new(EventBus::new(20));
    let store = engine.store.clone();
    let (plan, mut issued) = unused_issuer(&engine);
    let before = store.get(&plan.plan_id).unwrap();
    let ack = synthetic_ack(issued.definition());
    let available = store.workload_finalization_available();
    let (held, ready) = hold_workload_journal(store.clone());
    ready.await.unwrap();
    let retiring = Instant::now();
    issued.retire();
    assert!(retiring.elapsed() < Duration::from_secs(1));
    assert!(issued.retired);
    assert!(lease(&mut issued, 1).await.is_err());
    assert!(issued.acknowledge(ack).is_err());
    assert_eq!(store.workload_finalization_available(), available);
    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::time::sleep(Duration::from_millis(1)),
    )
    .await
    .unwrap();
    drop(issued);
    drop(held);
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert_eq!(
        saved
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .state,
        AttemptState::Unknown
    );
    assert_eq!(
        saved.revision,
        before.revision + 1,
        "Only one retirement checkpoint may be attempted"
    );
    assert_eq!(store.workload_finalization_available(), available + 1);
}

#[tokio::test]
async fn reporting_slot_precedes_issuance_and_remains_owned_through_the_actual_save() {
    let mut store = JobStore::in_memory().unwrap();
    Arc::get_mut(&mut store)
        .unwrap()
        .test_workload_finalization_limit(1);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (first, mut issued) = unused_issuer(&engine);
    let (second, _) = approved(&engine);
    let command = execute(&second);
    assert_eq!(store.workload_finalization_available(), 0);
    assert!(
        issue(
            &store,
            &command,
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    assert_eq!(json!(store.get(&second.plan_id).unwrap()), json!(second));
    let (held, ready) = hold_workload_journal(store.clone());
    ready.await.unwrap();
    issued.retire();
    let start = Instant::now();
    assert!(
        issue(
            &store,
            &command,
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "Full reporting capacity must refuse before touching the held journal"
    );
    assert_eq!(store.workload_finalization_available(), 0);
    drop(held);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(
        store.get(&first.plan_id).unwrap().status,
        PlanStatus::Interrupted
    );
    assert_eq!(json!(store.get(&second.plan_id).unwrap()), json!(second));
    assert_eq!(store.workload_finalization_available(), 1);
    let next = issue(
        &store,
        &command,
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    drop(next);
    store.wait_workload_finalizations().unwrap();
}

#[tokio::test]
async fn failed_retirement_checkpoint_denies_new_authority_without_retry_or_evidence_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("checkpoint-failure.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, mut issued) = unused_issuer(&engine);
    let (next, _) = approved(&engine);
    let before = json!(store.get(&plan.plan_id).unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_uncertainty BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.status')='interrupted' BEGIN SELECT RAISE(ABORT,'synthetic uncertainty checkpoint failure'); END;").unwrap();
    issued.retire();
    store.wait_workload_finalizations().unwrap();
    assert!(issued.retired);
    assert!(lease(&mut issued, 1).await.is_err());
    assert!(store.require_workload_reporting().is_err());
    assert_eq!(json!(store.get(&plan.plan_id).unwrap()), before);
    db.execute_batch("DROP TRIGGER refuse_uncertainty").unwrap();
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(
        json!(store.get(&plan.plan_id).unwrap()),
        before,
        "Dropping the token cannot retry a failed checkpoint"
    );
    assert!(
        issue(
            &store,
            &execute(&next),
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    assert_eq!(json!(store.get(&next.plan_id).unwrap()), json!(next));
    assert!(
        store.require_workload_reporting().is_err(),
        "Removing a fault does not revive authority on the failed handle"
    );
}

#[test]
fn runtime_drop_does_not_join_retirement_or_release_a_still_owned_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("owned-journal.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, mut issued) = unused_issuer(&engine);
    let retired = store.retirement();
    let (held, ready) = hold_workload_journal(store.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        ready.await.unwrap();
        issued.retire();
    });
    drop(issued);
    let stop = Instant::now();
    drop(runtime);
    assert!(stop.elapsed() < Duration::from_secs(1));
    drop(engine);
    drop(store);
    assert!(JobStore::open(&path, 20, 8 * 1024 * 1024).is_err());
    drop(held);
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    observer
        .block_on(retired.wait(Duration::from_secs(2)))
        .unwrap();
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    assert_eq!(
        reopened.get(&plan.plan_id).unwrap().status,
        PlanStatus::Interrupted
    );
    assert!(
        issue(
            &reopened,
            &execute(&plan),
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
}

#[tokio::test]
async fn successful_completion_releases_its_slot_without_recreating_execution_authority() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut issued, receipt) = completed_fixture(&dir.path().join("completed.db")).await;
    let available = store.workload_finalization_available();
    let done = finish(&mut issued, receipt.clone()).unwrap();
    assert_eq!(store.workload_finalization_available(), available + 1);
    assert_eq!(json!(finish(&mut issued, receipt).unwrap()), json!(done));
    assert!(lease(&mut issued, 2).await.is_err());
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(json!(store.get(&done.plan_id).unwrap()), json!(done));
    assert!(store.require_workload_reporting().is_ok());
}
