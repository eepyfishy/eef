use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::time::Duration;
fn definition() -> Definition {
    crate::workload_leases::definition::vision::tests::vision()
}
fn image() -> crate::python::vision::PrivateJpeg {
    crate::python::vision::PrivateJpeg::checked(
        STANDARD.encode([0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9]),
        128,
    )
    .unwrap()
}
async fn consume(ledger: &Arc<GrantLedger>, d: Definition) -> Result<Consumption> {
    super::super::tests::consume(ledger, d).await
}
fn ledger(path: &Path) -> Arc<GrantLedger> {
    GrantLedger::open(path, Default::default(), "owner").unwrap()
}

#[tokio::test]
async fn vision_intent_precedes_effects_and_bounded_result_reopens_without_replay_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = ledger(&path);
    let d = definition();
    let contract = d.lease_contract().unwrap();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let saved = consumed
        .supervise(renewals, async {
            let image = image();
            let permit = writer
                .begin_analysis(2, &image, Arc::new(reporting.checkpoint(&contract)?))
                .await?;
            let db = Connection::open(&path)?;
            let raw: String =
                db.query_row("SELECT intent FROM execution_workload_analyses", [], |r| {
                    r.get(0)
                })?;
            assert!(!raw.contains(image.as_base64()));
            assert!(raw.contains(image.digest()));
            let pending: i64 = db.query_row(
                "SELECT count(*) FROM execution_workload_analyses WHERE result IS NULL",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(
                pending, 1,
                "Model effects must follow durable pending intent"
            );
            let summary = writer
                .finish_analysis(
                    permit,
                    "movement detected".into(),
                    Arc::new(reporting.checkpoint(&contract)?),
                )
                .await?;
            assert_eq!(
                (
                    summary.calls,
                    summary.completed,
                    summary.result_bytes,
                    summary.reserved_output_tokens
                ),
                (1, 1, 19, 32)
            );
            Ok(summary)
        })
        .await
        .unwrap();
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let history = reopened
        .workload_analyses(&d.issuer_id, &d.grant_id, 0, 1)
        .unwrap();
    assert_eq!(history["state"], "unknown");
    assert_eq!(history["summary"], json!(saved));
    assert_eq!(
        history["analyses"][0]["result"]["text"],
        "movement detected"
    );
    for key in [
        "execution_authorized",
        "restart_authorized",
        "model_replay_authorized",
    ] {
        assert_eq!(history[key], false)
    }
    assert!(consume(&reopened, d.clone()).await.is_err());
    let mut retry = d.clone();
    retry.grant_id = uuid::Uuid::new_v4().to_string();
    retry.attempt = 2;
    assert!(consume(&reopened, retry).await.is_err());
    assert!(
        reopened
            .workload_analyses(&"b".repeat(64), &d.grant_id, 0, 1)
            .is_err()
    );
}

#[tokio::test]
async fn vision_pending_intent_refuses_duplicate_and_later_analysis_without_reusing_a_permit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = ledger(&path);
    let mut d = definition();
    if let Source::CameraVision { parameters, .. } = &mut d.source {
        parameters.images.max_images = 2
    }
    let contract = d.lease_contract().unwrap();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            let _lost = writer
                .begin_analysis(2, &image(), Arc::new(reporting.checkpoint(&contract)?))
                .await?;
            for position in [2, 3] {
                assert!(
                    writer
                        .begin_analysis(
                            position,
                            &image(),
                            Arc::new(reporting.checkpoint(&contract)?)
                        )
                        .await
                        .is_err()
                );
            }
            Ok(())
        })
        .await
        .unwrap();
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let history = reopened
        .workload_analyses(&d.issuer_id, &d.grant_id, 0, 2)
        .unwrap();
    assert_eq!(history["summary"]["calls"], 1);
    assert_eq!(history["summary"]["completed"], 0);
    assert_eq!(history["analyses"][0]["result"], Value::Null);
    assert!(consume(&reopened, d).await.is_err());
}

