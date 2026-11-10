// Included with real reviewed-workload helpers. These tests do not launch a
// source; a stop acknowledgement remains intent, never physical-stop proof.
async fn owned_stop_fixture() -> (Arc<TaskEngine>, Plan, Issued) {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
    let issued = issue_owned(
        &engine,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
        tokio::time::Instant::now() + Duration::from_secs(5),
    )
    .await
    .unwrap();
    (engine, plan, issued)
}
fn stop_deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(5)
}
fn workload_stop_report(d: &Definition) -> eefn::grants::stop::Report {
    let scope = d.scope().unwrap();
    eefn::grants::stop::Report {
        schema_version: 1,
        report_type: "grant_stop_receipt".into(),
        grant_id: scope.grant_id,
        grant_sha256: scope.grant_sha256,
        issuer_id: scope.issuer_id,
        session_id: scope.session_id,
        executor_node: scope.executor_node,
        job_id: scope.job_id,
        task_id: scope.task_id,
        attempt: scope.attempt,
        recorded_at_ms: eefn::protocol::now_ms().max(1),
        receipt_preexisted: true,
        reused: false,
        cancellation_signalled: true,
        persisted: true,
        stop_requested: true,
        execution_stopped: false,
        external_effects_stopped: false,
        execution_authorized: false,
        automatically_retried: false,
    }
}
async fn begin_workload_stop(store: &Arc<JobStore>, d: &Definition) -> Result<bool> {
    store
        .checkpoint_workload_stop_before(
            d.clone(),
            crate::jobs::WorkloadStopCheckpoint::Begin,
            stop_deadline(),
        )
        .await
}

#[tokio::test]
async fn stopping_original_issued_workload_persists_intent_and_retires_to_unknown_without_stop_proof()
 {
    let (engine, plan, mut issued) = owned_stop_fixture().await;
    let d = issued.definition().clone();
    let stopped = engine.cancel(&plan.plan_id).unwrap();
    assert_eq!(stopped.status, PlanStatus::Stopping);
    assert!(
        stopped
            .execution
            .as_ref()
            .unwrap()
            .stop_requested_at_ms
            .is_some()
    );
    assert!(
        stopped
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .stop
            .is_none()
    );
    assert!(
        issued
            .deliver(|| panic!("Stop cannot enqueue a lease"))
            .is_err()
    );
    assert!(lease(&mut issued, 1).await.is_err());
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    let execution = saved.execution.unwrap();
    assert_eq!(
        execution.workload.as_ref().unwrap().state,
        AttemptState::Unknown
    );
    assert_eq!(execution.workload.as_ref().unwrap().definition, d);
    assert_eq!(
        execution.stop_requested_at_ms,
        stopped.execution.unwrap().stop_requested_at_ms
    );
}

#[tokio::test]
async fn workload_stop_requires_explicit_intent_and_saves_only_one_exact_send_reservation() {
    let (engine, plan, issued) = owned_stop_fixture().await;
    let d = issued.definition().clone();
    let before = engine.store.get(&plan.plan_id).unwrap();
    let old = serde_json::to_value(&before).unwrap();
    assert!(
        old["execution"]["workload"].get("stop").is_none(),
        "Old schema-two serialization stays unchanged"
    );
    let decoded: Plan = serde_json::from_value(old.clone()).unwrap();
    validate_execution(&decoded).unwrap();
    assert!(begin_workload_stop(&engine.store, &d).await.is_err());
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), old);
    engine.cancel(&plan.plan_id).unwrap();
    assert!(begin_workload_stop(&engine.store, &d).await.unwrap());
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.execution.as_ref().unwrap().schema_version, 4);
    assert!(!begin_workload_stop(&engine.store, &d).await.unwrap());
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(saved)
    );
    let mut replacement = d.clone();
    replacement.session_id = uuid::Uuid::new_v4().to_string();
    assert!(
        begin_workload_stop(&engine.store, &replacement)
            .await
            .is_err()
    );
    assert!(
        engine
            .store
            .checkpoint_workload_stop_before(
                d,
                crate::jobs::WorkloadStopCheckpoint::Begin,
                tokio::time::Instant::now()
            )
            .await
            .is_err()
    );
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(saved)
    );
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}

