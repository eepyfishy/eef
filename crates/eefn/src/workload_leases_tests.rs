use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
fn contract() -> Contract {
    Contract {
        schema_version: 2,
        scope: crate::grants::stop::Scope {
            grant_id: uuid::Uuid::new_v4().to_string(),
            grant_sha256: "d".repeat(64),
            issuer_id: "a".repeat(64),
            session_id: uuid::Uuid::new_v4().to_string(),
            executor_node: "node".into(),
            job_id: "b".repeat(32),
            task_id: "c".repeat(32),
            attempt: 1,
        },
        max_session_ms: 60_000,
        max_lease_ms: 1000,
    }
}
fn policy() -> Policy {
    Policy {
        enabled: true,
        ..Default::default()
    }
}
fn until(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}
fn query(c: &Contract, sequence: u64) -> Query {
    Query {
        kind: "workload_lease_probe".into(),
        schema_version: 2,
        id: uuid::Uuid::new_v4().to_string(),
        contract: c.clone(),
        sequence,
    }
}
fn reply(q: &Query, lease: u64, session: u64) -> Reply {
    Reply {
        kind: "workload_lease".into(),
        schema_version: 2,
        id: q.id.clone(),
        contract_sha256: q.contract.digest().unwrap(),
        sequence: q.sequence,
        lease_remaining_ms: lease,
        session_remaining_ms: session,
    }
}
fn proof(c: &Contract, sequence: u64, lease: u64, session: u64) -> LeaseProof {
    let q = query(c, sequence);
    LeaseProof::confirm(&q, reply(&q, lease, session), Instant::now()).unwrap()
}