#[tokio::test]
async fn vision_result_and_call_budgets_refuse_widening_without_rewriting_pending_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = ledger(&path);
    let d = definition();
    let contract = d.lease_contract().unwrap();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    consumed
        .supervise(renewals, async {
            assert!(
                writer
                    .begin_analysis(1, &image(), Arc::new(reporting.checkpoint(&contract)?))
                    .await
                    .is_err()
            );
            let permit = writer
                .begin_analysis(2, &image(), Arc::new(reporting.checkpoint(&contract)?))
                .await?;
            assert!(
                writer
                    .finish_analysis(
                        permit,
                        "x".repeat(257),
                        Arc::new(reporting.checkpoint(&contract)?)
                    )
                    .await
                    .is_err()
            );
            assert!(
                writer
                    .begin_analysis(3, &image(), Arc::new(reporting.checkpoint(&contract)?))
                    .await
                    .is_err()
            );
            Ok(())
        })
        .await
        .unwrap();
    let value = ledger
        .workload_analyses(&d.issuer_id, &d.grant_id, 0, 1)
        .unwrap();
    assert_eq!(value["summary"]["completed"], 0);
    assert_eq!(value["analyses"][0]["result"], Value::Null);
}

#[tokio::test]
async fn vision_journal_corruption_and_schema_downgrade_are_retained_without_repair() {
    for mutation in [
        "UPDATE execution_workload_analyses SET intent_sha256=replace(intent_sha256,substr(intent_sha256,1,1),'z')",
        "UPDATE execution_workload_analyses SET source_sequence=3",
        "UPDATE execution_workload_analyses SET result_sha256='bad'",
        "DELETE FROM execution_workload_analyses",
        "DELETE FROM execution_workload_analysis_state",
        "UPDATE execution_workload_analysis_state SET call_count=0",
        "UPDATE execution_workload_analysis_state SET prefix_sha256=replace(prefix_sha256,substr(prefix_sha256,1,1),'z')",
        "PRAGMA user_version=5",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let ledger = ledger(&path);
        let d = definition();
        let contract = d.lease_contract().unwrap();
        let consumed = consume(&ledger, d.clone()).await.unwrap();
        let writer = consumed.event_writer().unwrap();
        let reporting = consumed.reporting_authority();
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        consumed
            .supervise(renewals, async {
                let permit = writer
                    .begin_analysis(2, &image(), Arc::new(reporting.checkpoint(&contract)?))
                    .await?;
                writer
                    .finish_analysis(
                        permit,
                        "movement detected".into(),
                        Arc::new(reporting.checkpoint(&contract)?),
                    )
                    .await?;
                Ok(())
            })
            .await
            .unwrap();
        let corruptor = Connection::open(&path).unwrap();
        corruptor.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        corruptor.execute_batch(mutation).unwrap();
        assert!(ledger.workload_receipt(&d.issuer_id, &d.grant_id).is_err());
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        let count: i64 = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_workload_analyses",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count,
            if mutation == "DELETE FROM execution_workload_analyses" {
                0
            } else {
                1
            }
        );
    }
}

