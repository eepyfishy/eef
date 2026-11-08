// Actual SQLite write contention with synthetic already-issued workload metadata.
// No node, model, interpreter, media source or OS policy is used.
async fn busy_ack_fixture(
    path: &std::path::Path,
) -> (Arc<TaskEngine>, Issued, rusqlite::Connection) {
    let mut store = new_store(path, 8 * 1024 * 1024);
    Arc::get_mut(&mut store).unwrap().test_workload_ack_limit(1);
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let (_, mut issued) = unused_issuer(&engine).await;
    lease(&mut issued, 1).await.unwrap();
    let external = rusqlite::Connection::open(path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    (engine, issued, external)
}
async fn ack_entered(store: &JobStore) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.workload_ack_started() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("ACK writer never entered its guarded journal edit");
}
async fn ack_released(store: &JobStore) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.workload_ack_available() != 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Actual ACK checkpoint still owns its slot");
}

#[tokio::test]
async fn cancelled_ack_keeps_actual_write_capacity_and_retires_the_retained_issuer() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut issued, external) = busy_ack_fixture(&dir.path().join("cancel.db")).await;
    let store = engine.store.clone();
    let d = issued.definition().clone();
    let ack = synthetic_ack(&d);
    let before = store.get(&d.job_id).unwrap();
    let mut saving = Box::pin(issued.acknowledge(ack.clone(), probe_deadline()));
    tokio::select! {
        biased;
        result = &mut saving => panic!("Contended checkpoint unexpectedly returned: {result:?}"),
        _ = ack_entered(&store) => {}
    }
    assert_eq!(store.workload_ack_available(), 0);
    let raw: String = external
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&d.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        json!(serde_json::from_str::<Plan>(&raw).unwrap()),
        json!(before),
        "Renewal ACK is not durable while SQLite write is blocked"
    );
    let start = Instant::now();
    let full = store
        .checkpoint_workload_ack_before(d.clone(), ack, probe_deadline())
        .await
        .unwrap_err();
    assert!(full.to_string().contains("capacity unavailable"));
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(saving);
    assert!(issued.retired && !issued.acknowledged);
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(
        store.workload_ack_available(),
        0,
        "Caller cancellation cannot free an active writer"
    );
    external.execute_batch("ROLLBACK").unwrap();
    ack_released(&store).await;
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert!((before.revision + 1..=before.revision + 2).contains(&saved.revision));
    assert!(store.require_workload_reporting().is_ok());
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(store.get(&d.job_id).unwrap().revision, saved.revision);
}

#[tokio::test]
async fn ack_deadline_closes_authority_while_an_actual_sqlite_write_can_finish_later() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, mut issued, external) = busy_ack_fixture(&dir.path().join("deadline.db")).await;
    let store = engine.store.clone();
    let d = issued.definition().clone();
    let end = tokio::time::Instant::now() + Duration::from_millis(500);
    let mut saving = Box::pin(issued.acknowledge(synthetic_ack(&d), end));
    tokio::select! {
        biased;
        result = &mut saving => panic!("ACK must enter SQLite before observation expires: {result:?}"),
        _ = ack_entered(&store) => {}
    }
    assert!(
        tokio::time::timeout(Duration::from_secs(2), saving)
            .await
            .expect("SQLite write blocked the runtime deadline")
            .is_err()
    );
    assert!(issued.retired && !issued.acknowledged);
    assert_eq!(store.workload_ack_available(), 0);
    assert!(lease(&mut issued, 2).await.is_err());
    external.execute_batch("ROLLBACK").unwrap();
    ack_released(&store).await;
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert!(
        issued
            .acknowledge(synthetic_ack(&d), probe_deadline())
            .await
            .is_err()
    );
    assert_eq!(store.get(&d.job_id).unwrap().revision, saved.revision);
}

#[tokio::test]
async fn failed_ack_checkpoint_cannot_activate_renewals_or_retry_after_fault_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failure.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (_, mut issued) = unused_issuer(&engine).await;
    lease(&mut issued, 1).await.unwrap();
    let d = issued.definition().clone();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("CREATE TRIGGER refuse_workload_ack BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.execution.workload.state')='accepted' BEGIN SELECT RAISE(ABORT,'synthetic ACK checkpoint failure'); END;").unwrap();
    assert!(
        issued
            .acknowledge(synthetic_ack(&d), probe_deadline())
            .await
            .is_err()
    );
    assert!(issued.retired && !issued.acknowledged);
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert!(
        saved
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .acknowledgement
            .is_none()
    );
    external
        .execute_batch("DROP TRIGGER refuse_workload_ack")
        .unwrap();
    assert!(
        issued
            .acknowledge(synthetic_ack(&d), probe_deadline())
            .await
            .is_err()
    );
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(store.get(&d.job_id).unwrap().revision, saved.revision);
    assert!(
        store.require_workload_reporting().is_ok(),
        "Saved uncertainty does not claim global corruption"
    );
}

#[test]
fn runtime_drop_does_not_join_an_ack_writer_or_release_its_owned_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (engine, mut issued, external) = runtime.block_on(busy_ack_fixture(&path));
    let store = engine.store.clone();
    let retired = store.retirement();
    let d = issued.definition().clone();
    runtime.block_on(async {
        let mut saving = Box::pin(issued.acknowledge(synthetic_ack(&d), probe_deadline()));
        tokio::select! {
            biased;
            result = &mut saving => panic!("Contended checkpoint unexpectedly returned: {result:?}"),
            _ = ack_entered(&store) => {}
        }
        drop(saving);
    });
    drop(issued);
    drop(engine);
    drop(store);
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(JobStore::open(&path, 20, 8 * 1024 * 1024).is_err());
    external.execute_batch("ROLLBACK").unwrap();
    drop(external);
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    observer
        .block_on(retired.wait(Duration::from_secs(3)))
        .unwrap();
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    assert_eq!(
        reopened.get(&d.job_id).unwrap().status,
        PlanStatus::Interrupted
    );
}
