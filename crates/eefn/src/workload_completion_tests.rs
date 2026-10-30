// Real SQLite/final-worker mechanics with explicitly synthetic source metadata.
// The opt-in combined native test separately proves the shipping constructor
// requires real owned root/descendant exit and the exact prepared adapter.
use super::super::tests::{FinalBlocker, definition, policy, proof_for};
use super::*;
use crate::python::stream::{StreamObservation, StreamSummary};

async fn fixture(
    path: &Path,
    remaining_ms: u64,
    omit_last: bool,
) -> (Arc<GrantLedger>, Consumption, Evidence, Definition) {
    let ledger = GrantLedger::open(path, Default::default(), "owner").unwrap();
    let mut d = definition();
    let crate::workload_leases::definition::Source::MicrophoneEnergy { parameters, .. } =
        &mut d.source;
    parameters.max_blocks = 2;
    d.limits.max_source_frames = 160;
    let proof = proof_for(&d, remaining_ms).await;
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
    let writer = consumed.event_writer().unwrap();
    let reporting = consumed.reporting_authority();
    let contract = d.lease_contract().unwrap();
    let source_contract = contract.clone();
    let observed = d.clone();
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let (consumed, finished) = consumed
        .drive(renewals, async move {
            let mut filter = crate::workload_events::Filter::new(&observed)?;
            let start = Instant::now();
            for sequence in 1..=2 {
                let event = filter
                    .observe(
                        StreamObservation {
                            sequence,
                            source_frames: sequence * 80,
                            active: Some(sequence == 2),
                            level: Some(if sequence == 2 { 1200 } else { 200 }),
                            discontinuity: false,
                        },
                        start + Duration::from_millis((sequence - 1) * 100),
                    )?
                    .unwrap();
                if sequence != 2 || !omit_last {
                    writer
                        .append(event, Arc::new(reporting.checkpoint(&source_contract)?))
                        .await?;
                }
            }
            filter.seal(&StreamSummary {
                stream_id: observed.grant_id,
                observations: 2,
                source_frames: 160,
            })
        })
        .await
        .unwrap();
    let crate::workload_leases::definition::Source::MicrophoneEnergy { adapter_sha256, .. } =
        &d.source;
    let evidence = Evidence {
        contract,
        boundary: finished.boundary,
        adapter_sha256: adapter_sha256.clone(),
        observations: 2,
        frames: 160,
        events: finished.value,
    };
    (ledger, consumed, evidence, d)
}
async fn persist(mut consumed: Consumption, evidence: Evidence) -> Result<Receipt> {
    consumed.reporting.retire();
    let slot = consumed.final_slot.take().unwrap();
    let stopped = consumed.stop.receiver.clone();
    finalization::start_with(&consumed.ledger, slot, move |journal| {
        save(journal, evidence, stopped)
    })?
    .wait()
    .await?
}
fn current_version(ledger: &GrantLedger) -> i64 {
    ledger
        .connection
        .lock()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn completed_source_and_prefix_reopen_without_replay_or_ordinary_output_authority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let (ledger, consumed, evidence, d) = fixture(&path, 50_000, false).await;
    let old = crate::grants::tests::grant();
    ledger.claim(&old).unwrap();
    let old_report = GrantReport::new(
        &old,
        ReceiptState::Unknown,
        Value::Null,
        Some("synthetic_uncertainty"),
    );
    ledger.finish(&old, &old_report).unwrap();
    let old_digest = definition_digest(&json!(old)).unwrap();
    let before = json!(
        ledger
            .result(&old.issuer_id, &old.grant_id, &old_digest)
            .unwrap()
    );
    let done = persist(consumed, evidence).await.unwrap();
    assert_eq!(done.receipt.state, "completed");
    assert_eq!(current_version(&ledger), 5);
    let report = done.completion.as_ref().unwrap();
    assert_eq!(
        (
            report.source_observations,
            report.source_frames,
            report.event_count
        ),
        (2, 160, 2)
    );
    assert!(report.owned_process_exit_observed && !report.physical_effects_stopped);
    assert!(!done.execution_authorized && !done.restart_authorized);
    let reopened = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert_eq!(
        json!(
            reopened
                .workload_receipt(&d.issuer_id, &d.grant_id)
                .unwrap()
        ),
        json!(done)
    );
    assert_eq!(
        json!(
            reopened
                .result(&old.issuer_id, &old.grant_id, &old_digest)
                .unwrap()
        ),
        before
    );
    assert!(
        reopened
            .result(&d.issuer_id, &d.grant_id, &d.scope().unwrap().grant_sha256)
            .is_err()
    );
    assert!(
        super::super::tests::consume(&reopened, d.clone())
            .await
            .is_err()
    );
    let shown = reopened
        .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
        .unwrap();
    assert_eq!(shown["state"], "completed");
    assert_eq!(shown["owned_process_exit_observed"], true);
    assert_eq!(shown["source_termination_confirmed"], false);
}

