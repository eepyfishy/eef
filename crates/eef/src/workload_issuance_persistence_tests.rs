// Real SQLite contention and metadata only. No grant offer, node connection,
// model, interpreter, source process or machine policy change is involved.
async fn issuance_request(
    store: Arc<JobStore>,
    command: ExecuteCommand,
    deadline: tokio::time::Instant,
) -> Result<Issued> {
    issue(
        &store,
        &command,
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
        deadline,
    )
    .await
}
fn busy_issuance_fixture(path: &std::path::Path) -> (Arc<TaskEngine>, Plan, rusqlite::Connection) {
    busy_issuance_fixture_with_session(path, false)
}
fn busy_issuance_fixture_with_session(
    path: &std::path::Path,
    short_session: bool,
) -> (Arc<TaskEngine>, Plan, rusqlite::Connection) {
    let mut store = new_store(path, 8 * 1024 * 1024);
    Arc::get_mut(&mut store).unwrap().test_workload_ack_limit(1);
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let plan = if short_session {
        let mut input = template_input();
        input.params["workload"]["limits"]["max_session_ms"] = json!(1000);
        input.params["workload"]["limits"]["max_lease_ms"] = json!(500);
        let (review, input) = review_input(&engine, input);
        let plan = compile(
            &engine,
            &PlanningCommand::Compile {
                id: review.plan_id,
                expected_revision: review.revision,
                operation_id: uuid::Uuid::new_v4().to_string(),
                input,
            },
        )
        .unwrap();
        approve(
            &engine,
            &ApprovalCommand {
                id: plan.plan_id.clone(),
                expected_revision: plan.revision,
                operation_id: uuid::Uuid::new_v4().to_string(),
                plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
                valid_for_ms: 120000,
            },
            &"a".repeat(64),
        )
        .unwrap()
    } else {
        approved(&engine).0
    };
    let external = rusqlite::Connection::open(path).unwrap();
    external.execute_batch("BEGIN IMMEDIATE").unwrap();
    (engine, plan, external)
}
async fn issuance_entered(store: &JobStore) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.workload_issuance_started() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Issuance writer never entered its guarded journal edit");
}
fn assert_unknown_issuance(store: &JobStore, before: &Plan) -> Plan {
    // A late native commit can enqueue uncertainty only after its waiter is
    // gone. Drain actual issuance BEFORE observing finalization completion.
    store.wait_workload_issuance().unwrap();
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&before.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert_eq!(saved.revision, before.revision + 2);
    let w = saved.execution.as_ref().unwrap().workload.as_ref().unwrap();
    assert_eq!(w.state, AttemptState::Unknown);
    assert!(w.acknowledgement.is_none() && w.completion.is_none());
    assert_eq!(saved.tasks[0].assignments.len(), 1);
    assert_eq!(saved.tasks[0].attempts, 1);
    assert_eq!(store.workload_ack_available(), 1);
    assert!(store.require_workload_reporting().is_ok());
    saved
}

