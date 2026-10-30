use super::*;
use crate::workload_leases::{IssuerWindow, Query, Requests, definition::*};

include!("workload_event_journal_tests.rs");

#[derive(Default)]
struct SourceEvidence {
    polled: std::sync::atomic::AtomicBool,
    dropped: std::sync::atomic::AtomicBool,
    receipt_accepted_at_drop: std::sync::atomic::AtomicBool,
}
struct PendingSource {
    ledger: Arc<GrantLedger>,
    definition: Definition,
    evidence: Arc<SourceEvidence>,
    started: Option<tokio::sync::oneshot::Sender<()>>,
}
impl std::future::Future for PendingSource {
    type Output = Result<()>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.evidence
            .polled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }
        std::task::Poll::Pending
    }
}
impl Drop for PendingSource {
    fn drop(&mut self) {
        let accepted = self
            .ledger
            .workload_receipt(&self.definition.issuer_id, &self.definition.grant_id)
            .is_ok_and(|r| r.receipt.state == "accepted");
        self.evidence
            .receipt_accepted_at_drop
            .store(accepted, std::sync::atomic::Ordering::SeqCst);
        self.evidence
            .dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
fn pending_source(
    ledger: &Arc<GrantLedger>,
    definition: &Definition,
) -> (
    PendingSource,
    tokio::sync::oneshot::Receiver<()>,
    Arc<SourceEvidence>,
) {
    let evidence = Arc::new(SourceEvidence::default());
    let (started, receiver) = tokio::sync::oneshot::channel();
    (
        PendingSource {
            ledger: ledger.clone(),
            definition: definition.clone(),
            evidence: evidence.clone(),
            started: Some(started),
        },
        receiver,
        evidence,
    )
}
fn source_retired_before_uncertainty(evidence: &SourceEvidence) {
    use std::sync::atomic::Ordering::SeqCst;
    assert!(evidence.polled.load(SeqCst));
    assert!(evidence.dropped.load(SeqCst));
    assert!(
        evidence.receipt_accepted_at_drop.load(SeqCst),
        "Source Drop precedes final journal retirement"
    );
}

fn policy() -> Policy {
    Policy {
        enabled: true,
        ..Default::default()
    }
}
fn definition() -> Definition {
    let g = crate::grants::tests::grant();
    Definition {
        schema_version: 2,
        grant_id: g.grant_id,
        issuer_id: g.issuer_id,
        session_id: g.session_id,
        executor_node: g.executor_node,
        job_id: g.job_id,
        task_id: g.task_id,
        attempt: 1,
        plan_sha256: g.plan_sha256,
        request_context: g.request_context,
        priority: 0,
        source: Source::MicrophoneEnergy {
            resource_id: "owner::microphone".into(),
            resource_sha256: "d".repeat(64),
            adapter_sha256: "e".repeat(64),
            parameters: EnergyParameters {
                sample_rate: 16000,
                channels: 1,
                block_frames: 80,
                max_blocks: 100,
                activation_level: 1000,
                deactivation_level: 500,
            },
        },
        limits: Limits {
            max_session_ms: 60_000,
            max_lease_ms: 10_000,
            max_job_memory_mib: 128,
            max_source_frames: 8000,
        },
        events: Events {
            mode: EventMode::ActivityTransitions,
            min_interval_ms: 100,
            max_events: 100,
            max_event_bytes: 1024,
            max_total_bytes: 65536,
            max_pending: 4,
            overflow: Overflow::Stop,
        },
    }
}
async fn proof_for(d: &Definition, remaining_ms: u64) -> LeaseProof {
    let c = d.lease_contract().unwrap();
    let mut issuer = IssuerWindow::new(
        c.clone(),
        &policy(),
        Instant::now() + Duration::from_millis(remaining_ms),
    )
    .unwrap();
    let requests = Arc::new(Requests::default());
    let manager = requests.clone();
    requests
        .request(&c, 1, |value| {
            let q: Query = serde_json::from_value(value).unwrap();
            let reply = issuer
                .reply(&q, &d.issuer_id, &d.session_id, &d.executor_node)
                .unwrap();
            manager.complete(json!(reply));
            async { Ok(()) }
        })
        .await
        .unwrap()
}
async fn consume(ledger: &Arc<GrantLedger>, d: Definition) -> Result<Consumption> {
    let proof = proof_for(&d, 50_000).await;
    let (issuer, session, node) = (
        d.issuer_id.clone(),
        d.session_id.clone(),
        d.executor_node.clone(),
    );
    ledger.consume_workload(d, &policy(), proof, &issuer, &session, &node)
}
fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn consumed_runner_stop_drops_source_before_final_real_journal_write() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    consumed.acknowledgement().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    assert!(reporting.checkpoint(&contract).is_err());
    let (source, started, evidence) = pending_source(&ledger, &d);
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let run = consumed.supervise(renewals, source);
    tokio::pin!(run);
    tokio::select! {value=started=>value.unwrap(), result=&mut run=>panic!("Source ended before stop: {result:?}")};
    let checkpoint = reporting.checkpoint(&contract).unwrap();
    let stop = ledger
        .request_stop_scope(
            &d.scope().unwrap(),
            &d.issuer_id,
            &d.session_id,
            &d.executor_node,
        )
        .unwrap();
    assert!(stop.cancellation_signalled);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut run)
            .await
            .unwrap()
            .is_err()
    );
    source_retired_before_uncertainty(&evidence);
    assert!(checkpoint.check(&contract).is_err());
    assert!(reporting.checkpoint(&contract).is_err());
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}

