// Included in the existing real-journal test module; no second job database.
fn stored_event(
    sequence: u64,
    source_sequence: u64,
    active: Option<bool>,
) -> crate::workload_events::Event {
    crate::workload_events::Event {
        schema_version: 2,
        kind: crate::workload_events::Kind::MicrophoneActivity,
        sequence,
        source_sequence,
        source_frames: source_sequence * 80,
        elapsed_ms: (sequence - 1) * 100,
        active,
        level: active.map(|active| if active { 1200 } else { 0 }),
        discontinuity: active.is_none(),
    }
}
async fn store_events(
    ledger: &Arc<GrantLedger>,
    definition: Definition,
    events: Vec<crate::workload_events::Event>,
) -> Result<Vec<event_journal::AppendReceipt>> {
    let consumed = consume(ledger, definition.clone()).await?;
    let writer = consumed.event_writer()?;
    let reporting = consumed.reporting_authority();
    let contract = definition.lease_contract()?;
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            let mut results = Vec::new();
            for event in events {
                results.push(
                    writer
                        .append(event, Arc::new(reporting.checkpoint(&contract)?))
                        .await?,
                );
            }
            Ok(results)
        })
        .await
}

#[tokio::test]
async fn event_prefix_reopens_with_bounded_inspection_and_exact_duplicate_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let d = definition();
    let first = stored_event(1, 1, Some(false));
    let second = stored_event(2, 3, Some(true));
    let receipts = store_events(
        &ledger,
        d.clone(),
        vec![first.clone(), first.clone(), second.clone()],
    )
    .await
    .unwrap();
    assert!(!receipts[0].reused && receipts[1].reused && !receipts[2].reused);
    assert_eq!(receipts[0].data_sha256, receipts[1].data_sha256);
    assert_eq!(
        receipts.iter().map(|r| r.sequence).collect::<Vec<_>>(),
        [1, 1, 2]
    );
    let shown = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 0, 1)
        .unwrap();
    assert_eq!(shown["durable_events"][0]["event"], json!(first));
    assert_eq!(shown["event_count"], 2);
    assert_eq!(shown["more"], true);
    assert_eq!(shown["next_sequence"], 1);
    assert_eq!(shown["execution_authorized"], false);
    assert_eq!(shown["source_termination_confirmed"], false);
    let last = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 1, 128)
        .unwrap();
    assert_eq!(last["durable_events"][0]["event"], json!(second));
    assert_eq!(last["more"], false);
    for (issuer, id, cursor, limit) in [
        ("f".repeat(64), d.grant_id.clone(), 0, 1),
        (d.issuer_id.clone(), uuid::Uuid::new_v4().to_string(), 0, 1),
        (d.issuer_id.clone(), d.grant_id.clone(), 3, 1),
        (d.issuer_id.clone(), d.grant_id.clone(), 0, 0),
        (d.issuer_id.clone(), d.grant_id.clone(), 0, 129),
    ] {
        assert!(ledger.workload_events(&issuer, &id, cursor, limit).is_err());
    }
    drop(ledger);
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert_eq!(
        reopened
            .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
            .unwrap()["state"],
        "unknown"
    );
    assert!(
        consume(&reopened, d).await.is_err(),
        "Historical events never restore execution"
    );
}

