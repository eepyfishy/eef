// Synthetic coordinator issuers and actual journals; no source or node runs.
fn probe_deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(2)
}

#[tokio::test]
async fn cancelling_a_polled_probe_retires_the_kept_issuer_before_storage_unlocks() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, mut issued) = unused_issuer(&engine);
    let d = issued.definition().clone();
    let q = query(&issued, 1);
    let before = engine.store.get(&plan.plan_id).unwrap().revision;
    // Constructing an unpolled future does not start a probe or change authority.
    drop(issued.lease(
        &q,
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
        probe_deadline(),
    ));
    assert!(!issued.retired);
    let (held, ready) = hold_workload_journal(engine.store.clone());
    ready.await.unwrap();
    let mut probe = Box::pin(issued.lease(
        &q,
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut probe => panic!("Held journal unexpectedly returned: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
    }
    let start = Instant::now();
    drop(probe);
    assert!(
        issued.retired,
        "Cancelling a borrowed probe must close the retained token"
    );
    assert!(lease(&mut issued, 1).await.is_err());
    assert!(issued.acknowledge(synthetic_ack(&d)).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    let available = engine.store.workload_finalization_available();
    drop(held);
    engine.store.wait_workload_finalizations().unwrap();
    let saved = engine.store.get(&plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    assert_eq!(saved.revision, before + 1);
    assert_eq!(
        engine.store.workload_finalization_available(),
        available + 1
    );
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
    assert_eq!(
        engine.store.get(&plan.plan_id).unwrap().revision,
        saved.revision
    );
}

#[tokio::test]
async fn probe_wait_obeys_both_caller_and_original_issuer_deadlines() {
    for original_issuer_expires in [false, true] {
        let engine = TaskEngine::new(EventBus::new(20));
        let (plan, mut issued) = unused_issuer(&engine);
        let d = issued.definition().clone();
        let q = query(&issued, 1);
        let before = engine.store.get(&plan.plan_id).unwrap().revision;
        let end = tokio::time::Instant::now() + Duration::from_millis(30);
        let caller_end = if original_issuer_expires {
            issued.deadline = end.into_std();
            probe_deadline()
        } else {
            end
        };
        let (held, ready) = hold_workload_journal(engine.store.clone());
        ready.await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            issued.lease(
                &q,
                &d.issuer_id,
                &d.session_id,
                &d.executor_node,
                caller_end,
            ),
        )
        .await
        .expect("Probe journal wait blocked runtime timeout");
        assert!(
            result.is_err(),
            "Expired probe cannot return a usable lease"
        );
        assert!(issued.retired);
        drop(held);
        engine.store.wait_workload_finalizations().unwrap();
        let saved = engine.store.get(&plan.plan_id).unwrap();
        assert_eq!(saved.status, PlanStatus::Interrupted);
        assert_eq!(saved.revision, before + 1);
        assert!(lease(&mut issued, 1).await.is_err());
    }
}

#[tokio::test]
async fn delayed_probe_computes_remaining_budget_after_the_actual_journal_read() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (_, mut issued) = unused_issuer(&engine);
    let original_end = issued.deadline;
    let d = issued.definition().clone();
    let q = query(&issued, 1);
    let (held, ready) = hold_workload_journal(engine.store.clone());
    ready.await.unwrap();
    let mut probe = Box::pin(issued.lease(
        &q,
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
        probe_deadline(),
    ));
    tokio::select! {
        biased;
        result = &mut probe => panic!("Held journal unexpectedly returned: {result:?}"),
        _ = tokio::time::sleep(Duration::from_millis(30)) => {}
    }
    let budget_before_unlock = original_end
        .saturating_duration_since(Instant::now())
        .as_millis() as u64;
    drop(held);
    let reply = probe.await.unwrap();
    assert!(
        reply.session_remaining_ms <= budget_before_unlock,
        "A reply prepared before the journal wait would extend the original budget"
    );
    assert_eq!(reply.sequence, 1);
    assert!(reply.lease_remaining_ms <= d.limits.max_lease_ms);
    assert!(!issued.retired);
    assert!(
        lease(&mut issued, 2).await.is_err(),
        "Durable ACK still precedes renewals"
    );
    issued.acknowledge(synthetic_ack(&d)).unwrap();
    assert_eq!(lease(&mut issued, 2).await.unwrap().sequence, 2);
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}

#[tokio::test]
async fn invalid_peer_or_probe_cannot_consume_sequence_or_change_durable_history() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, mut issued) = unused_issuer(&engine);
    let d = issued.definition().clone();
    let q = query(&issued, 1);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    for (issuer, session, node) in [
        (
            "b".repeat(64),
            d.session_id.clone(),
            d.executor_node.clone(),
        ),
        (
            d.issuer_id.clone(),
            uuid::Uuid::new_v4().to_string(),
            d.executor_node.clone(),
        ),
        (
            d.issuer_id.clone(),
            d.session_id.clone(),
            "other-node".into(),
        ),
    ] {
        assert!(
            issued
                .lease(&q, &issuer, &session, &node, probe_deadline())
                .await
                .is_err()
        );
        assert!(!issued.retired);
        assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
    }
    let mut changed = q.clone();
    changed.contract.max_lease_ms -= 1;
    assert!(
        issued
            .lease(
                &changed,
                &d.issuer_id,
                &d.session_id,
                &d.executor_node,
                probe_deadline()
            )
            .await
            .is_err()
    );
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(lease(&mut issued, 1).await.unwrap().sequence, 1);
    assert!(lease(&mut issued, 1).await.is_err());
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
    issued.acknowledge(synthetic_ack(&d)).unwrap();
    assert_eq!(lease(&mut issued, 2).await.unwrap().sequence, 2);
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(lease(&mut issued, 3).await.unwrap().sequence, 3);
    assert!(!issued.retired);
    drop(issued);
    engine.store.wait_workload_finalizations().unwrap();
}
