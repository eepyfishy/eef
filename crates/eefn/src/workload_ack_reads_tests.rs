// Real grant journals and original lease proofs; no source process or device.
async fn ack_reader_entered(ledger: &GrantLedger) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while ledger
            .ack_reads_started
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Original native ACK reader did not enter");
}
async fn ack_reader_released(ledger: &GrantLedger) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while ledger.event_workers.available_permits() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Actual ACK reader still owns its original capacity");
}
fn ack_observation_end() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(2)
}

#[tokio::test]
async fn cancelled_ack_read_closes_the_kept_consumption_and_retains_actual_read_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("cancel-read.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let mut consumed = consume(&ledger, d.clone()).await.unwrap();
    let original_ack = consumed.acknowledgement().unwrap();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let mut pending = Box::pin(consumed.acknowledgement_before(ack_observation_end()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held ACK read returned: {}", result.is_ok()),
        _ = ack_reader_entered(&ledger) => {}
    }
    let start = Instant::now();
    drop(pending);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(consumed.initial.is_none());
    assert!(consumed.acknowledgement().is_err());
    assert!(consumed.event_writer_from_ack(original_ack).is_err());
    assert_eq!(ledger.event_workers.available_permits(), 0);
    assert_eq!(ledger.final_slots.available_permits(), 0);
    let (released, state) = blocker.release();
    assert!(released);
    assert_eq!(state, "accepted");
    ack_reader_released(&ledger).await;
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "accepted"
    );
    assert!(
        consumed
            .acknowledgement_before(ack_observation_end())
            .await
            .is_err()
    );
    assert_eq!(
        ledger
            .ack_reads_started
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    consumed.retire().await.unwrap();
    wait_unknown(&ledger, &d).await;
    assert!(consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn caller_and_original_lease_deadlines_both_bound_node_ack_inspection() {
    for original_lease in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let ledger = GrantLedger::open(
            &dir.path().join("deadline-read.db"),
            Default::default(),
            "owner",
        )
        .unwrap();
        let d = definition();
        let proof = proof_for(&d, if original_lease { 1000 } else { 50000 }).await;
        let mut consumed = ledger
            .consume_workload(
                d.clone(),
                &policy(),
                proof,
                &d.issuer_id,
                &d.session_id,
                "owner",
            )
            .unwrap();
        let blocker = FinalBlocker::hold(&ledger, &d).await;
        let end = if original_lease {
            ack_observation_end()
        } else {
            tokio::time::Instant::now() + Duration::from_millis(500)
        };
        let mut pending = Box::pin(consumed.acknowledgement_before(end));
        tokio::select! {
            biased;
            result = &mut pending => panic!("Read must enter before the deadline: {}", result.is_ok()),
            _ = ack_reader_entered(&ledger) => {}
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(2), pending)
                .await
                .unwrap()
                .is_err()
        );
        assert!(consumed.initial.is_none());
        assert_eq!(ledger.event_workers.available_permits(), 0);
        assert!(blocker.release().0);
        ack_reader_released(&ledger).await;
        consumed.retire().await.unwrap();
        wait_unknown(&ledger, &d).await;
    }
}

#[tokio::test]
async fn stop_after_a_native_ack_snapshot_cannot_start_the_original_consumption() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("stopped-read.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let mut consumed = consume(&ledger, d.clone()).await.unwrap();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let mut pending = Box::pin(consumed.acknowledgement_before(ack_observation_end()));
    tokio::select! {
        biased;
        result = &mut pending => panic!("Held read returned: {}", result.is_ok()),
        _ = ack_reader_entered(&ledger) => {}
    }
    assert!(blocker.release().0);
    // Keep the waiter unpolled until the actual read has delivered its snapshot.
    ack_reader_released(&ledger).await;
    let stopped = ledger
        .request_stop_scope(&d.scope().unwrap(), &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(stopped.cancellation_signalled && !stopped.execution_stopped);
    assert!(pending.await.is_err());
    assert!(consumed.initial.is_none());
    assert!(consumed.acknowledgement().is_err());
    consumed.retire().await.unwrap();
    wait_unknown(&ledger, &d).await;
}

#[tokio::test]
async fn unreadable_node_ack_retires_authority_without_repairing_retained_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("unreadable-read.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let mut consumed = consume(&ledger, d.clone()).await.unwrap();
    ledger
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE execution_receipts SET workload_definition='{}' WHERE grant_id=?1",
            [&d.grant_id],
        )
        .unwrap();
    assert!(
        consumed
            .acknowledgement_before(ack_observation_end())
            .await
            .is_err()
    );
    assert!(consumed.initial.is_none());
    assert!(consumed.retire().await.is_err());
    tokio::time::timeout(Duration::from_secs(3), async {
        while ledger.final_slots.available_permits() == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
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

struct UnpolledAckSource(Arc<std::sync::atomic::AtomicUsize>);
impl std::future::Future for UnpolledAckSource {
    type Output = Result<()>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.0.fetch_or(1, std::sync::atomic::Ordering::SeqCst);
        std::task::Poll::Pending
    }
}
impl Drop for UnpolledAckSource {
    fn drop(&mut self) {
        self.0.fetch_or(2, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test]
async fn runner_cancel_drops_its_unpolled_source_while_original_ack_read_is_blocked() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &dir.path().join("runner-read.db"),
        Default::default(),
        "owner",
    )
    .unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let state = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let mut runner = Box::pin(consumed.supervise(renewals, UnpolledAckSource(state.clone())));
    tokio::select! {
        biased;
        result = &mut runner => panic!("Held runner returned: {}", result.is_ok()),
        _ = ack_reader_entered(&ledger) => {}
    }
    drop(runner);
    assert_eq!(
        state.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "Drop source without ever polling it"
    );
    assert_eq!(ledger.event_workers.available_permits(), 0);
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert!(blocker.release().0);
    wait_unknown(&ledger, &d).await;
}

#[test]
fn node_ack_reader_and_finalization_keep_actual_ownership_after_runtime_destruction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime-read.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let d = definition();
    let consumed = runtime.block_on(consume(&ledger, d.clone())).unwrap();
    // Only this contention fixture extends SQLite's busy wait so the test can
    // observe the original actual reader across runtime teardown. No app setting.
    ledger
        .connection
        .lock()
        .unwrap()
        .busy_timeout(Duration::from_secs(3))
        .unwrap();
    let external = Connection::open(&path).unwrap();
    external.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let weak = Arc::downgrade(&ledger);
    let state = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    runtime.block_on(async {
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        let mut runner = Box::pin(consumed.supervise(renewals, UnpolledAckSource(state.clone())));
        tokio::select! {
            biased;
            result = &mut runner => panic!("Held runner returned: {}", result.is_ok()),
            _ = ack_reader_entered(&ledger) => {}
        }
        drop(runner);
    });
    drop(ledger);
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(state.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(
        weak.upgrade().is_some(),
        "Actual native read/finalization retains its exact journal"
    );
    external.execute_batch("ROLLBACK").unwrap();
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
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert_eq!(
        reopened
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
}