#[tokio::test]
async fn cancelling_whole_consumed_task_drops_source_before_persisting_uncertainty() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (source, started, evidence) = pending_source(&ledger, &d);
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    {
        let run = consumed.supervise(renewals, source);
        tokio::pin!(run);
        tokio::select! {value=started=>value.unwrap(), result=&mut run=>panic!("Source ended before cancellation: {result:?}")};
        // Drop the actual future at scope exit, not just a Pin reference to it.
    }
    source_retired_before_uncertainty(&evidence);
    assert!(reporting.checkpoint(&contract).is_err());
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}

#[tokio::test]
async fn retired_consumption_never_polls_a_prepared_source_again() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let (source, _started, evidence) = pending_source(&ledger, &d);
    ledger.mark_workload_unknown(&d.scope().unwrap()).unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    assert!(consumed.supervise(renewals, source).await.is_err());
    assert!(!evidence.polled.load(SeqCst));
    assert!(evidence.dropped.load(SeqCst));
    assert!(
        consume(&ledger, d).await.is_err(),
        "Consumed uncertainty is never replayable"
    );
}

#[tokio::test]
async fn source_result_alone_cannot_mark_a_consumed_session_completed() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    assert_eq!(
        consumed
            .supervise(renewals, async {
                reporting.checkpoint(&contract)?.check(&contract)?;
                Ok(7)
            })
            .await
            .unwrap(),
        7
    );
    let retained = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
    assert_eq!(retained.receipt.state, "unknown");
    assert!(!retained.execution_authorized && !retained.restart_authorized);
    assert!(reporting.checkpoint(&contract).is_err());
}

#[tokio::test]
async fn whole_task_cancellation_retires_reporting_before_blocked_final_journal_io() {
    use std::sync::mpsc;
    struct SourceDrop(mpsc::Sender<()>);
    impl Drop for SourceDrop {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let owned = SourceDrop(dropped_tx);
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let source = async move {
                let _owned = owned;
                started_tx.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            };
            let run = consumed.supervise(renewals, source);
            tokio::pin!(run);
            tokio::select! {
                biased;
                _=cancel_rx=>{},
                result=&mut run=>panic!("Source ended before requested cancellation: {result:?}"),
            }
            // The actual owned run future drops at scope exit. Its final
            // journal write will block on the real mutex held by this test.
        });
    });
    let started = started_rx.recv_timeout(Duration::from_secs(5));
    if started.is_err() {
        let _ = cancel_tx.send(());
        let _ = worker.join();
        panic!("Source did not start: {started:?}");
    }
    let checkpoint = reporting.checkpoint(&contract).unwrap();
    let journal = ledger.connection.lock().unwrap();
    let _ = cancel_tx.send(());
    let source_dropped = dropped_rx.recv_timeout(Duration::from_secs(5));
    let retired_before_save =
        checkpoint.check(&contract).is_err() && reporting.checkpoint(&contract).is_err();
    let waiting_for_save = !worker.is_finished();
    let state = journal.query_row::<String, _, _>(
        "SELECT state FROM execution_receipts WHERE grant_id=?1",
        [&d.grant_id],
        |r| r.get(0),
    );
    drop(journal);
    let joined = worker.join();
    source_dropped.unwrap();
    joined.unwrap();
    assert!(
        retired_before_save,
        "Reporting must retire before source destruction and blocked storage"
    );
    assert!(waiting_for_save);
    assert_eq!(state.unwrap(), "accepted");
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn consumed_runner_uses_original_lease_and_drops_source_on_missing_renewal() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let proof = proof_for(&d, 3000).await;
    let consumed = ledger
        .consume_workload(
            d.clone(),
            &policy(),
            proof,
            &d.issuer_id,
            &d.session_id,
            &d.executor_node,
        )
        .unwrap();
    let (source, started, evidence) = pending_source(&ledger, &d);
    let (sender, renewals) = crate::workload_leases::renewal_channel();
    drop(sender);
    let run = consumed.supervise(renewals, source);
    tokio::pin!(run);
    tokio::select! {value=started=>value.unwrap(), result=&mut run=>panic!("Source ended before first poll: {result:?}")};
    let error = tokio::time::timeout(Duration::from_secs(5), &mut run)
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("expired"));
    source_retired_before_uncertainty(&evidence);
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}