#[test]
fn session_contract_is_separate_from_v1_and_disabled_by_default() {
    let c = contract();
    c.validate().unwrap();
    assert!(c.permitted(&Policy::default()).is_err());
    c.permitted(&policy()).unwrap();
    assert_eq!(crate::grants::MAX_DURATION_MS, 30_000);
    let mut legacy = crate::grants::GrantPolicy::default();
    legacy.max_duration_ms = 60_000;
    assert!(legacy.validate().is_err());
    assert!(serde_json::from_value::<crate::grants::ExecutionGrant>(json!(c)).is_err());
    assert!(crate::execution_boundary::require_control("workload.session").is_err());
    assert!(!crate::engine::CAPABILITIES.contains(&"workload.session"));
    for (session, lease) in [
        (0, 1000),
        (MAX_SESSION_MS + 1, 1000),
        (60_000, 0),
        (60_000, MAX_LEASE_MS + 1),
        (100, 101),
    ] {
        let mut changed = c.clone();
        changed.max_session_ms = session;
        changed.max_lease_ms = lease;
        assert!(changed.validate().is_err());
    }
}
#[test]
fn challenge_rejects_changed_scope_nonce_sequence_bounds_and_expiry() {
    let c = contract();
    let q = query(&c, 1);
    let valid = reply(&q, 500, 30_000);
    for field in [
        "id",
        "contract_sha256",
        "sequence",
        "schema_version",
        "type",
        "lease_remaining_ms",
        "session_remaining_ms",
    ] {
        let mut value = json!(valid);
        value[field] = match field {
            "id" => json!(uuid::Uuid::new_v4().to_string()),
            "contract_sha256" => json!("f".repeat(64)),
            "type" => json!("grant_lease"),
            "sequence" => json!(2),
            "schema_version" => json!(1),
            "lease_remaining_ms" => json!(1001),
            _ => json!(60_001),
        };
        assert!(
            LeaseProof::confirm(&q, serde_json::from_value(value).unwrap(), Instant::now())
                .is_err(),
            "{field}"
        );
    }
    assert!(
        LeaseProof::confirm(&q, valid.clone(), Instant::now() - Duration::from_secs(3)).is_err()
    );
    assert!(
        LeaseProof::confirm(&q, valid.clone(), Instant::now() + Duration::from_secs(1)).is_err()
    );
    assert!(LeaseProof::confirm(&q, reply(&q, 500, 499), Instant::now()).is_err());
    assert!(LeaseProof::confirm(&q, reply(&q, 0, 500), Instant::now()).is_err());
    let mut value = json!(valid);
    value["media"] = json!("not a lease field");
    assert!(serde_json::from_value::<Reply>(value).is_err());
    let mut value = json!(q);
    value["payload"] = json!("not a probe field");
    assert!(serde_json::from_value::<Query>(value).is_err());
}
#[tokio::test]
async fn nonce_correlator_ignores_wrong_replies_and_consumes_each_nonce_once() {
    let c = contract();
    let requests = Arc::new(Requests::default());
    let mut previous = None;
    let manager = requests.clone();
    let first = requests
        .request(&c, 1, |value| {
            let q: Query = serde_json::from_value(value).unwrap();
            let correct = reply(&q, 900, 50_000);
            let mut wrong = correct.clone();
            wrong.contract_sha256 = "f".repeat(64);
            manager.complete(json!(wrong));
            assert_eq!(manager.pending.lock().unwrap().len(), 1);
            manager.complete(json!(correct));
            manager.complete(json!(correct));
            previous = Some(json!(correct));
            async { Ok(()) }
        })
        .await
        .unwrap();
    assert_eq!(first.sequence, 1);
    assert!(requests.pending.lock().unwrap().is_empty());
    let old = previous.unwrap();
    let manager = requests.clone();
    let second = requests
        .request(&c, 2, |value| {
            let q: Query = serde_json::from_value(value).unwrap();
            assert_ne!(q.id, old["id"].as_str().unwrap());
            manager.complete(old);
            assert_eq!(manager.pending.lock().unwrap().len(), 1);
            manager.complete(json!(reply(&q, 900, 49_000)));
            async { Ok(()) }
        })
        .await
        .unwrap();
    assert_eq!(second.sequence, 2);
    assert!(requests.pending.lock().unwrap().is_empty());
}
#[tokio::test]
async fn cancelled_challenge_retires_only_its_pending_nonce() {
    let c = contract();
    let requests = Arc::new(Requests::default());
    let (tx, rx) = oneshot::channel();
    {
        let request = requests.request(&c, 1, |value| {
            tx.send(value).unwrap();
            async { Ok(()) }
        });
        tokio::pin!(request);
        tokio::select! {q=rx=>assert!(q.unwrap()["id"].is_string()),_=&mut request=>panic!("No reply was supplied")};
        assert_eq!(requests.pending.lock().unwrap().len(), 1);
    }
    assert!(requests.pending.lock().unwrap().is_empty());
}
#[tokio::test]
async fn local_clock_change_during_challenge_cannot_create_a_proof() {
    let c = contract();
    let requests = Arc::new(Requests::default());
    let manager = requests.clone();
    let result = requests
        .request(&c, 1, |value| {
            let q: Query = serde_json::from_value(value).unwrap();
            manager
                .pending
                .lock()
                .unwrap()
                .get_mut(&q.id)
                .unwrap()
                .sent_wall -= 3000;
            manager.complete(json!(reply(&q, 900, 50_000)));
            async { Ok(()) }
        })
        .await;
    assert!(result.is_err());
    assert!(requests.pending.lock().unwrap().is_empty());
}
#[test]
fn renewal_keeps_original_session_end_and_rejects_replays_or_new_scope() {
    let c = contract();
    let mut window = Window::new(&c, &policy(), proof(&c, 1, 900, 40_000)).unwrap();
    let original = window.session_deadline;
    window.advance(proof(&c, 2, 1000, 60_000)).unwrap();
    assert_eq!(window.session_deadline, original);
    assert!(window.advance(proof(&c, 2, 1000, 60_000)).is_err());
    assert!(window.advance(proof(&c, 4, 1000, 60_000)).is_err());
    let mut changed = c.clone();
    changed.scope.session_id = uuid::Uuid::new_v4().to_string();
    assert!(window.advance(proof(&changed, 3, 1000, 60_000)).is_err());
    window.deadline = Instant::now();
    assert!(window.advance(proof(&c, 3, 1000, 60_000)).is_err());
}
struct DropCount(Arc<AtomicUsize>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn queued_renewals_drive_one_source_future_without_reopening_it() {
    let c = contract();
    let (sender, renewals) = renewal_channel();
    sender.try_send(proof(&c, 2, 1000, 60_000)).ok().unwrap();
    drop(sender);
    let starts = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let start = starts.clone();
    let guard = DropCount(drops.clone());
    let result = supervise(
        &c,
        &policy(),
        proof(&c, 1, 900, 50_000),
        renewals,
        async move {
            let _owned = guard;
            start.fetch_add(1, Ordering::SeqCst);
            Ok(7)
        },
    )
    .await
    .unwrap();
    assert_eq!(result, 7);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn disabled_policy_and_expired_initial_proof_never_poll_the_operation() {
    let c = contract();
    let starts = Arc::new(AtomicUsize::new(0));
    let start = starts.clone();
    let (_, renewals) = renewal_channel();
    assert!(
        supervise(
            &c,
            &Policy::default(),
            proof(&c, 1, 900, 50_000),
            renewals,
            async move {
                start.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        )
        .await
        .is_err()
    );
    let mut expired = proof(&c, 1, 900, 50_000);
    expired.deadline = Instant::now();
    let start = starts.clone();
    let (_, renewals) = renewal_channel();
    assert!(
        supervise(&c, &policy(), expired, renewals, async move {
            start.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
        .is_err()
    );
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn missing_renewal_expires_and_drops_owned_future_before_return() {
    let c = contract();
    let (sender, renewals) = renewal_channel();
    drop(sender);
    let drops = Arc::new(AtomicUsize::new(0));
    let guard = DropCount(drops.clone());
    let result = supervise(
        &c,
        &policy(),
        proof(&c, 1, 50, 50_000),
        renewals,
        async move {
            let _owned = guard;
            std::future::pending::<Result<()>>().await
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn operation_failure_is_not_retried_or_hidden_by_a_live_lease() {
    let c = contract();
    let (_sender, renewals) = renewal_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    let count = starts.clone();
    assert!(
        supervise(
            &c,
            &policy(),
            proof(&c, 1, 900, 50_000),
            renewals,
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(anyhow::anyhow!("synthetic operation failure"))
            }
        )
        .await
        .is_err()
    );
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}
#[test]
fn renewal_inbox_is_bounded_metadata_not_an_unbounded_event_queue() {
    let c = contract();
    let (sender, _renewals) = renewal_channel();
    for sequence in 2..=5 {
        assert!(sender.try_send(proof(&c, sequence, 900, 50_000)).is_ok());
    }
    assert!(matches!(
        sender.try_send(proof(&c, 6, 900, 50_000)),
        Err(mpsc::error::TrySendError::Full(_))
    ));
}

#[test]
fn issuer_requires_exact_authenticated_scope_and_durable_ack_before_renewing() {
    let c = contract();
    let mut issuer = IssuerWindow::new(c.clone(), &policy(), until(50_000)).unwrap();
    let q = query(&c, 1);
    assert!(
        issuer
            .reply(
                &q,
                &c.scope.issuer_id,
                &uuid::Uuid::new_v4().to_string(),
                &c.scope.executor_node
            )
            .is_err()
    );
    let first = issuer
        .reply(
            &q,
            &c.scope.issuer_id,
            &c.scope.session_id,
            &c.scope.executor_node,
        )
        .unwrap();
    assert_eq!(first.sequence, 1);
    assert!(first.lease_remaining_ms <= c.max_lease_ms);
    let next = query(&c, 2);
    assert!(
        issuer
            .reply(
                &next,
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
    let mut wrong = c.scope.clone();
    wrong.grant_sha256 = "e".repeat(64);
    assert!(issuer.acknowledge_consumed(&wrong).is_err());
    issuer.acknowledge_consumed(&c.scope).unwrap();
    assert!(issuer.acknowledge_consumed(&c.scope).is_err());
    let renewed = issuer
        .reply(
            &next,
            &c.scope.issuer_id,
            &c.scope.session_id,
            &c.scope.executor_node,
        )
        .unwrap();
    assert_eq!(renewed.sequence, 2);
    assert!(
        issuer
            .reply(
                &next,
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
}
#[test]
fn issuer_rejects_contract_widening_and_never_revives_a_stopped_or_expired_offer() {
    let c = contract();
    assert!(IssuerWindow::new(c.clone(), &policy(), until(120_000)).is_err());
    assert!(IssuerWindow::new(c.clone(), &policy(), Instant::now()).is_err());
    let original = until(50_000);
    let mut issuer = IssuerWindow::new(c.clone(), &policy(), original).unwrap();
    assert_eq!(issuer.deadline, original);
    let mut changed = c.clone();
    changed.max_session_ms += 1;
    assert!(
        issuer
            .reply(
                &query(&changed, 1),
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
    issuer.stop();
    assert!(
        issuer
            .reply(
                &query(&c, 1),
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
    assert!(issuer.acknowledge_consumed(&c.scope).is_err());
    let mut issuer = IssuerWindow::new(c.clone(), &policy(), until(50_000)).unwrap();
    issuer.deadline = Instant::now();
    assert!(
        issuer
            .reply(
                &query(&c, 1),
                &c.scope.issuer_id,
                &c.scope.session_id,
                &c.scope.executor_node
            )
            .is_err()
    );
}
#[tokio::test]
async fn issuer_and_nonce_correlator_agree_without_cross_machine_wall_clock_comparisons() {
    let c = contract();
    let mut issuer = IssuerWindow::new(c.clone(), &policy(), until(50_000)).unwrap();
    let requests = Arc::new(Requests::default());
    let manager = requests.clone();
    let proof = requests
        .request(&c, 1, |value| {
            let q: Query = serde_json::from_value(value).unwrap();
            let reply = issuer
                .reply(
                    &q,
                    &c.scope.issuer_id,
                    &c.scope.session_id,
                    &c.scope.executor_node,
                )
                .unwrap();
            manager.complete(json!(reply));
            async { Ok(()) }
        })
        .await
        .unwrap();
    assert!(proof.deadline <= proof.session_deadline);
    assert!(proof.session_deadline <= issuer.deadline);
    assert_eq!(proof.sequence, 1);
}
