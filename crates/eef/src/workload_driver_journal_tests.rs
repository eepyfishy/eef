// Real original issuer/journal mechanics, synthetic bounded event/receipt data.
fn observed_workload_event() -> eefn::workload_network::Event {
    eefn::workload_network::Event {
        schema_version: 2,
        kind: eefn::workload_network::EventKind::MicrophoneActivity,
        sequence: 1,
        source_sequence: 1,
        source_frames: 80,
        elapsed_ms: 0,
        active: Some(true),
        level: Some(1000),
        discontinuity: false,
    }
}
async fn accepted_event_fixture() -> (Arc<TaskEngine>, Plan, Issued) {
    let (engine, plan, mut issued) = owned_stop_fixture().await;
    lease(&mut issued, 1).await.unwrap();
    issued
        .acknowledge(synthetic_ack(issued.definition()), stop_deadline())
        .await
        .unwrap();
    (engine, plan, issued)
}
#[tokio::test]
async fn workload_event_history_is_bounded_immutable_and_completion_confirms_exact_prefix() {
    let (engine, plan, mut issued) = accepted_event_fixture().await;
    let d = issued.definition().clone();
    let event = observed_workload_event();
    engine
        .store
        .queue_workload_event(d.clone(), event.clone(), stop_deadline())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.execution.as_ref().unwrap().schema_version, 5);
    let view = crate::jobs::view(&saved, true);
    assert_eq!(view["workload"]["event_count"], 1);
    assert_eq!(view["workload"]["events"][0], json!(event));
    assert_eq!(
        saved
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .observations,
        [event.clone()]
    );
    assert!(
        engine
            .store
            .queue_workload_event(d.clone(), event.clone(), stop_deadline())
            .unwrap()
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(saved)
    );
    assert!(
        engine
            .store
            .change(
                &plan.plan_id,
                "Cannot erase authenticated event history",
                |p| {
                    p.execution
                        .as_mut()
                        .unwrap()
                        .workload
                        .as_mut()
                        .unwrap()
                        .observations
                        .clear();
                    Ok(())
                }
            )
            .is_err()
    );
    let ack = synthetic_ack(&d);
    let mut receipt = completed_receipt(&d, &ack, 1);
    assert!(
        finish(&mut issued, receipt.clone()).await.is_err(),
        "Wrong prefix cannot complete"
    );
    let summary = eefn::workload_network::summarize_events(&d, &[event]).unwrap();
    let report = receipt.completion.as_mut().unwrap();
    report.event_count = summary.count;
    report.event_bytes = summary.bytes;
    report.event_prefix_sha256 = summary.prefix_sha256.clone();
    assert!(finish(&mut issued, receipt.clone()).await.is_err(),
        "A legacy record prefix cannot stand in for payload evidence");
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(saved));
    let report = receipt.completion.as_mut().unwrap();
    report.schema_version = 2;
    report.event_payload_prefix_sha256 = Some("a".repeat(64));
    assert!(finish(&mut issued, receipt.clone()).await.is_err(),
        "A validly shaped but wrong payload checksum cannot complete");
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(saved));
    let report = receipt.completion.as_mut().unwrap();
    report.event_prefix_sha256 = "f".repeat(64);
    report.event_payload_prefix_sha256 = Some(summary.prefix_sha256);
    let completed = finish(&mut issued, receipt).await.unwrap();
    assert_eq!(completed.status, PlanStatus::Completed);
    assert_eq!(completed.execution.as_ref().unwrap().schema_version, 5);
    assert!(lease(&mut issued, 2).await.is_err());
}
#[tokio::test]
async fn cancelled_event_metadata_waiter_keeps_actual_storage_capacity_without_appending_late() {
    let (engine, plan, issued) = accepted_event_fixture().await;
    let d = issued.definition().clone();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    let capacity = engine.store.workload_ack_available();
    let (held, ready) = hold_workload_journal(engine.store.clone());
    ready.await.unwrap();
    let receiver = engine
        .store
        .queue_workload_event(d, observed_workload_event(), stop_deadline())
        .unwrap();
    assert_eq!(engine.store.workload_ack_available(), capacity - 1);
    drop(receiver);
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
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}
#[tokio::test]
async fn stopping_event_session_preserves_its_prefix_and_schema_without_turning_intent_into_completion()
 {
    let (engine, plan, issued) = accepted_event_fixture().await;
    let d = issued.definition().clone();
    let event = observed_workload_event();
    engine
        .store
        .queue_workload_event(d.clone(), event.clone(), stop_deadline())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let mut cancelled = issued.stop_receiver().unwrap();
    assert!(!*cancelled.borrow());
    engine.cancel(&plan.plan_id).unwrap();
    cancelled.changed().await.unwrap();
    assert!(*cancelled.borrow());
    assert!(begin_workload_stop(&engine.store, &d).await.unwrap());
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    let e = saved.execution.unwrap();
    assert_eq!(e.schema_version, 5);
    let w = e.workload.unwrap();
    assert_eq!(w.observations, [event]);
    assert!(w.stop.unwrap().outcome_unknown && w.completion.is_none());
}