#[tokio::test]
async fn cancelled_issuance_read_has_no_issued_row_or_failed_reporting_latch() {
    let engine = TaskEngine::new(EventBus::new(20));
    let store = engine.store.clone();
    let (plan, _) = approved(&engine);
    let capacity = store.workload_finalization_available();
    let (held, ready) = hold_workload_journal(store.clone());
    ready.await.unwrap();
    let mut pending = Box::pin(issuance_request(
        store.clone(),
        execute(&plan),
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held initial read returned: {}", result.is_ok()),
        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    assert_eq!(store.workload_finalization_available(), capacity - 1);
    drop(pending);
    assert_eq!(store.workload_finalization_available(), capacity);
    assert_eq!(store.workload_issuance_started(), 0);
    assert!(store.require_workload_reporting().is_ok());
    drop(held);
    assert_eq!(json!(store.get(&plan.plan_id).unwrap()), json!(plan));
}

#[tokio::test]
async fn cancelled_issuance_retains_native_capacity_and_retires_a_late_commit_once() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, plan, external) = busy_issuance_fixture(&dir.path().join("cancel-issue.db"));
    let store = engine.store.clone();
    let capacity = store.workload_finalization_available();
    let mut pending = Box::pin(issuance_request(
        store.clone(),
        execute(&plan),
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held write returned: {}", result.is_ok()),
        _ = issuance_entered(&store) => {}
    }
    let start = Instant::now();
    drop(pending);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(store.workload_ack_available(), 0);
    assert_eq!(store.workload_finalization_available(), capacity - 1);
    assert_eq!(
        store.workload_ack_started(),
        0,
        "Issuance cannot count as an ACK"
    );
    let raw: String = external
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&plan.plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        json!(serde_json::from_str::<Plan>(&raw).unwrap()),
        json!(plan)
    );
    external.execute_batch("ROLLBACK").unwrap();
    let saved = assert_unknown_issuance(&store, &plan);
    assert_eq!(store.workload_finalization_available(), capacity);
    assert_eq!(store.workload_issuance_started(), 1);
    assert_eq!(store.get(&plan.plan_id).unwrap().revision, saved.revision);
    assert!(
        issuance_request(store.clone(), execute(&saved), probe_deadline())
            .await
            .is_err()
    );
    assert_eq!(store.get(&plan.plan_id).unwrap().revision, saved.revision);
}

#[tokio::test]
async fn lost_delivered_checkpoint_and_cancelled_post_commit_read_both_retire_without_authority() {
    for post_commit_read in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (engine, plan, external) =
            busy_issuance_fixture(&dir.path().join("delivered-issue.db"));
        let store = engine.store.clone();
        let mut pending = Box::pin(issuance_request(
            store.clone(),
            execute(&plan),
            probe_deadline(),
        ));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Held write returned: {}", result.is_ok()),
            _ = issuance_entered(&store) => {}
        }
        external.execute_batch("ROLLBACK").unwrap();
        store.wait_workload_issuance().unwrap();
        let issued = store.get(&plan.plan_id).unwrap();
        assert_eq!(issued.status, PlanStatus::Running);
        assert_eq!(issued.revision, plan.revision + 1);
        // Keep the original future unpolled until the success is already in its
        // channel. Optionally poll once into the real blocked post-commit read.
        let held = if post_commit_read {
            let (held, ready) = hold_workload_journal(store.clone());
            ready.await.unwrap();
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            Some(held)
        } else {
            None
        };
        drop(pending);
        drop(held);
        let saved = assert_unknown_issuance(&store, &plan);
        assert_eq!(
            saved
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap()
                .definition,
            issued
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap()
                .definition
        );
    }
}

#[tokio::test]
async fn issuance_observation_and_original_session_deadlines_both_refuse_late_authority() {
    for short_session in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (engine, plan, external) = busy_issuance_fixture_with_session(
            &dir.path().join("deadline-issue.db"),
            short_session,
        );
        let store = engine.store.clone();
        let end = tokio::time::Instant::now()
            + Duration::from_millis(if short_session { 2000 } else { 500 });
        let mut pending = Box::pin(issuance_request(store.clone(), execute(&plan), end));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Write must enter before deadline: {}", result.is_ok()),
            _ = issuance_entered(&store) => {}
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(2), pending)
                .await
                .unwrap()
                .is_err()
        );
        assert_eq!(store.workload_ack_available(), 0);
        external.execute_batch("ROLLBACK").unwrap();
        let saved = assert_unknown_issuance(&store, &plan);
        if short_session {
            assert_eq!(
                saved
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .definition
                    .limits
                    .max_session_ms,
                1000
            );
        }
    }
}

