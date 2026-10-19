//! Final checkpoint contention must not retain the executing future or its tree.
use super::*;
use std::sync::mpsc;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Completed,
    Failed,
    Deadline,
    Permission,
}
struct Dropped(mpsc::Sender<()>);
impl Drop for Dropped {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn ordering(mode: Mode) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("node.json");
    let policy: crate::NodePolicy = serde_json::from_value(
        json!({"filesystem":{"write":true},"full_access":["filesystem.write"]}),
    )
    .unwrap();
    crate::setup::write_json(&config, &json!({"node_id":"owner","permissions":policy})).unwrap();
    let service = crate::NodeService::new(config, "owner".into());
    let engine = crate::NodeEngine::new(false, vec![], dir.path().into())
        .unwrap()
        .with_policy(policy.clone())
        .unwrap()
        .with_service(service.clone());
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let mut g = super::tests::grant();
    if matches!(mode, Mode::Deadline) {
        g.limits.max_execution_ms = 1000;
    }
    if matches!(mode, Mode::Permission) {
        g.capability = "filesystem".into();
        g.action = "write".into();
        g.params =
            json!({"path":dir.path().join("not-executed"),"content":"synthetic future only"});
    }
    let accepted = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        "owner",
    )
    .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let drop_guard = Dropped(dropped_tx);
    // Hold the actual journal mutex, not a sleep pretending that storage is slow.
    // The executor thread can observe policy/deadline but cannot finish its save.
    let journal = ledger.connection.lock().unwrap();
    let worker = std::thread::spawn(move || {
        runtime().block_on(async move {
            accepted
                .run_operation(&engine, async move {
                    let _guard = drop_guard;
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                    if matches!(mode, Mode::Failed) {
                        bail!("PRIVATE_EXECUTOR_FAILURE")
                    }
                    Ok(json!({"synthetic":true}))
                })
                .await
        })
    });
    let started = started_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    if matches!(mode, Mode::Completed | Mode::Failed) {
        let _ = release_tx.send(());
    } else if matches!(mode, Mode::Permission) {
        service
            .save_configuration(json!({"node_id":"owner","permissions":{}}))
            .unwrap();
    }
    let dropped_before_save = dropped_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    let not_returned_before_save = !worker.is_finished();
    let state: String = journal
        .query_row(
            "SELECT state FROM execution_receipts WHERE grant_id=?1",
            [&g.grant_id],
            |r| r.get(0),
        )
        .unwrap();
    // Always release/join before asserting, including against the buggy version.
    drop(journal);
    let report = worker.join().unwrap();
    assert!(started, "operation must actually poll: {mode:?}");
    assert!(
        dropped_before_save,
        "executor retained while final checkpoint blocks: {mode:?}"
    );
    assert!(not_returned_before_save);
    assert_eq!(state, "accepted");
    assert!(report.confirms(&g));
    assert!(report.persisted);
    assert_eq!(
        report.state,
        if matches!(mode, Mode::Completed) {
            ReceiptState::Completed
        } else {
            ReceiptState::Unknown
        }
    );
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("PRIVATE_EXECUTOR_FAILURE")
    );
    if matches!(mode, Mode::Permission) {
        assert_eq!(report.error_code.as_deref(), Some("local_policy_changed"));
    }
    let receipt = ledger
        .result(
            &g.issuer_id,
            &g.grant_id,
            &definition_digest(&json!(g)).unwrap(),
        )
        .unwrap();
    assert_eq!(receipt["report"], json!(report));
    let fresh = crate::NodeEngine::new(false, vec![], dir.path().into())
        .unwrap()
        .with_policy(policy)
        .unwrap();
    fresh.check_grant_admission(&g).unwrap();
    assert!(
        AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &fresh,
            &g.issuer_id,
            &g.session_id,
            "owner"
        )
        .is_err()
    );
}
#[test]
fn grant_cleanup_completed_future_retires_before_blocked_final_checkpoint() {
    ordering(Mode::Completed);
}
#[test]
fn grant_cleanup_failed_future_retires_before_blocked_final_checkpoint() {
    ordering(Mode::Failed);
}
#[test]
fn grant_cleanup_deadline_retires_before_blocked_final_checkpoint() {
    ordering(Mode::Deadline);
}
#[test]
fn grant_cleanup_permission_loss_retires_before_blocked_final_checkpoint() {
    ordering(Mode::Permission);
}

#[cfg(windows)]
#[test]
fn grant_cleanup_native_tree_exits_while_final_checkpoint_is_still_blocked() {
    use crate::process_lifetime::tests::{Processes, args};
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("node.json");
    let policy: crate::NodePolicy = serde_json::from_value(json!({"process":{"enabled":true,"max_seconds":30,"max_output_bytes":4096},"full_access":["process.exec"]})).unwrap();
    crate::setup::write_json(&config, &json!({"node_id":"owner","permissions":policy})).unwrap();
    let service = crate::NodeService::new(config, "owner".into());
    let engine = crate::NodeEngine::new(false, vec![], dir.path().into())
        .unwrap()
        .with_policy(policy.clone())
        .unwrap()
        .with_service(service.clone());
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let mut g = super::tests::grant();
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
    let journal = ledger.connection.lock().unwrap();
    let worker = std::thread::spawn(move || runtime().block_on(accepted.run(&engine)));
    let observer = runtime();
    let processes = observer.block_on(Processes::observe(dir.path()));
    processes.alive();
    service
        .save_configuration(json!({"node_id":"owner","permissions":{}}))
        .unwrap();
    let exited_before_save = observer.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !processes.exited_now() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok()
    });
    let not_returned_before_save = !worker.is_finished();
    let state: String = journal
        .query_row(
            "SELECT state FROM execution_receipts WHERE grant_id=?1",
            [&g.grant_id],
            |r| r.get(0),
        )
        .unwrap();
    drop(journal);
    let report = worker.join().unwrap();
    assert!(
        exited_before_save,
        "held native root/child handles must signal before final storage unblocks"
    );
    assert!(not_returned_before_save);
    assert_eq!(state, "accepted");
    assert!(report.confirms(&g));
    assert!(report.persisted);
    assert_eq!(report.state, ReceiptState::Unknown);
    assert_eq!(report.error_code.as_deref(), Some("local_policy_changed"));
    let fresh = crate::NodeEngine::new(false, vec![], dir.path().into())
        .unwrap()
        .with_policy(policy)
        .unwrap();
    fresh.check_grant_admission(&g).unwrap();
    assert!(
        AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &fresh,
            &g.issuer_id,
            &g.session_id,
            "owner"
        )
        .is_err()
    );
}
