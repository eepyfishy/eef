// Included in the existing real-journal tests. All locks are actual SQLite
// connection mutexes; there is a finite fallback if an assertion regresses.
struct FinalBlocker {
    release: Option<std::sync::mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<(bool, String)>>,
}
impl FinalBlocker {
    async fn hold(ledger: &Arc<GrantLedger>, definition: &Definition) -> Self {
        let ledger = ledger.clone();
        let id = definition.grant_id.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (locked, ready) = tokio::sync::oneshot::channel();
        let worker = std::thread::spawn(move || {
            let journal = ledger.connection.lock().unwrap();
            let _ = locked.send(());
            let released = wait.recv_timeout(Duration::from_secs(8)).is_ok();
            let state = journal
                .query_row(
                    "SELECT state FROM execution_receipts WHERE grant_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            drop(journal);
            (released, state)
        });
        ready.await.unwrap();
        Self {
            release: Some(release),
            worker: Some(worker),
        }
    }
    fn release(mut self) -> (bool, String) {
        let _ = self.release.take().unwrap().send(());
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for FinalBlocker {
    fn drop(&mut self) {
        if let Some(sender) = self.release.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn bounded_final_wait_keeps_control_responsive_and_cannot_restore_execution() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Retired(Arc<AtomicBool>);
    impl Drop for Retired {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let retired = Arc::new(AtomicBool::new(false));
    let owned = Retired(retired.clone());
    let (started, ready) = tokio::sync::oneshot::channel();
    let (finish, result) = tokio::sync::oneshot::channel();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let run = consumed.supervise(renewals, async move {
        let _owned = owned;
        let _ = started.send(());
        result.await?;
        Ok(7)
    });
    tokio::pin!(run);
    tokio::select! {_=ready=>{},value=&mut run=>panic!("Source ended before final wait: {value:?}")};
    let checkpoint = reporting.checkpoint(&contract).unwrap();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let _ = finish.send(());
    let outcome = tokio::time::timeout(Duration::from_secs(4), &mut run)
        .await
        .unwrap();
    assert!(
        outcome
            .unwrap_err()
            .to_string()
            .contains("Final workload checkpoint still pending")
    );
    assert!(retired.load(Ordering::SeqCst));
    assert!(checkpoint.check(&contract).is_err());
    assert_eq!(ledger.final_slots.available_permits(), 0);
    let mut fresh = definition();
    fresh.job_id = "e".repeat(32);
    fresh.task_id = "f".repeat(32);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), consume(&ledger, fresh.clone()))
            .await
            .unwrap()
            .is_err(),
        "Pending finalization must deny admission without waiting on the journal mutex"
    );
    let (released, state) = blocker.release();
    assert!(released);
    assert_eq!(state, "accepted");
    wait_unknown(&ledger, &d).await;
    assert!(!ledger.final_reporting_failed.load(Ordering::Acquire));
    assert!(
        consume(&ledger, d).await.is_err(),
        "A late final save cannot replay consumed work"
    );
    consume(&ledger, fresh)
        .await
        .unwrap()
        .retire()
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelling_final_waiter_keeps_reserved_capacity_until_the_actual_save_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let reporting = consumed.reporting_authority();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    {
        let final_wait = consumed.retire();
        tokio::pin!(final_wait);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut final_wait)
                .await
                .is_err()
        );
        // Drop the actual waiter; the already-queued save owns its capacity.
    }
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert!(reporting.checkpoint(&d.lease_contract().unwrap()).is_err());
    assert_eq!(blocker.release(), (true, "accepted".into()));
    wait_unknown(&ledger, &d).await;
    assert_eq!(ledger.final_slots.available_permits(), 1);
    assert!(consume(&ledger, d).await.is_err());
}

#[test]
fn dropping_consumption_without_a_runtime_does_not_wait_for_storage() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let consumed = runtime.block_on(consume(&ledger, d.clone())).unwrap();
    let blocker = runtime.block_on(FinalBlocker::hold(&ledger, &d));
    drop(runtime);
    let start = Instant::now();
    drop(consumed);
    let nonblocking = start.elapsed() < Duration::from_secs(1);
    assert_eq!(ledger.final_slots.available_permits(), 0);
    let (released, state) = blocker.release();
    assert!(released && nonblocking);
    assert_eq!(state, "accepted");
    let end = Instant::now() + Duration::from_secs(5);
    while ledger.final_slots.available_permits() == 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
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
async fn failed_final_checkpoint_latches_admission_denial_without_retry_or_false_completion() {
    use std::sync::atomic::Ordering;
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    ledger.connection.lock().unwrap().execute_batch("CREATE TRIGGER refuse_final BEFORE UPDATE ON execution_receipts WHEN NEW.state='unknown' BEGIN SELECT RAISE(ABORT,'synthetic final failure'); END;").unwrap();
    assert!(consumed.retire().await.is_err());
    assert!(ledger.final_reporting_failed.load(Ordering::Acquire));
    assert!(!ledger.workload_reporting_ready());
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "accepted"
    );
    ledger
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_final")
        .unwrap();
    let fresh = definition();
    let refused = consume(&ledger, fresh).await.err().unwrap();
    assert!(
        refused
            .to_string()
            .contains("Workload final reporting failed")
    );
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "accepted"
    );
    assert_eq!(
        ledger
            .connection
            .lock()
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn final_capacity_is_reserved_before_consumption_and_refusals_leave_no_new_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    assert_eq!(ledger.final_slots.available_permits(), 0);
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let mut fresh = definition();
    fresh.job_id = "1".repeat(32);
    fresh.task_id = "2".repeat(32);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), consume(&ledger, fresh.clone()))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(blocker.release(), (true, "accepted".into()));
    assert_eq!(
        ledger
            .connection
            .lock()
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        1
    );
    consumed.retire().await.unwrap();
    consume(&ledger, fresh)
        .await
        .unwrap()
        .retire()
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_post_commit_stop_registration_delegates_final_save_without_starting_a_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    ledger.connection.lock().unwrap().execute_batch("CREATE TABLE execution_stop_meta(schema_version INTEGER NOT NULL PRIMARY KEY); INSERT INTO execution_stop_meta VALUES(1); CREATE TABLE execution_stop_requests(grant_id TEXT NOT NULL PRIMARY KEY,grant_digest TEXT NOT NULL,issuer_id TEXT NOT NULL,session_id TEXT NOT NULL,node_id TEXT NOT NULL,job_id TEXT NOT NULL,task_id TEXT NOT NULL,attempt INTEGER NOT NULL,recorded_ms INTEGER NOT NULL,receipt_preexisted INTEGER NOT NULL);").unwrap();
    // Upgrade to v4 first, then inject failure AFTER the next consumed insert.
    let old = definition();
    consume(&ledger, old).await.unwrap().retire().await.unwrap();
    ledger.connection.lock().unwrap().execute_batch("CREATE TRIGGER corrupt_stop AFTER INSERT ON execution_receipts WHEN NEW.receipt_kind='workload' BEGIN UPDATE execution_stop_meta SET schema_version=99; END;").unwrap();
    let fresh = definition();
    assert!(consume(&ledger, fresh.clone()).await.is_err());
    wait_unknown(&ledger, &fresh).await;
    assert_eq!(
        ledger
            .workload_receipt(&fresh.issuer_id, &fresh.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(
        GrantLedger::open(&path, Default::default(), "owner").is_err(),
        "Damaged stop evidence is not repaired"
    );
}