#[tokio::test]
async fn real_journal_consumes_exact_definition_before_ack_and_preserves_legacy_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.sqlite3");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let old = crate::grants::tests::grant();
    ledger.claim(&old).unwrap();
    let report = GrantReport::new(
        &old,
        ReceiptState::Unknown,
        Value::Null,
        Some("synthetic_uncertainty"),
    );
    ledger.finish(&old, &report).unwrap();
    let old_digest = definition_digest(&json!(old)).unwrap();
    let before = ledger
        .result(&old.issuer_id, &old.grant_id, &old_digest)
        .unwrap();
    assert_eq!(version(&path), 2);
    let d = definition();
    let expected = d.scope().unwrap();
    let token = consume(&ledger, d.clone()).await.unwrap();
    assert_eq!(version(&path), 4);
    let ack = token.acknowledgement().unwrap();
    ack.validate().unwrap();
    assert_eq!(ack.definition.scope().unwrap(), expected);
    assert_eq!(ack.receipt.state, "accepted");
    assert!(!ack.execution_authorized && !ack.restart_authorized);
    // Evidence was committed: an independent connection sees it before ACK use.
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        read(&db, "owner", Some(&d.issuer_id), &d.grant_id)
            .unwrap()
            .definition
            .scope()
            .unwrap(),
        expected
    );
    assert!(
        ledger
            .result(&d.issuer_id, &d.grant_id, &expected.grant_sha256)
            .is_err()
    );
    assert_eq!(
        ledger
            .result(&old.issuer_id, &old.grant_id, &old_digest)
            .unwrap(),
        before
    );
    assert!(consume(&ledger, d.clone()).await.is_err());
    drop(token);
    drop(ledger);
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let retained = reopened
        .workload_receipt(&d.issuer_id, &d.grant_id)
        .unwrap();
    assert_eq!(retained.receipt.state, "unknown");
    assert!(!retained.restart_authorized && !retained.execution_authorized);
    assert!(consume(&reopened, d).await.is_err());
    assert_eq!(
        reopened
            .result(&old.issuer_id, &old.grant_id, &old_digest)
            .unwrap(),
        before
    );
}

#[tokio::test]
async fn unresolved_attempts_fence_both_operation_kinds_and_higher_attempts() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("one.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let token = consume(&ledger, d.clone()).await.unwrap();
    drop(token);
    let mut operation = crate::grants::tests::grant();
    operation.issuer_id = d.issuer_id.clone();
    operation.job_id = d.job_id.clone();
    operation.task_id = d.task_id.clone();
    operation.attempt = 2;
    assert_eq!(
        denial_code(&ledger.claim(&operation).unwrap_err()),
        "grant_attempt_replayed_or_unresolved"
    );
    let mut later = d.clone();
    later.grant_id = uuid::Uuid::new_v4().to_string();
    later.attempt = 2;
    assert!(consume(&ledger, later).await.is_err());
    let other = GrantLedger::open(&dir.path().join("two.db"), Default::default(), "owner").unwrap();
    other.claim(&operation).unwrap();
    let mut session = d;
    session.attempt = 3;
    assert!(consume(&other, session).await.is_err());
}

