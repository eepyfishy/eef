// Real SQLite and the production bounded native worker. No source is created.
pub(super) fn admission_end() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(10)
}
async fn admission_entered(ledger: &GrantLedger) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while ledger
            .admission_started
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Original native admission did not enter");
}
async fn admission_released(ledger: &GrantLedger) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while ledger.event_workers.available_permits() != 1
            || ledger.final_slots.available_permits() != 1
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Actual admission/final save still owns its original capacity");
}
async fn admission_committed_unclaimed(ledger: &GrantLedger, d: &Definition) {
    tokio::time::timeout(Duration::from_secs(3), async {
        // The waiting future is deliberately NOT polled during this observer.
        while ledger.event_workers.available_permits() != 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert_eq!(
        ledger
            .ack_reads_started
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "Caller has not confirmed the original proof or read its ACK"
    );
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "accepted"
    );
}

struct AdmissionBlocker {
    release: Option<std::sync::mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<bool>>,
}
impl AdmissionBlocker {
    async fn hold(ledger: &Arc<GrantLedger>) -> Self {
        let ledger = ledger.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (locked, ready) = tokio::sync::oneshot::channel();
        let worker = std::thread::spawn(move || {
            let journal = ledger.connection.lock().unwrap();
            let _ = locked.send(());
            let released = wait.recv_timeout(Duration::from_secs(8)).is_ok();
            drop(journal);
            drop(ledger);
            released
        });
        ready.await.unwrap();
        Self {
            release: Some(release),
            worker: Some(worker),
        }
    }
    fn release(mut self) {
        let _ = self.release.take().unwrap().send(());
        assert!(self.worker.take().unwrap().join().unwrap());
    }
}
impl Drop for AdmissionBlocker {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
fn no_admission(path: &std::path::Path) {
    assert_eq!(version(path), 2);
    assert_eq!(
        Connection::open(path)
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn event_reservation_above_owner_limit_refuses_before_journal_then_exact_limit_consumes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("event-owner-limit.db");
    let policy = GrantPolicy::default();
    let ledger = GrantLedger::open(&path, policy.clone(), "owner").unwrap();
    let mut oversized = definition();
    oversized.events.max_total_bytes = policy.max_result_bytes + 1;
    oversized.validate().unwrap();
    let error = match consume(&ledger, oversized).await {
        Ok(_) => panic!("Oversized event reservation must not consume authority"),
        Err(error) => error,
    };
    assert_eq!(crate::grants::denial_code(&error), "grant_owner_limits_exceeded");
    assert_eq!(
        ledger.admission_started.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    no_admission(&path);
    let mut bounded = definition();
    bounded.events.max_total_bytes = policy.max_result_bytes;
    // A separately obtained proof and original admission budget, not a retry.
    consume(&ledger, bounded).await.unwrap().retire().await.unwrap();
    admission_released(&ledger).await;
    assert_eq!(
        ledger.admission_started.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn cancelled_native_admission_keeps_capacity_until_actual_mutex_wait_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancel-admission.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let blocker = AdmissionBlocker::hold(&ledger).await;
    let mut pending = Box::pin(consume(&ledger, definition()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held admission returned: {}", result.is_ok()),
        _ = admission_entered(&ledger) => {}
    }
    let start = Instant::now();
    drop(pending);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(ledger.event_workers.available_permits(), 0);
    assert_eq!(ledger.final_slots.available_permits(), 0);
    // The same observer remains responsive; another admission must refuse
    // immediately, without waiting on that actual native journal mutex.
    assert!(
        tokio::time::timeout(Duration::from_millis(500), consume(&ledger, definition()))
            .await
            .unwrap()
            .is_err()
    );
    blocker.release();
    admission_released(&ledger).await;
    no_admission(&path);
    assert!(
        !ledger
            .final_reporting_failed
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn caller_and_original_proof_deadlines_bound_consumption_without_late_commit() {
    for original in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deadline-admission.db");
        let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
        let d = definition();
        let proof = proof_for(&d, if original { 1000 } else { 50000 }).await;
        let blocker = AdmissionBlocker::hold(&ledger).await;
        let policy = policy();
        let end = if original {
            admission_end()
        } else {
            tokio::time::Instant::now() + Duration::from_millis(100)
        };
        let mut pending = Box::pin(ledger.consume_workload(
            d.clone(),
            &policy,
            proof,
            &d.issuer_id,
            &d.session_id,
            "owner",
            end,
        ));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Deadline before native entry: {}", result.is_ok()),
            _ = admission_entered(&ledger) => {}
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(2), &mut pending)
                .await
                .unwrap()
                .is_err()
        );
        drop(pending);
        assert_eq!(ledger.event_workers.available_permits(), 0);
        assert_eq!(ledger.final_slots.available_permits(), 0);
        blocker.release();
        admission_released(&ledger).await;
        no_admission(&path);
    }
}

#[tokio::test]
async fn losing_a_committed_admission_retires_unknown_using_its_original_final_slot() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("lost-admission.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let admission_blocker = AdmissionBlocker::hold(&ledger).await;
    let mut pending = Box::pin(consume(&ledger, d.clone()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Admission unexpectedly confirmed: {}", result.is_ok()),
        _ = admission_entered(&ledger) => {}
    }
    admission_blocker.release();
    admission_committed_unclaimed(&ledger, &d).await;
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    drop(pending);
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), consume(&ledger, definition()))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(blocker.release(), (true, "accepted".into()));
    wait_unknown(&ledger, &d).await;
    assert!(consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn committed_packet_cannot_refresh_an_expired_original_admission_proof() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("late-admission.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let policy = policy();
    let proof = proof_for(&d, 1000).await;
    let original_end = proof
        .initial_deadline(&d.lease_contract().unwrap(), &policy)
        .unwrap();
    let admission_blocker = AdmissionBlocker::hold(&ledger).await;
    let mut pending = Box::pin(ledger.consume_workload(
        d.clone(),
        &policy,
        proof,
        &d.issuer_id,
        &d.session_id,
        "owner",
        admission_end(),
    ));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Admission unexpectedly confirmed: {}", result.is_ok()),
        _ = admission_entered(&ledger) => {}
    }
    admission_blocker.release();
    admission_committed_unclaimed(&ledger, &d).await;
    tokio::time::sleep_until(tokio::time::Instant::from_std(original_end)).await;
    assert!(pending.await.is_err());
    wait_unknown(&ledger, &d).await;
    assert_eq!(
        ledger
            .ack_reads_started
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn stop_after_consumption_commit_is_checked_before_admission_returns_authority() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("stop-admission.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let admission_blocker = AdmissionBlocker::hold(&ledger).await;
    let mut pending = Box::pin(consume(&ledger, d.clone()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Admission unexpectedly confirmed: {}", result.is_ok()),
        _ = admission_entered(&ledger) => {}
    }
    admission_blocker.release();
    admission_committed_unclaimed(&ledger, &d).await;
    let stop = ledger
        .request_stop_scope(&d.scope().unwrap(), &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(stop.cancellation_signalled && !stop.execution_stopped);
    assert!(pending.await.is_err());
    wait_unknown(&ledger, &d).await;
    assert!(consume(&ledger, d).await.is_err());
}

#[test]
fn native_admission_keeps_actual_journal_ownership_after_async_runtime_destruction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime-admission.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let blocker = runtime.block_on(AdmissionBlocker::hold(&ledger));
    runtime.block_on(async {
        let mut pending = Box::pin(consume(&ledger, definition()));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Held admission returned: {}", result.is_ok()),
            _ = admission_entered(&ledger) => {}
        }
        drop(pending);
    });
    let weak = Arc::downgrade(&ledger);
    drop(ledger);
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(weak.upgrade().is_some());
    blocker.release();
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    observer.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            while weak.upgrade().is_some() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    });
    no_admission(&path);
}

#[tokio::test]
async fn failed_insert_rolls_back_upgrade_without_false_reporting_failure_or_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failed-admission.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    ledger.connection.lock().unwrap().execute_batch(
        "CREATE TEMP TRIGGER fail_workload_admission BEFORE INSERT ON execution_receipts BEGIN SELECT RAISE(ABORT,'synthetic admission failure'); END;"
    ).unwrap();
    assert!(consume(&ledger, definition()).await.is_err());
    admission_released(&ledger).await;
    no_admission(&path);
    assert!(
        !ledger
            .final_reporting_failed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert_eq!(
        ledger
            .admission_started
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    ledger
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_workload_admission")
        .unwrap();
    // Explicit new request after the injected fault is removed, not a retry loop.
    consume(&ledger, definition())
        .await
        .unwrap()
        .retire()
        .await
        .unwrap();
}

#[tokio::test]
async fn damaged_committed_packet_evidence_is_retained_and_denies_further_admission() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("damaged-admission.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let admission_blocker = AdmissionBlocker::hold(&ledger).await;
    let mut pending = Box::pin(consume(&ledger, d.clone()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Admission unexpectedly confirmed: {}", result.is_ok()),
        _ = admission_entered(&ledger) => {}
    }
    admission_blocker.release();
    admission_committed_unclaimed(&ledger, &d).await;
    ledger
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE execution_receipts SET workload_definition='{}' WHERE grant_id=?1",
            [&d.grant_id],
        )
        .unwrap();
    assert!(pending.await.is_err());
    admission_released(&ledger).await;
    assert!(
        ledger
            .final_reporting_failed
            .load(std::sync::atomic::Ordering::Acquire)
    );
    let raw: String = ledger
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT workload_definition FROM execution_receipts WHERE grant_id=?1",
            [&d.grant_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, "{}");
    assert!(consume(&ledger, definition()).await.is_err());
}