#[tokio::test]
async fn missing_required_event_cannot_be_laundered_by_a_finished_source_summary() {
    let dir = tempfile::tempdir().unwrap();
    let (ledger, consumed, evidence, d) =
        fixture(&dir.path().join("receipts.db"), 50_000, true).await;
    assert!(persist(consumed, evidence).await.is_err());
    assert_eq!(current_version(&ledger), 4);
    let saved = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
    assert_eq!(saved.receipt.state, "accepted");
    assert!(saved.completion.is_none());
    assert!(
        ledger
            .final_reporting_failed
            .load(std::sync::atomic::Ordering::Acquire)
    );
}

#[tokio::test]
async fn changed_adapter_counts_or_contract_refuse_without_completion_or_schema_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..3 {
        let (ledger, consumed, mut evidence, d) = fixture(
            &dir.path().join(format!("changed-{index}.db")),
            50_000,
            false,
        )
        .await;
        match index {
            0 => evidence.adapter_sha256 = "f".repeat(64),
            1 => evidence.frames += 80,
            _ => evidence.contract.scope.task_id = "e".repeat(32),
        }
        assert!(persist(consumed, evidence).await.is_err());
        assert_eq!(current_version(&ledger), 4);
        let saved = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
        assert_eq!(saved.receipt.state, "accepted");
        assert!(saved.completion.is_none());
    }
}

#[tokio::test]
async fn durable_stop_on_an_independent_handle_keeps_finished_work_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let (ledger, consumed, evidence, d) = fixture(&path, 50_000, false).await;
    let independent = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    independent
        .request_stop_scope(&d.scope().unwrap(), &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(
        !*consumed.stop.receiver.borrow(),
        "Independent registries require the durable fence"
    );
    assert!(persist(consumed, evidence).await.is_err());
    assert_eq!(current_version(&ledger), 4);
    let saved = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
    assert_eq!(saved.receipt.state, "unknown");
    assert!(saved.completion.is_none());
    assert!(
        ledger.workload_reporting_ready(),
        "Confirmed uncertainty is not a storage failure"
    );
}

#[tokio::test]
async fn failed_completion_commit_rolls_back_version_report_and_state_without_retry() {
    let dir = tempfile::tempdir().unwrap();
    let (ledger, consumed, evidence, d) =
        fixture(&dir.path().join("receipts.db"), 50_000, false).await;
    let before = ledger
        .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
        .unwrap();
    ledger.connection.lock().unwrap().execute_batch("CREATE TRIGGER refuse_completion BEFORE UPDATE ON execution_receipts WHEN NEW.state='completed' BEGIN SELECT RAISE(ABORT,'synthetic commit failure'); END;").unwrap();
    assert!(persist(consumed, evidence).await.is_err());
    assert_eq!(current_version(&ledger), 4);
    assert_eq!(
        ledger
            .workload_events(&d.issuer_id, &d.grant_id, 0, 128)
            .unwrap(),
        before
    );
    assert!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .completion
            .is_none()
    );
    assert!(!ledger.workload_reporting_ready());
}