#[tokio::test]
async fn conflicting_or_skipped_event_identity_never_changes_retained_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            let first = stored_event(1, 1, Some(false));
            writer
                .append(first.clone(), Arc::new(reporting.checkpoint(&contract)?))
                .await?;
            let before = ledger.workload_events(&d.issuer_id, &d.grant_id, 0, 128)?;
            let mut conflicting = first.clone();
            conflicting.level = Some(100);
            for event in [
                conflicting,
                stored_event(3, 3, Some(true)),
                stored_event(2, 1, Some(true)),
            ] {
                assert!(
                    writer
                        .append(event, Arc::new(reporting.checkpoint(&contract)?))
                        .await
                        .is_err()
                );
                assert_eq!(
                    ledger.workload_events(&d.issuer_id, &d.grant_id, 0, 128)?,
                    before
                );
            }
            assert!(
                writer
                    .append(first, Arc::new(reporting.checkpoint(&contract)?))
                    .await?
                    .reused
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn malformed_event_metadata_is_refused_before_any_persistent_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            for (field, value) in [
                ("schema_version", json!(1)),
                ("source_sequence", json!(0)),
                ("source_frames", json!(81)),
                ("elapsed_ms", json!(1)),
                ("level", json!(32769)),
                ("level", Value::Null),
                ("discontinuity", json!(true)),
            ] {
                let mut event = json!(stored_event(1, 1, Some(true)));
                event[field] = value;
                assert!(
                    writer
                        .append(
                            serde_json::from_value(event)?,
                            Arc::new(reporting.checkpoint(&contract)?)
                        )
                        .await
                        .is_err()
                );
                assert_eq!(
                    ledger.workload_events(&d.issuer_id, &d.grant_id, 0, 128)?["event_count"],
                    0
                );
            }
            writer
                .append(
                    stored_event(1, 1, Some(false)),
                    Arc::new(reporting.checkpoint(&contract)?),
                )
                .await?;
            for mut event in [
                stored_event(2, 2, Some(true)),
                stored_event(2, 2, Some(false)),
            ] {
                event.elapsed_ms = 99;
                assert!(
                    writer
                        .append(event, Arc::new(reporting.checkpoint(&contract)?))
                        .await
                        .is_err()
                );
            }
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn persistent_event_count_individual_and_total_bytes_enforce_reviewed_bounds() {
    let dir = tempfile::tempdir().unwrap();
    for mode in ["count", "individual", "total"] {
        let ledger = GrantLedger::open(
            &dir.path().join(format!("{mode}.db")),
            Default::default(),
            "owner",
        )
        .unwrap();
        let mut d = definition();
        match mode {
            "count" => d.events.max_events = 1,
            "individual" => d.events.max_event_bytes = 128,
            _ => {
                d.events.max_total_bytes = 1024;
                d.events.max_event_bytes = 512;
            }
        }
        let consumed = consume(&ledger, d.clone()).await.unwrap();
        let writer = consumed.event_writer().unwrap();
        let reporting = consumed.reporting_authority();
        let contract = d.lease_contract().unwrap();
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        consumed
            .supervise(renewals, async {
                let mut count = 0;
                let mut bytes = 0;
                for sequence in 1..=8 {
                    let event = stored_event(sequence, sequence, Some(sequence % 2 == 0));
                    let size = serde_json::to_vec(&event)?.len();
                    let expected = sequence <= d.events.max_events
                        && size <= d.events.max_event_bytes
                        && bytes + size <= d.events.max_total_bytes;
                    let result = writer
                        .append(event, Arc::new(reporting.checkpoint(&contract)?))
                        .await;
                    assert_eq!(result.is_ok(), expected, "{mode}:{sequence}");
                    if expected {
                        count += 1;
                        bytes += size;
                    } else {
                        break;
                    }
                }
                let shown = ledger.workload_events(&d.issuer_id, &d.grant_id, 0, 128)?;
                assert_eq!(shown["event_count"], count);
                assert_eq!(shown["event_bytes"], bytes);
                assert!(count < 8);
                Ok(())
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn stopped_and_retired_event_writers_cannot_persist_new_observations() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let held = Arc::new(Mutex::new(None));
    let captured = held.clone();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    assert!(
        consumed
            .supervise(renewals, async {
                let checkpoint = Arc::new(reporting.checkpoint(&contract)?);
                *captured.lock().unwrap() = Some(checkpoint.clone());
                ledger.request_stop_scope(&contract.scope, &d.issuer_id, &d.session_id, "owner")?;
                writer
                    .append(stored_event(1, 1, Some(false)), checkpoint)
                    .await?;
                Ok(())
            })
            .await
            .is_err()
    );
    let checkpoint = held.lock().unwrap().take().unwrap();
    assert!(
        writer
            .append(stored_event(1, 1, Some(false)), checkpoint)
            .await
            .is_err()
    );
    assert_eq!(
        ledger
            .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
            .unwrap()["event_count"],
        0
    );
}

#[tokio::test]
async fn event_insert_failure_rolls_back_counters_and_never_claims_completion() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    ledger.connection.lock().unwrap().execute_batch("CREATE TRIGGER refuse_event BEFORE INSERT ON execution_workload_events BEGIN SELECT RAISE(ABORT,'synthetic event save failure'); END;").unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    assert!(
        consumed
            .supervise(renewals, async {
                writer
                    .append(
                        stored_event(1, 1, Some(false)),
                        Arc::new(reporting.checkpoint(&contract)?),
                    )
                    .await?;
                Ok(())
            })
            .await
            .is_err()
    );
    let shown = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
        .unwrap();
    assert_eq!(shown["event_count"], 0);
    assert_eq!(shown["state"], "unknown");
}

#[tokio::test]
async fn event_storage_reservation_is_shared_with_ordinary_receipts_before_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(
        &path,
        GrantPolicy {
            max_result_bytes: 1024 * 1024,
            ..Default::default()
        },
        "owner",
    )
    .unwrap();
    let mut first = definition();
    first.events.max_events = 200;
    let token = consume(&ledger, first.clone()).await.unwrap();
    let mut second = definition();
    second.job_id = "f".repeat(32);
    second.task_id = "e".repeat(32);
    assert!(
        consume(&ledger, second.clone()).await.is_err(),
        "Outstanding row/index reservation must be charged"
    );
    let mut ordinary = crate::grants::tests::grant();
    ordinary.limits.max_result_bytes = 1024 * 1024;
    ordinary.job_id = "1".repeat(32);
    ordinary.task_id = "2".repeat(32);
    assert_eq!(
        crate::grants::denial_code(&ledger.claim(&ordinary).unwrap_err()),
        "grant_receipt_quota_full",
        "Ordinary operations cannot bypass event reservations"
    );
    drop(token);
    // Retired reservations no longer block a fresh task; physical retained pages
    // and all old result/receipt budgets still count.
    drop(consume(&ledger, second).await.unwrap());
    assert_eq!(version(&path), 4);
}

#[tokio::test]
async fn damaged_event_prefix_and_downgraded_schema_are_retained_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    for (index,sql) in [
        "UPDATE execution_workload_events SET data='{}'",
        "UPDATE execution_workload_events SET data_sha256=replace(hex(zeroblob(32)),'0','a')",
        "UPDATE execution_workload_event_state SET event_count=event_count+1",
        "UPDATE execution_workload_event_state SET reserved_bytes=reserved_bytes+1",
        "DELETE FROM execution_workload_events",
        "PRAGMA user_version=3",
        "DROP TABLE execution_workload_event_state",
        "UPDATE execution_workload_events SET data=hex(zeroblob(8192))",
        "UPDATE execution_workload_events SET data_sha256=hex(zeroblob(8192))",
        "UPDATE execution_workload_event_state SET prefix_sha256=hex(zeroblob(8192))",
        "INSERT INTO execution_workload_event_state SELECT '00000000-0000-4000-8000-000000000001',grant_digest,0,0,0,prefix_sha256 FROM execution_workload_event_state",
    ].into_iter().enumerate() {
        let path=dir.path().join(format!("corrupt-{index}.db"));
        let ledger=GrantLedger::open(&path,Default::default(),"owner").unwrap();
        store_events(&ledger,definition(),vec![stored_event(1,1,Some(false))]).await.unwrap();
        drop(ledger); let db=Connection::open(&path).unwrap();db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();db.execute_batch(sql).unwrap();drop(db);
        let before=Sha256::digest(std::fs::read(&path).unwrap());
        assert!(GrantLedger::open(&path,Default::default(),"owner").is_err(),"{index}");
        assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()),before,"No repair for{index}");
    }
}

