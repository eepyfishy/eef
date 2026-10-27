use super::*;
fn stop(ledger: &GrantLedger, g: &ExecutionGrant) -> Result<Report> {
    ledger.request_stop(g, &g.issuer_id, &g.session_id, &g.executor_node)
}
fn engine(dir: &Path) -> crate::NodeEngine {
    crate::NodeEngine::new(false, vec![], dir.into()).unwrap()
}
#[test]
fn stop_before_admission_consumes_authority_durably_without_execution_or_fake_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let g = super::super::tests::grant();
    let report = stop(&ledger, &g).unwrap();
    assert!(report.confirms(&g));
    assert!(!report.receipt_preexisted);
    assert!(!report.cancellation_signalled);
    assert!(!report.reused);
    assert!(ledger.claim(&g).is_err());
    let repeated = stop(&ledger, &g).unwrap();
    assert!(repeated.reused);
    assert_eq!(repeated.recorded_at_ms, report.recorded_at_ms);
    let result = ledger
        .result(
            &g.issuer_id,
            &g.grant_id,
            &definition_digest(&json!(g)).unwrap(),
        )
        .unwrap();
    assert_eq!(result["receipt"]["state"], "unknown");
    assert!(result["report"].is_null());
    drop(ledger);
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    assert!(ledger.claim(&g).is_err());
    let mut retagged = g.clone();
    retagged.grant_id = uuid::Uuid::new_v4().to_string();
    retagged.session_id = uuid::Uuid::new_v4().to_string();
    assert!(
        ledger.claim(&retagged).is_err(),
        "a stopped logical attempt cannot acquire replacement authority"
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("PRAGMA user_version", [], |r| r.get(0))
            .unwrap(),
        2
    );
    crate::request_receipt::read_consumed(&db, "owner", Some(&g.issuer_id), &g.grant_id).unwrap();
}
#[tokio::test]
async fn stop_after_claim_signals_shared_executor_and_retains_unknown_consumption() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let g = super::super::tests::grant();
    let engine = engine(dir.path());
    let accepted = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        "owner",
    )
    .unwrap();
    let report = stop(&ledger, &g).unwrap();
    assert!(report.receipt_preexisted);
    assert!(report.cancellation_signalled);
    let final_report = accepted.run(&engine).await;
    assert_eq!(final_report.state, ReceiptState::Unknown);
    assert!(final_report.persisted);
    assert_eq!(
        final_report.error_code.as_deref(),
        Some("grant_stop_requested_effects_unknown")
    );
    assert!(final_report.data.is_null());
    assert!(final_report.confirms(&g));
    assert!(ledger.claim(&g).is_err());
}
#[tokio::test]
async fn stop_of_completed_work_preserves_original_result_and_never_replays() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let g = super::super::tests::grant();
    let engine = engine(dir.path());
    let report = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        "owner",
    )
    .unwrap()
    .run(&engine)
    .await;
    assert_eq!(report.state, ReceiptState::Completed);
    let before = ledger
        .result(
            &g.issuer_id,
            &g.grant_id,
            &definition_digest(&json!(g)).unwrap(),
        )
        .unwrap();
    let receipt = stop(&ledger, &g).unwrap();
    assert!(receipt.receipt_preexisted);
    assert!(!receipt.cancellation_signalled);
    assert_eq!(
        before,
        ledger
            .result(
                &g.issuer_id,
                &g.grant_id,
                &definition_digest(&json!(g)).unwrap()
            )
            .unwrap()
    );
    assert!(ledger.claim(&g).is_err());
}
#[test]
fn stop_scope_and_receipt_substitution_cannot_target_another_session_or_claim_effect_stop() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let g = super::super::tests::grant();
    for (issuer, session, node) in [
        ("b".repeat(64), g.session_id.clone(), "owner"),
        (
            g.issuer_id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "owner",
        ),
        (g.issuer_id.clone(), g.session_id.clone(), "other"),
    ] {
        assert!(ledger.request_stop(&g, &issuer, &session, node).is_err());
    }
    assert_eq!(
        ledger
            .connection
            .lock()
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        0
    );
    let proof = stop(&ledger, &g).unwrap();
    for field in [
        "execution_stopped",
        "external_effects_stopped",
        "execution_authorized",
        "automatically_retried",
    ] {
        let mut bad = json!(proof);
        bad[field] = json!(true);
        assert!(!serde_json::from_value::<Report>(bad).unwrap().confirms(&g));
    }
    let mut altered = g.clone();
    altered.params = json!({"changed":true});
    assert!(stop(&ledger, &altered).is_err());
    assert!(
        serde_json::from_value::<WireRequest>(
            json!({"type":"stop_grant","id":"a".repeat(32),"scope":Scope::from_grant(&g).unwrap(),"pid":1})
        )
        .is_err()
    );
}
#[test]
fn stop_claim_then_watch_race_observes_the_retained_stop_and_registry_retires() {
    let dir = tempfile::tempdir().unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let g = super::super::tests::grant();
    ledger.claim(&g).unwrap();
    stop(&ledger, &g).unwrap();
    let watch = ledger.watch_stop(&g).unwrap();
    assert!(*watch.receiver.borrow());
    assert_eq!(ledger.stops.0.lock().unwrap().len(), 1);
    drop(watch);
    assert!(ledger.stops.0.lock().unwrap().is_empty());
}
#[test]
fn stop_quota_and_corrupt_adjunct_fail_closed_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipts.db");
    let mut policy = GrantPolicy::default();
    policy.max_receipts = 1;
    let ledger = GrantLedger::open(&path, policy, "owner").unwrap();
    let g = super::super::tests::grant();
    stop(&ledger, &g).unwrap();
    let other = super::super::tests::grant();
    assert!(stop(&ledger, &other).is_err());
    assert_eq!(
        ledger
            .connection
            .lock()
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM execution_receipts", [], |r| r.get(0))
            .unwrap(),
        1
    );
    drop(ledger);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("UPDATE execution_stop_meta SET schema_version=99;")
        .unwrap();
    drop(db);
    let bytes = std::fs::read(&path).unwrap();
    assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
    assert_eq!(bytes, std::fs::read(&path).unwrap());
}
#[cfg(windows)]
#[tokio::test]
async fn stop_native_process_uses_owned_tree_cleanup_before_original_deadline() {
    use crate::process_lifetime::tests::{Processes, args};
    let dir = tempfile::tempdir().unwrap();
    let policy: crate::NodePolicy = serde_json::from_value(
        json!({"process":{"enabled":true,"max_seconds":30},"full_access":["process.exec"]}),
    )
    .unwrap();
    let engine = engine(dir.path()).with_policy(policy).unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let mut g = super::super::tests::grant();
    g.capability = "process.exec".into();
    g.params = json!({"program":std::env::current_exe().unwrap(),"args":args("loop",dir.path()),"timeout_seconds":20});
    g.limits.max_execution_ms = 10_000;
    let accepted = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        "owner",
    )
    .unwrap();
    let task = tokio::spawn(async move { accepted.run(&engine).await });
    let processes = Processes::observe(dir.path()).await;
    processes.alive();
    let start = Instant::now();
    let proof = stop(&ledger, &g).unwrap();
    assert!(proof.cancellation_signalled);
    assert!(!proof.execution_stopped);
    let report = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !processes.exited_now() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(report.state, ReceiptState::Completed);
    assert!(!report.success);
    assert_eq!(
        report.error_code.as_deref(),
        Some("process_exit_unsuccessful")
    );
    assert!(report.confirms(&g));
    assert!(crate::process_stop::confirmed(&g, &proof, &report).is_some());
    assert!(!proof.external_effects_stopped);
    assert!(!proof.execution_stopped);
    let retained: crate::request_result::RetainedResult = serde_json::from_value(
        ledger
            .result(
                &g.issuer_id,
                &g.grant_id,
                &definition_digest(&json!(g)).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(retained.report.unwrap()).unwrap(),
        serde_json::to_value(&report).unwrap()
    );
}