#[tokio::test]
async fn definitely_uncommitted_issuance_failure_preserves_approval_without_automatic_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("refused-issue.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    let capacity = store.workload_finalization_available();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute_batch("CREATE TRIGGER refuse_issuance BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.execution.workload.state')='issued' BEGIN SELECT RAISE(ABORT,'synthetic issuance failure'); END;").unwrap();
    assert!(
        issuance_request(store.clone(), execute(&plan), probe_deadline())
            .await
            .is_err()
    );
    store.wait_workload_issuance().unwrap();
    store.wait_workload_finalizations().unwrap();
    assert_eq!(json!(store.get(&plan.plan_id).unwrap()), json!(plan));
    assert!(store.require_workload_reporting().is_ok());
    assert_eq!(store.workload_finalization_available(), capacity);
    external
        .execute_batch("DROP TRIGGER refuse_issuance")
        .unwrap();
    assert_eq!(store.workload_issuance_started(), 1);
    assert_eq!(json!(store.get(&plan.plan_id).unwrap()), json!(plan));
}

#[tokio::test]
async fn unreadable_uncertain_issuance_latches_denial_without_repairing_retained_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unreadable-issue.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    let external = rusqlite::Connection::open(&path).unwrap();
    // FAIL retains the trigger's earlier mutation, unlike ABORT. This models a
    // failed write whose row cannot safely be classified as unissued afterward.
    external.execute_batch("CREATE TRIGGER damage_issuance BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.execution.workload.state')='issued' BEGIN UPDATE durable_jobs SET plan_json='{}' WHERE job_id=OLD.job_id; SELECT RAISE(FAIL,'synthetic uncertain issuance failure'); END;").unwrap();
    assert!(
        issuance_request(store.clone(), execute(&plan), probe_deadline())
            .await
            .is_err()
    );
    store.wait_workload_issuance().unwrap();
    store.wait_workload_finalizations().unwrap();
    assert!(store.require_workload_reporting().is_err());
    assert!(store.get(&plan.plan_id).is_err());
    let raw: String = external
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&plan.plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, "{}");
    external
        .execute_batch("DROP TRIGGER damage_issuance")
        .unwrap();
    assert!(
        issuance_request(store.clone(), execute(&plan), probe_deadline())
            .await
            .is_err()
    );
    assert_eq!(store.workload_issuance_started(), 1);
    assert!(store.require_workload_reporting().is_err());
}

#[tokio::test]
async fn failed_orphan_uncertainty_denies_new_issuance_without_repeated_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("orphan-failure.db");
    let (engine, plan, external) = busy_issuance_fixture(&path);
    let store = engine.store.clone();
    // Install the fault before any issuance starts, then hold the real write.
    external.execute_batch("ROLLBACK; CREATE TRIGGER refuse_orphan BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.status')='interrupted' BEGIN SELECT RAISE(ABORT,'synthetic orphan checkpoint failure'); END; BEGIN IMMEDIATE;").unwrap();
    let mut pending = Box::pin(issuance_request(
        store.clone(),
        execute(&plan),
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held write returned: {}", result.is_ok()),
        _ = issuance_entered(&store) => {}
    }
    drop(pending);
    external.execute_batch("ROLLBACK").unwrap();
    store.wait_workload_issuance().unwrap();
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Running);
    assert_eq!(saved.revision, plan.revision + 1);
    assert!(store.require_workload_reporting().is_err());
    external
        .execute_batch("DROP TRIGGER refuse_orphan")
        .unwrap();
    assert!(
        issuance_request(store.clone(), execute(&saved), probe_deadline())
            .await
            .is_err()
    );
    store.wait_workload_finalizations().unwrap();
    assert_eq!(store.get(&plan.plan_id).unwrap().revision, saved.revision);
    assert_eq!(store.workload_issuance_started(), 1);
    assert!(store.require_workload_reporting().is_err());
}

#[test]
fn runtime_drop_keeps_actual_issuance_and_orphan_retirement_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime-issue.db");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (engine, plan, external) = busy_issuance_fixture(&path);
    let store = engine.store.clone();
    let retired = store.retirement();
    runtime.block_on(async {
        let mut pending = Box::pin(issuance_request(
            store.clone(),
            execute(&plan),
            probe_deadline(),
        ));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Held write returned: {}", result.is_ok()),
            _ = issuance_entered(&store) => {}
        }
        drop(pending);
    });
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
    let saved = reopened.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert_eq!(saved.revision, plan.revision + 2);
}