#[tokio::test]
async fn vision_admission_reserves_storage_atomically_and_metadata_workloads_keep_their_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.db");
    let policy = GrantPolicy {
        max_journal_bytes: 64 * 1024,
        ..Default::default()
    };
    let small = GrantLedger::open(&path, policy, "owner").unwrap();
    assert!(consume(&small, definition()).await.is_err());
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_master WHERE name='execution_workload_analyses'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    let result_limited = GrantLedger::open(
        &dir.path().join("result-limited.db"),
        GrantPolicy {
            max_result_bytes: 32768,
            ..Default::default()
        },
        "owner",
    )
    .unwrap();
    // Metadata alone fits this owner bound, but approved analysis must fit too.
    assert!(consume(&result_limited, definition()).await.is_err());
    let storage_path = dir.path().join("reserved.db");
    let reserved = GrantLedger::open(
        &storage_path,
        GrantPolicy {
            max_journal_bytes: 2 * 1024 * 1024,
            max_result_bytes: 1024 * 1024,
            ..Default::default()
        },
        "owner",
    )
    .unwrap();
    let mut small_metadata = crate::workload_leases::definition::camera_tests::camera();
    small_metadata.events.max_total_bytes = 1024;
    let consumed = consume(&reserved, small_metadata).await.unwrap();
    consumed.retire().await.unwrap();
    let before: i64 = Connection::open(&storage_path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let mut expensive = definition();
    expensive.events.max_total_bytes = 1024;
    expensive.limits.max_source_frames = 100;
    if let Source::CameraVision { parameters, .. } = &mut expensive.source {
        parameters.capture.max_frames = 100;
        parameters.images.max_images = 100;
        parameters.inference.max_result_bytes = 4096;
    }
    assert!(consume(&reserved, expensive).await.is_err());
    assert_eq!(
        Connection::open(&storage_path)
            .unwrap()
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        before
    );
    let path = dir.path().join("mixed.db");
    let ledger = ledger(&path);
    let consumed = consume(&ledger, definition()).await.unwrap();
    consumed.retire().await.unwrap();
    let legacy = super::super::tests::definition();
    let original = json!(legacy);
    let consumed = consume(&ledger, legacy.clone()).await.unwrap();
    consumed.retire().await.unwrap();
    let receipt = ledger
        .workload_receipt(&legacy.issuer_id, &legacy.grant_id)
        .unwrap();
    assert_eq!(json!(receipt.definition), original);
    assert_eq!(receipt.schema_version, 2);
    assert!(
        ledger
            .workload_analyses(&legacy.issuer_id, &legacy.grant_id, 0, 1)
            .is_err()
    );
    assert_eq!(
        Connection::open(path)
            .unwrap()
            .query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        6
    );
}

#[tokio::test]
async fn cancelled_vision_write_keeps_native_capacity_until_actual_work_retires() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = ledger(&path);
    let d = definition();
    let contract = d.lease_contract().unwrap();
    let mut consumed = consume(&ledger, d.clone()).await.unwrap();
    consumed
        .initial
        .as_mut()
        .unwrap()
        .restrict_deadlines_for_test(
            Instant::now() + Duration::from_secs(2),
            Instant::now() + Duration::from_secs(2),
        );
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let acknowledged = std::sync::atomic::AtomicBool::new(false);
    let (_renew, renewals) = crate::workload_leases::renewal_channel();
    let run = consumed.supervise(renewals, async {
        let checkpoint = Arc::new(reporting.checkpoint(&contract)?);
        let held = ledger.connection.lock().unwrap();
        assert!(sender.send((held, checkpoint.deadline())).is_ok());
        writer.begin_analysis(2, &image(), checkpoint).await?;
        acknowledged.store(true, std::sync::atomic::Ordering::SeqCst);
        Err::<(), anyhow::Error>(anyhow::anyhow!(
            "Expired analysis obtained an intent acknowledgment"
        ))
    });
    let release = async {
        let (held, deadline) = receiver.await.unwrap();
        tokio::time::sleep_until(
            tokio::time::Instant::from_std(deadline) + Duration::from_millis(100),
        )
        .await;
        assert_eq!(ledger.event_workers.available_permits(), 0);
        drop(held);
    };
    let (result, ()) =
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(run, release) })
            .await
            .unwrap();
    assert!(result.is_err());
    assert!(!acknowledged.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(ledger.event_workers.available_permits(), 1);
    let history = ledger
        .workload_analyses(&d.issuer_id, &d.grant_id, 0, 1)
        .unwrap();
    assert_eq!(history["summary"]["calls"], 0);
    assert_eq!(history["state"], "unknown");
}