#[test]
fn stop_unversioned_or_partial_evidence_is_not_adopted_or_initialized() {
    for partial in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.db");
        let db = Connection::open(&path).unwrap();
        if partial {
            db.execute_batch("CREATE TABLE execution_stop_meta(schema_version INTEGER); INSERT INTO execution_stop_meta VALUES(1);").unwrap();
        } else {
            initialize(&db).unwrap();
        }
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(GrantLedger::open(&path, Default::default(), "owner").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_master WHERE name='execution_receipts'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            0
        );
    }
}

#[test]
fn stop_control_scope_is_bounded_and_never_resends_original_private_payload() {
    let mut g = super::super::tests::grant();
    let original = Scope::from_grant(&g).unwrap();
    g.params = json!({"text":"PRIVATE_STOP_PAYLOAD".repeat(128)});
    let scope = Scope::from_grant(&g).unwrap();
    scope.validate().unwrap();
    assert_ne!(scope.grant_sha256, original.grant_sha256);
    let wire = json!({"type":"stop_grant","id":"a".repeat(32),"scope":scope});
    let bytes = serde_json::to_vec(&wire).unwrap();
    assert!(bytes.len() < 1024);
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains("PRIVATE_STOP_PAYLOAD")
    );
    serde_json::from_value::<WireRequest>(wire.clone())
        .unwrap()
        .validate(&g.issuer_id, &g.session_id, &g.executor_node)
        .unwrap();
    let mut bad = wire;
    bad["scope"]["grant_sha256"] = json!("bad");
    assert!(
        serde_json::from_value::<WireRequest>(bad)
            .unwrap()
            .validate(&g.issuer_id, &g.session_id, &g.executor_node)
            .is_err()
    );
}