#[tokio::test]
async fn blocked_historical_completion_is_bounded_without_retaining_source_authority() {
    let dir = tempfile::tempdir().unwrap();
    let (ledger, consumed, evidence, d) =
        fixture(&dir.path().join("receipts.db"), 3000, false).await;
    let reporting = consumed.reporting_authority();
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    let pending = tokio::time::timeout(Duration::from_secs(4), persist(consumed, evidence))
        .await
        .unwrap();
    assert!(
        pending
            .unwrap_err()
            .to_string()
            .contains("Final workload checkpoint still pending")
    );
    assert!(reporting.checkpoint(&d.lease_contract().unwrap()).is_err());
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert_eq!(blocker.release(), (true, "accepted".into()));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !ledger.workload_reporting_ready() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let saved = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
    assert_eq!(saved.receipt.state, "completed");
    assert!(!saved.execution_authorized && !saved.restart_authorized);
    assert!(super::super::tests::consume(&ledger, d).await.is_err());
}

#[tokio::test]
async fn cancelled_completion_waiter_keeps_capacity_until_actual_historical_save() {
    let dir = tempfile::tempdir().unwrap();
    let (ledger, consumed, evidence, d) =
        fixture(&dir.path().join("receipts.db"), 50_000, false).await;
    let blocker = FinalBlocker::hold(&ledger, &d).await;
    {
        let pending = persist(consumed, evidence);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut pending)
                .await
                .is_err()
        );
    }
    assert_eq!(ledger.final_slots.available_permits(), 0);
    assert_eq!(blocker.release(), (true, "accepted".into()));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !ledger.workload_reporting_ready() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "completed"
    );
}

#[tokio::test]
async fn damaged_completion_or_downgrade_is_refused_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..14 {
        let path = dir.path().join(format!("damaged-{index}.db"));
        let (ledger, consumed, evidence, _) = fixture(&path, 50_000, false).await;
        let done = persist(consumed, evidence).await.unwrap();
        if index < 6 {
            ledger
                .connection
                .lock()
                .unwrap()
                .execute_batch(
                    [
                        "PRAGMA user_version=4",
                        "UPDATE execution_receipts SET report=NULL",
                        "UPDATE execution_receipts SET report=hex(zeroblob(8192))",
                        "UPDATE execution_receipts SET report_sha256='broken'",
                        "UPDATE execution_receipts SET state='accepted'",
                        "DELETE FROM execution_workload_events WHERE sequence=2",
                    ][index],
                )
                .unwrap();
        } else {
            let mut report = done.completion.unwrap();
            match index {
                6 => report.source_observations += 1,
                7 => report.source_frames += 80,
                8 => report.event_prefix_sha256 = "a".repeat(64),
                9 => report.event_count -= 1,
                10 => report.event_bytes += 1,
                11 => report.lease_sequence = 0,
                12 => report.owned_process_exit_observed = false,
                _ => report.physical_effects_stopped = true,
            }
            let raw = serde_json::to_string(&report).unwrap();
            ledger.connection.lock().unwrap().execute("UPDATE execution_receipts SET report=?1,report_sha256=?2 WHERE receipt_kind='workload'",
                params![raw,hex::encode(Sha256::digest(raw.as_bytes()))]).unwrap();
        }
        drop(ledger);
        let before = Sha256::digest(std::fs::read(&path).unwrap());
        assert!(
            GrantLedger::open(&path, Default::default(), "owner").is_err(),
            "Corruption {index}"
        );
        assert_eq!(
            Sha256::digest(std::fs::read(&path).unwrap()),
            before,
            "No repair for {index}"
        );
    }
}