#[tokio::test]
async fn cancelled_event_save_retains_worker_slot_and_never_writes_after_retirement() {
    use std::sync::mpsc;
    struct Dropped(mpsc::Sender<()>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let source_contract = contract.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (write_tx, write_rx) = tokio::sync::oneshot::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let owned = Dropped(dropped_tx);
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let source = async move {
                    let _owned = owned;
                    let checkpoint = Arc::new(reporting.checkpoint(&source_contract)?);
                    started_tx.send(checkpoint.clone()).unwrap();
                    write_rx.await?;
                    writer
                        .append(stored_event(1, 1, Some(false)), checkpoint)
                        .await?;
                    std::future::pending::<Result<()>>().await
                };
                let run = consumed.supervise(renewals, source);
                tokio::pin!(run);
                tokio::select! {
                    biased;
                    _=cancel_rx=>{},
                    result=&mut run=>panic!("Source ended before cancellation: {result:?}"),
                }
            });
    });
    let ready = started_rx.recv_timeout(Duration::from_secs(5));
    if ready.is_err() {
        let _ = cancel_tx.send(());
        let _ = worker.join();
        panic!("No active source notification: {:?}", ready.as_ref().err());
    }
    let checkpoint = ready.unwrap();
    let journal = ledger.connection.lock().unwrap();
    let _ = write_tx.send(());
    let end = Instant::now() + Duration::from_secs(5);
    while ledger.event_workers.available_permits() != 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let charged = ledger.event_workers.available_permits() == 0;
    let _ = cancel_tx.send(());
    let dropped = dropped_rx.recv_timeout(Duration::from_secs(5));
    let retired = checkpoint.check(&contract).is_err();
    let retained_slot = ledger.event_workers.available_permits() == 0;
    drop(journal);
    let joined = worker.join();
    dropped.unwrap();
    joined.unwrap();
    assert!(
        charged && retained_slot,
        "A cancelled waiter cannot free an occupied blocking-worker slot"
    );
    assert!(retired);
    tokio::time::timeout(Duration::from_secs(5), async {
        while ledger.event_workers.available_permits() != 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let shown = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
        .unwrap();
    assert_eq!(shown["event_count"], 0);
    assert_eq!(shown["state"], "unknown");
    assert!(consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn legacy_v3_consumption_upgrades_without_inventing_events_or_recovery_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let old = definition();
    drop(consume(&ledger, old.clone()).await.unwrap());
    let before = json!(
        ledger
            .workload_receipt(&old.issuer_id, &old.grant_id)
            .unwrap()
    );
    drop(ledger);
    // Construct exact pre-event v3 evidence; v3 never stored event rows.
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE execution_workload_events; DROP TABLE execution_workload_event_state; PRAGMA user_version=3;").unwrap();
    drop(db);
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert_eq!(
        ledger
            .workload_events(&old.issuer_id, &old.grant_id, 0, 128)
            .unwrap()["event_count"],
        0
    );
    let mut fresh = definition();
    fresh.job_id = "e".repeat(32);
    fresh.task_id = "f".repeat(32);
    drop(consume(&ledger, fresh).await.unwrap());
    assert_eq!(version(&path), 4);
    assert_eq!(
        json!(
            ledger
                .workload_receipt(&old.issuer_id, &old.grant_id)
                .unwrap()
        ),
        before
    );
    let shown = ledger
        .workload_events(&old.issuer_id, &old.grant_id, 0, 128)
        .unwrap();
    assert_eq!(shown["event_count"], 0);
    assert_eq!(shown["execution_authorized"], false);
    assert!(consume(&ledger, old).await.is_err());
}

#[tokio::test]
async fn sqlite_event_write_wait_cannot_reset_original_lease_or_commit_late_observation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Retired(Arc<AtomicBool>);
    impl Drop for Retired {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let mut d = definition();
    d.limits.max_lease_ms = 5000;
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    ledger
        .connection
        .lock()
        .unwrap()
        .busy_timeout(Duration::from_secs(7))
        .unwrap();
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let retired = Arc::new(AtomicBool::new(false));
    let observed = retired.clone();
    let owned = Retired(retired);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        let retired_before_io_returns = observed.load(Ordering::SeqCst);
        blocker.execute_batch("COMMIT").unwrap();
        retired_before_io_returns
    });
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let result = consumed
        .supervise(renewals, async move {
            let _owned = owned;
            writer
                .append(
                    stored_event(1, 1, Some(false)),
                    Arc::new(reporting.checkpoint(&contract)?),
                )
                .await?;
            Ok(())
        })
        .await;
    assert!(
        release.join().unwrap(),
        "Source future must retire at original lease expiry before storage unblocks"
    );
    assert!(result.is_err());
    let shown = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
        .unwrap();
    assert_eq!(shown["event_count"], 0);
    assert_eq!(shown["state"], "unknown");
}