#[cfg(windows)]
fn native_process_fixture(dir: &Path) -> (crate::NodeEngine, Arc<GrantLedger>, ExecutionGrant) {
    let program = std::env::current_exe().unwrap();
    let policy: crate::NodePolicy = serde_json::from_value(
        json!({"process":{"enabled":true,"max_seconds":30,"allowed_executables":[program]}}),
    )
    .unwrap();
    let runner = engine(dir).with_policy(policy).unwrap();
    let ledger = GrantLedger::open(&dir.join("receipts.db"), Default::default(), "owner").unwrap();
    let mut grant = super::super::tests::grant();
    grant.capability = "process.exec".into();
    grant.params =
        json!({"program":program,"args":crate::process_lifetime::tests::args("loop",dir)});
    grant.limits.max_execution_ms = 10_000;
    (runner, ledger, grant)
}
#[cfg(windows)]
#[tokio::test]
async fn process_stop_before_first_poll_does_not_launch_or_invent_a_tree_observation() {
    let dir = tempfile::tempdir().unwrap();
    let (runner, ledger, grant) = native_process_fixture(dir.path());
    let accepted = AcceptedGrant::prepare(
        grant.clone(),
        ledger.clone(),
        &runner,
        &grant.issuer_id,
        &grant.session_id,
        "owner",
    )
    .unwrap();
    let intent = stop(&ledger, &grant).unwrap();
    let report = accepted.run(&runner).await;
    assert_eq!(report.state, ReceiptState::Unknown);
    assert_eq!(
        report.error_code.as_deref(),
        Some("grant_stop_requested_effects_unknown")
    );
    assert!(report.data.is_null());
    assert!(!dir.path().join("pids.json").exists());
    assert!(crate::process_stop::confirmed(&grant, &intent, &report).is_none());
    assert!(
        AcceptedGrant::prepare(
            grant.clone(),
            ledger,
            &runner,
            &grant.issuer_id,
            &grant.session_id,
            "owner"
        )
        .is_err()
    );
}
#[cfg(windows)]
#[tokio::test]
async fn owned_tree_exits_while_stop_journal_is_held_but_confirmation_waits_for_checkpoint() {
    use crate::process_lifetime::tests::Processes;
    let dir = tempfile::tempdir().unwrap();
    let (runner, ledger, grant) = native_process_fixture(dir.path());
    let accepted = AcceptedGrant::prepare(
        grant.clone(),
        ledger.clone(),
        &runner,
        &grant.issuer_id,
        &grant.session_id,
        "owner",
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let report = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(accepted.run(&runner));
        let _ = tx.send(());
        report
    });
    let processes = Processes::observe(dir.path()).await;
    processes.alive();
    let mut exited_while_held = false;
    let mut reply_before_release = false;
    let scope = Scope::from_grant(&grant).unwrap();
    let intent = ledger
        .request_stop_scope_after_signal(
            &scope,
            &grant.issuer_id,
            &grant.session_id,
            "owner",
            || {
                let end = Instant::now() + Duration::from_secs(2);
                while Instant::now() < end && !processes.exited_now() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                exited_while_held = processes.exited_now();
                reply_before_release = rx.try_recv().is_ok();
            },
        )
        .unwrap();
    let report = worker.join().unwrap();
    assert!(
        exited_while_held,
        "actual held root/descendant handles must be exited before journal release"
    );
    assert!(
        !reply_before_release,
        "no durable-result acknowledgement before final checkpoint"
    );
    assert!(crate::process_stop::confirmed(&grant, &intent, &report).is_some());
    assert!(!intent.execution_stopped);
    assert!(!intent.external_effects_stopped);
    let retained: crate::request_result::RetainedResult = serde_json::from_value(
        ledger
            .result(
                &grant.issuer_id,
                &grant.grant_id,
                &definition_digest(&json!(grant)).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json!(retained.report.unwrap()), json!(report));
}
#[cfg(windows)]
#[tokio::test]
async fn completed_process_is_not_relabelled_as_a_stop_observation() {
    use crate::process_lifetime::tests::{Processes, args};
    let dir = tempfile::tempdir().unwrap();
    let (runner, ledger, mut grant) = native_process_fixture(dir.path());
    grant.params["args"] = json!(args("exit", dir.path()));
    let accepted = AcceptedGrant::prepare(
        grant.clone(),
        ledger.clone(),
        &runner,
        &grant.issuer_id,
        &grant.session_id,
        "owner",
    )
    .unwrap();
    let task = tokio::spawn(async move { accepted.run(&runner).await });
    let processes = Processes::observe(dir.path()).await;
    std::fs::write(dir.path().join("release"), b"fixture").unwrap();
    let report = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(report.success);
    assert!(processes.exited_now());
    let intent = stop(&ledger, &grant).unwrap();
    assert!(!intent.cancellation_signalled);
    assert!(crate::process_stop::confirmed(&grant, &intent, &report).is_none());
    let retained: crate::request_result::RetainedResult = serde_json::from_value(
        ledger
            .result(
                &grant.issuer_id,
                &grant.grant_id,
                &definition_digest(&json!(grant)).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json!(retained.report.unwrap()), json!(report));
}
