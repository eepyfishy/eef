// Synthetic authenticated-node reports and real journals; no physical source.
async fn completion_entered(store: &JobStore) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.workload_completion_started() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Completion writer never entered its guarded edit");
}

#[tokio::test]
async fn cancelled_completion_keeps_its_reservation_and_saves_only_historical_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancel-completion.db");
    let (store, mut issued, receipt) = completed_fixture(&path).await;
    let d = issued.definition().clone();
    let before = store.get(&d.job_id).unwrap();
    let available = store.workload_finalization_available();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut saving = Box::pin(issued.complete(
        receipt.clone(),
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut saving => panic!("Held completion unexpectedly returned: {result:?}"),
        _ = completion_entered(&store) => {}
    }
    let start = Instant::now();
    drop(saving);
    assert!(issued.retired && issued.final_slot.is_none());
    assert!(lease(&mut issued, 2).await.is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(store.workload_finalization_available(), available);
    let raw: String = external
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&d.job_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        json!(serde_json::from_str::<Plan>(&raw).unwrap()),
        json!(before)
    );
    external.execute_batch("ROLLBACK").unwrap();
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Completed);
    assert_eq!(saved.revision, before.revision + 1);
    assert_eq!(store.workload_finalization_available(), available + 1);
    // Exact history stays readable after the old live deadline/clock correlation
    // ceases to be usable. It never reconstructs that original issuer.
    issued.deadline = Instant::now();
    issued.issued_wall = 1;
    assert_eq!(
        json!(finish(&mut issued, receipt).await.unwrap()),
        json!(saved)
    );
    assert!(lease(&mut issued, 2).await.is_err());
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(store.get(&d.job_id).unwrap().revision, saved.revision);
    assert!(store.require_workload_reporting().is_ok());
}

#[tokio::test]
async fn completion_observation_deadline_never_reopens_the_closed_issuer_or_discards_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deadline-completion.db");
    let (store, mut issued, receipt) = completed_fixture(&path).await;
    let d = issued.definition().clone();
    let available = store.workload_finalization_available();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    let end = tokio::time::Instant::now() + Duration::from_millis(500);
    let mut saving = Box::pin(issued.complete(
        receipt.clone(),
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
        end,
    ));
    tokio::select! {
        biased;
        result = &mut saving => panic!("Completion must enter SQLite before deadline: {result:?}"),
        _ = completion_entered(&store) => {}
    }
    assert!(
        tokio::time::timeout(Duration::from_secs(2), saving)
            .await
            .expect("Completion blocked runtime deadline")
            .is_err()
    );
    assert!(issued.retired);
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(store.workload_finalization_available(), available);
    external.execute_batch("ROLLBACK").unwrap();
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Completed);
    assert_eq!(
        json!(finish(&mut issued, receipt).await.unwrap()),
        json!(saved)
    );
    assert_eq!(store.workload_finalization_available(), available + 1);
}

#[tokio::test]
async fn failed_completion_and_uncertainty_checkpoint_deny_new_authority_without_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("double-failure.db");
    let (store, mut issued, receipt) = completed_fixture(&path).await;
    let d = issued.definition().clone();
    let before = store.get(&d.job_id).unwrap();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("CREATE TRIGGER refuse_final_history BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.status') IN ('completed','interrupted') BEGIN SELECT RAISE(ABORT,'synthetic final history write failure'); END;").unwrap();
    assert!(finish(&mut issued, receipt.clone()).await.is_err());
    store.wait_workload_finalizations().unwrap();
    assert!(issued.retired);
    assert!(store.require_workload_reporting().is_err());
    assert_eq!(json!(store.get(&d.job_id).unwrap()), json!(before));
    external
        .execute_batch("DROP TRIGGER refuse_final_history")
        .unwrap();
    assert!(finish(&mut issued, receipt).await.is_err());
    assert!(lease(&mut issued, 2).await.is_err());
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(json!(store.get(&d.job_id).unwrap()), json!(before));
    assert!(store.require_workload_reporting().is_err());
    assert!(
        issue(
            &store,
            &execute(&before),
            &uuid::Uuid::new_v4().to_string(),
            &policy(),
            tokio::time::Instant::now() + std::time::Duration::from_secs(2)
        )
        .await
        .is_err()
    );
}

#[test]
fn runtime_drop_preserves_completion_writer_ownership_until_the_actual_save_ends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime-completion.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (store, mut issued, receipt) = runtime.block_on(completed_fixture(&path));
    let d = issued.definition().clone();
    let retired = store.retirement();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    runtime.block_on(async {
        let mut saving = Box::pin(issued.complete(
            receipt,
            &d.issuer_id,
            &d.session_id,
            &d.executor_node,
            probe_deadline(),
        ));
        tokio::select! {
            biased;
            result = &mut saving => panic!("Held completion unexpectedly returned: {result:?}"),
            _ = completion_entered(&store) => {}
        }
        drop(saving);
    });
    drop(issued);
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
    let saved = reopened.get(&d.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Completed);
    assert!(
        observer
            .block_on(issue(
                &reopened,
                &execute(&saved),
                &uuid::Uuid::new_v4().to_string(),
                &policy(),
                tokio::time::Instant::now() + std::time::Duration::from_secs(2)
            ))
            .is_err()
    );
}