#[tokio::test]
async fn workload_stop_receipt_cannot_be_forged_erased_retimed_or_replaced() {
    use crate::jobs::WorkloadStopCheckpoint;
    let (engine, plan, issued) = owned_stop_fixture().await;
    let d = issued.definition().clone();
    engine.cancel(&plan.plan_id).unwrap();
    assert!(begin_workload_stop(&engine.store, &d).await.unwrap());
    let receipt = workload_stop_report(&d);
    for mutation in 0..3 {
        let mut wrong = receipt.clone();
        match mutation {
            0 => wrong.execution_stopped = true,
            1 => wrong.external_effects_stopped = true,
            _ => wrong.execution_authorized = true,
        }
        assert!(
            engine
                .store
                .checkpoint_workload_stop_before(
                    d.clone(),
                    WorkloadStopCheckpoint::Receipt(wrong),
                    stop_deadline()
                )
                .await
                .is_err()
        );
    }
    let mut wrong = receipt.clone();
    wrong.session_id = uuid::Uuid::new_v4().to_string();
    assert!(
        engine
            .store
            .checkpoint_workload_stop_before(
                d.clone(),
                WorkloadStopCheckpoint::Receipt(wrong),
                stop_deadline()
            )
            .await
            .is_err()
    );
    assert!(
        engine
            .store
            .checkpoint_workload_stop_before(
                d.clone(),
                WorkloadStopCheckpoint::Receipt(receipt.clone()),
                stop_deadline()
            )
            .await
            .unwrap()
    );
    assert!(
        !engine
            .store
            .checkpoint_workload_stop_before(
                d.clone(),
                WorkloadStopCheckpoint::Receipt(receipt.clone()),
                stop_deadline()
            )
            .await
            .unwrap()
    );
    let saved = engine.store.get(&plan.plan_id).unwrap();
    for mutation in 0..4 {
        assert!(
            engine
                .store
                .change(
                    &plan.plan_id,
                    "Attempt invalid retained stop mutation",
                    |p| {
                        let e = p.execution.as_mut().unwrap();
                        match mutation {
                            0 => {
                                e.workload.as_mut().unwrap().stop = None;
                                e.schema_version = 2;
                            }
                            1 => {
                                e.workload
                                    .as_mut()
                                    .unwrap()
                                    .stop
                                    .as_mut()
                                    .unwrap()
                                    .send_attempted_at_ms += 1
                            }
                            2 => {
                                e.workload
                                    .as_mut()
                                    .unwrap()
                                    .stop
                                    .as_mut()
                                    .unwrap()
                                    .report
                                    .as_mut()
                                    .unwrap()
                                    .reused = true
                            }
                            _ => e.stop_requested_at_ms = None,
                        }
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(
            json!(engine.store.get(&plan.plan_id).unwrap()),
            json!(saved)
        );
    }
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    let stop = saved.execution.unwrap().workload.unwrap().stop.unwrap();
    assert!(!stop.outcome_unknown);
    assert_eq!(stop.report.unwrap(), receipt);
}

#[tokio::test]
async fn cancelled_workload_stop_checkpoint_holds_original_capacity_until_native_storage_exits() {
    let (engine, plan, issued) = owned_stop_fixture().await;
    let d = issued.definition().clone();
    engine.cancel(&plan.plan_id).unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    let capacity = engine.store.workload_ack_available();
    let entered = engine.store.workload_stop_entered();
    let (held, ready) = hold_workload_journal(engine.store.clone());
    ready.await.unwrap();
    let mut checkpoint = Box::pin(begin_workload_stop(&engine.store, &d));
    tokio::select! {
        biased;
        result=&mut checkpoint=>panic!("Held journal returned: {result:?}"),
        entered=tokio::time::timeout(Duration::from_secs(2), async {
            while engine.store.workload_stop_entered() == entered {
                tokio::task::yield_now().await;
            }
        })=>entered.expect("Stop worker did not enter its journal wait"),
    }
    assert_eq!(engine.store.workload_ack_available(), capacity - 1);
    let now = Instant::now();
    drop(checkpoint);
    assert!(now.elapsed() < Duration::from_secs(1));
    assert_eq!(engine.store.workload_ack_available(), capacity - 1);
    drop(held);
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.store.workload_ack_available() != capacity {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(saved)
    );
    assert!(engine.store.require_workload_reporting().is_ok());
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}

#[tokio::test]
async fn offline_workload_stop_is_reserved_once_without_retry_or_retargeting() {
    let (engine, plan, issued) = owned_stop_fixture().await;
    let d = issued.definition().clone();
    engine.cancel(&plan.plan_id).unwrap();
    let server = eefn::NodeServer::new("private-workload-stop-fixture", "127.0.0.1", 0).unwrap();
    let driver =
        crate::request_execution::Driver::peer_fixture(engine.clone(), server, Default::default());
    driver.stop_workload(&d, stop_deadline()).await.unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    let stop = saved
        .execution
        .as_ref()
        .unwrap()
        .workload
        .as_ref()
        .unwrap()
        .stop
        .as_ref()
        .unwrap();
    assert!(stop.outcome_unknown && stop.report.is_none());
    driver.stop_workload(&d, stop_deadline()).await.unwrap();
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(saved)
    );
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}

#[tokio::test]
async fn stop_retirement_preserves_accepted_workload_receipt_without_completing_or_renewing() {
    let (engine, plan, mut issued) = owned_stop_fixture().await;
    lease(&mut issued, 1).await.unwrap();
    let d = issued.definition().clone();
    let ack = synthetic_ack(&d);
    issued
        .acknowledge(ack.clone(), stop_deadline())
        .await
        .unwrap();
    engine.cancel(&plan.plan_id).unwrap();
    assert!(begin_workload_stop(&engine.store, &d).await.unwrap());
    assert!(
        finish(&mut issued, completed_receipt(&d, &ack, 1))
            .await
            .is_err()
    );
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    let workload = saved.execution.unwrap().workload.unwrap();
    assert_eq!(json!(workload.acknowledgement.unwrap()), json!(ack));
    assert!(workload.completion.is_none());
    assert!(workload.stop.unwrap().outcome_unknown);
}

#[tokio::test]
async fn reopening_workload_stop_history_retains_scope_intent_and_receipt_without_resume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workload-stop.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    let issued = issue_owned(
        &engine,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
        stop_deadline(),
    )
    .await
    .unwrap();
    let d = issued.definition().clone();
    engine.cancel(&plan.plan_id).unwrap();
    assert!(begin_workload_stop(&store, &d).await.unwrap());
    store
        .checkpoint_workload_stop_before(
            d.clone(),
            crate::jobs::WorkloadStopCheckpoint::Receipt(workload_stop_report(&d)),
            stop_deadline(),
        )
        .await
        .unwrap();
    drop(issued);
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&plan.plan_id).unwrap();
    let view = crate::jobs::view(&saved, true);
    assert_eq!(view["remote_effects_unknown"], true);
    assert_eq!(view["remote_stop_confirmed"], false);
    assert_eq!(view["workload"]["stop"]["outcome_unknown"], false);
    assert_eq!(
        view["workload"]["stop"]["report"]["execution_stopped"],
        false
    );
    assert_eq!(view["can_resume"], false);
    drop(engine);
    drop(store);
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    assert_eq!(json!(reopened.get(&plan.plan_id).unwrap()), json!(saved));
    assert!(
        issue(
            &reopened,
            &execute(&plan),
            &uuid::Uuid::new_v4().to_string(),
            &policy(),
            stop_deadline()
        )
        .await
        .is_err()
    );
}