#[tokio::test]
async fn independent_handles_can_consume_one_attempt_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let one = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let two = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let d = definition();
    let p1 = proof_for(&d, 50_000).await;
    let p2 = proof_for(&d, 50_000).await;
    let barrier = std::sync::Barrier::new(2);
    let (a, b) = std::thread::scope(|scope| {
        let start = |ledger: Arc<GrantLedger>, d: Definition, proof| {
            let barrier = &barrier;
            move || {
                barrier.wait();
                ledger.consume_workload(
                    d.clone(),
                    &policy(),
                    proof,
                    &d.issuer_id,
                    &d.session_id,
                    &d.executor_node,
                )
            }
        };
        let a = scope.spawn(start(one.clone(), d.clone(), p1));
        let b = scope.spawn(start(two.clone(), d.clone(), p2));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        1
    );
    drop(a);
    drop(b);
    assert_eq!(
        one.workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}

#[tokio::test]
async fn existing_stop_journal_fences_before_start_and_signals_consumed_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let d = definition();
    let scope = d.scope().unwrap();
    ledger
        .request_stop_scope(&scope, &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(consume(&ledger, d).await.is_err());
    assert_eq!(version(&path), 2);
    let d = definition();
    let token = consume(&ledger, d.clone()).await.unwrap();
    let scope = d.scope().unwrap();
    let stop = ledger
        .request_stop_scope(&scope, &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(stop.cancellation_signalled);
    assert!(!stop.execution_stopped && !stop.external_effects_stopped);
    assert!(token.acknowledgement().is_err());
    drop(token);
    drop(ledger);
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert_eq!(
        reopened
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(
        reopened
            .inspect_stop(&scope, &d.issuer_id, "owner")
            .unwrap()
            .confirms(&scope)
    );
}

#[tokio::test]
async fn disabled_policy_wrong_session_and_shared_quota_denials_leave_no_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(
        &path,
        GrantPolicy {
            max_receipts: 1,
            ..Default::default()
        },
        "owner",
    )
    .unwrap();
    let d = definition();
    let p = proof_for(&d, 50_000).await;
    assert!(
        ledger
            .consume_workload(
                d.clone(),
                &Policy::default(),
                p,
                &d.issuer_id,
                &d.session_id,
                "owner"
            )
            .is_err()
    );
    let p = proof_for(&d, 50_000).await;
    assert!(
        ledger
            .consume_workload(
                d.clone(),
                &policy(),
                p,
                &d.issuer_id,
                &uuid::Uuid::new_v4().to_string(),
                "owner"
            )
            .is_err()
    );
    assert_eq!(version(&path), 2);
    let p = proof_for(&d, 50_000).await;
    let mut changed = d.clone();
    let Source::MicrophoneEnergy { parameters, .. } = &mut changed.source;
    parameters.activation_level += 1;
    assert!(
        ledger
            .consume_workload(changed, &policy(), p, &d.issuer_id, &d.session_id, "owner")
            .is_err()
    );
    assert_eq!(version(&path), 2);
    ledger.claim(&crate::grants::tests::grant()).unwrap();
    assert!(consume(&ledger, d).await.is_err());
    assert_eq!(version(&path), 2);
    let small_path = dir.path().join("small.db");
    let small = GrantLedger::open(
        &small_path,
        GrantPolicy {
            max_journal_bytes: 65536,
            ..Default::default()
        },
        "owner",
    )
    .unwrap();
    assert!(consume(&small, definition()).await.is_err());
    assert_eq!(version(&small_path), 2);
}

#[tokio::test]
async fn sqlite_lock_wait_spends_original_lease_and_rolls_back_schema_and_consumption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    ledger
        .connection
        .lock()
        .unwrap()
        .busy_timeout(Duration::from_secs(2))
        .unwrap();
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let d = definition();
    let p = proof_for(&d, 1000).await;
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1250));
        blocker.execute_batch("COMMIT").unwrap();
    });
    let start = Instant::now();
    assert!(
        ledger
            .consume_workload(
                d.clone(),
                &policy(),
                p,
                &d.issuer_id,
                &d.session_id,
                "owner"
            )
            .is_err()
    );
    assert!(start.elapsed() >= Duration::from_secs(1));
    release.join().unwrap();
    assert_eq!(version(&path), 2);
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn malformed_retained_definition_or_reservation_is_refused_without_repair() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    for (index,sql) in [
        "UPDATE execution_receipts SET workload_definition='{}' WHERE receipt_kind='workload'",
        "UPDATE execution_receipts SET max_result_bytes=0 WHERE receipt_kind='workload'",
        "UPDATE execution_receipts SET state='completed' WHERE receipt_kind='workload'",
        "UPDATE execution_receipts SET receipt_kind='operation' WHERE receipt_kind='workload'",
        "UPDATE execution_receipts SET workload_definition=replace(hex(zeroblob(32769)),'0',' ') WHERE receipt_kind='workload'",
    ].into_iter().enumerate() {
        let path=dir.path().join(format!("corrupt-{index}.db"));
        let ledger=GrantLedger::open(&path,Default::default(),"owner").unwrap();
        drop(consume(&ledger,definition()).await.unwrap());drop(ledger);
        let db=Connection::open(&path).unwrap();db.execute_batch(sql).unwrap();drop(db);
        let before=Sha256::digest(std::fs::read(&path).unwrap());
        assert!(GrantLedger::open(&path,Default::default(),"owner").is_err(),"{index}");
        assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()),before,"No repair for case{index}");
    }
}

#[tokio::test]
async fn downgraded_schema_owner_mismatch_and_changed_legacy_evidence_are_retained() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    drop(consume(&ledger, definition()).await.unwrap());
    drop(ledger);
    assert!(GrantLedger::open(&path, Default::default(), "other-node").is_err());
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA user_version=2").unwrap();
    drop(db);
    assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
    let path = dir.path().join("legacy.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let g = crate::grants::tests::grant();
    ledger.claim(&g).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("UPDATE execution_receipts SET grant_digest='damaged'")
        .unwrap();
    drop(db);
    assert!(consume(&ledger, definition()).await.is_err());
    assert_eq!(version(&path), 2);
}
