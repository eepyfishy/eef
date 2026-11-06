// Real engine registration and real job persistence; no source or network offer.
#[tokio::test]
async fn original_engine_owned_issuer_is_revoked_before_shutdown_waits_for_its_drop() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
    let mut issued = issue_owned(
        &engine,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
        tokio::time::Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert!(engine.claim_workload(&plan.plan_id).is_err());
    let sends = std::sync::atomic::AtomicUsize::new(0);
    issued
        .deliver(|| {
            sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    let q = query(&issued, 1);
    let mut stopping = Box::pin(engine.shutdown());
    tokio::select! {
        biased;
        _=&mut stopping=>panic!("Original issuer still owns its engine registration"),
        _=tokio::time::sleep(Duration::from_millis(20))=>{}
    }
    assert!(
        issued
            .deliver(|| {
                sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .is_err()
    );
    let d = issued.definition().clone();
    assert!(
        issued
            .lease(
                &q,
                &d.issuer_id,
                &d.session_id,
                &d.executor_node,
                tokio::time::Instant::now() + Duration::from_secs(1)
            )
            .await
            .is_err()
    );
    assert_eq!(sends.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(issued);
    tokio::time::timeout(Duration::from_secs(1), stopping)
        .await
        .unwrap();
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
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
    assert_eq!(saved.execution.unwrap().workload.unwrap().definition, d);
}

#[tokio::test]
async fn cancelled_owned_issuance_releases_pending_registration_without_waiting_for_storage() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
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
    let command = execute(&plan);
    let session = uuid::Uuid::new_v4().to_string();
    let policy = policy();
    let mut pending = Box::pin(issue_owned(
        &engine,
        &command,
        &session,
        &policy,
        tokio::time::Instant::now() + Duration::from_secs(5),
    ));
    tokio::select! {
        biased;
        result=&mut pending=>panic!("Blocked issuance returned: {}", result.is_ok()),
        _=tokio::time::sleep(Duration::from_millis(20))=>{}
    }
    assert!(engine.claim_workload(&plan.plan_id).is_err());
    let start = Instant::now();
    drop(pending);
    assert!(start.elapsed() < Duration::from_secs(1));
    tokio::time::timeout(Duration::from_secs(1), engine.shutdown())
        .await
        .unwrap();
    release.send(()).unwrap();
    blocker.join().unwrap();
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(plan));
}
