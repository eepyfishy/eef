//! Final checkpoint contention must not retain the executing future or its tree.
use super::*;
use std::sync::mpsc;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Completed,
    Failed,
    Deadline,
    Permission,
    Expired,
    ClockJump,
}

#[derive(Clone)]
struct ControlledClock {
    state: Arc<Mutex<(Instant, u64)>>,
    wake: Arc<tokio::sync::Notify>,
}
impl ControlledClock {
    fn at(accepted: &AcceptedGrant) -> Self {
        Self {
            state: Arc::new(Mutex::new((accepted.accepted, accepted.accepted_wall))),
            wake: Arc::new(tokio::sync::Notify::new()),
        }
    }
    fn advance_to(&self, deadline: Instant) {
        let mut state = self.state.lock().unwrap();
        let elapsed = deadline.duration_since(state.0);
        state.1 += u64::try_from(elapsed.as_millis()).unwrap();
        state.0 = deadline;
        drop(state);
        // notify_one retains a permit if the select's sleep is not polled yet.
        self.wake.notify_one();
    }
    fn jump_wall(&self) {
        self.state.lock().unwrap().1 += 1001;
    }
}
impl ExecutionClock for ControlledClock {
    fn wall_ms(&self) -> u64 {
        self.state.lock().unwrap().1
    }
    fn elapsed(&self, since: Instant) -> Duration {
        self.state.lock().unwrap().0.duration_since(since)
    }
    fn reached(&self, deadline: Instant) -> bool {
        self.state.lock().unwrap().0 >= deadline
    }
    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()> + Send {
        async move {
            loop {
                let notified = self.wake.notified();
                if self.reached(deadline) {
                    return;
                }
                notified.await;
            }
        }
    }
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
    if matches!(mode, Mode::Deadline | Mode::Expired) {
        g.limits.max_execution_ms = 1000;
    }
    if matches!(mode, Mode::Permission) {
        g.capability = "filesystem".into();
        g.action = "write".into();
        g.params =
            json!({"path":dir.path().join("not-executed"),"content":"synthetic future only"});
    }
    let mut accepted = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        "owner",
    )
    .unwrap();
    let clock = ControlledClock::at(&accepted);
    let worker_clock = clock.clone();
    let deadline = accepted.deadline;
    if matches!(mode, Mode::Expired) {
        // Narrow only this synthetic test lease. The real wrapper must refuse
        // before polling; never rejuvenate a lease consumed by setup or I/O.
        accepted.deadline = accepted.deadline.min(Instant::now());
    }
    let (started_tx, started_rx) = mpsc::channel();
    let (dropped_tx, dropped_rx) = mpsc::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let mut release_tx = Some(release_tx);
    let drop_guard = Dropped(dropped_tx);
    // Hold the actual journal mutex, not a sleep pretending that storage is slow.
    // The executor thread can observe policy/deadline but cannot finish its save.
    let journal = ledger.connection.lock().unwrap();
    let worker = std::thread::spawn(move || {
        runtime().block_on(async move {
            let operation = async move {
                let _guard = drop_guard;
                let _ = started_tx.send(());
                let _ = release_rx.await;
                if matches!(mode, Mode::Failed) {
                    bail!("PRIVATE_EXECUTOR_FAILURE")
                }
                Ok(json!({"synthetic":true}))
            };
            if matches!(mode, Mode::Expired) {
                accepted.run_operation(&engine, operation).await
            } else {
                accepted
                    .run_operation_with_clock(&engine, operation, worker_clock)
                    .await
            }
        })
    });
    let started = started_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    if matches!(mode, Mode::Completed | Mode::Failed) {
        let _ = release_tx.take().unwrap().send(());
    } else if matches!(mode, Mode::Permission) {
        service
            .save_configuration(json!({"node_id":"owner","permissions":{}}))
            .unwrap();
    } else if matches!(mode, Mode::Deadline) {
        // Only after actual first poll. No real sleep or larger grant budget.
        // Also unblock cleanup on a failed start handshake, then fail its
        // original assertion after joining rather than hanging the fixture.
        clock.advance_to(deadline);
    } else if matches!(mode, Mode::ClockJump) {
        clock.jump_wall();
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
    // Observations above are already fixed. Unblock a buggy synthetic operation
    // as teardown only, so frozen test time cannot hang a failing fixture.
    if let Some(release) = release_tx.take() {
        let _ = release.send(());
    }
    drop(journal);
    let report = worker.join().unwrap();
    if matches!(mode, Mode::Expired) {
        assert!(!started, "expired real-clock operation must never poll");
    } else {
        assert!(started, "operation must actually poll: {mode:?}");
    }
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
    if matches!(mode, Mode::Deadline) {
        assert!(matches!(
            report.error_code.as_deref(),
            Some("grant_deadline_exceeded" | "grant_deadline_or_clock_changed")
        ));
    }
    if matches!(mode, Mode::Expired | Mode::ClockJump) {
        assert_eq!(
            report.error_code.as_deref(),
            Some("grant_deadline_or_clock_changed")
        );
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

#[test]
fn grant_cleanup_expired_real_clock_never_polls_before_blocked_final_checkpoint() {
    ordering(Mode::Expired);
}

#[test]
fn grant_cleanup_clock_jump_retires_before_blocked_final_checkpoint() {
    ordering(Mode::ClockJump);
}

#[tokio::test]
async fn completed_result_crossing_original_deadline_stays_unknown_and_never_replays() {
    let dir = tempfile::tempdir().unwrap();
    let engine = crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap();
    let ledger =
        GrantLedger::open(&dir.path().join("receipts.db"), Default::default(), "owner").unwrap();
    let g = super::tests::grant();
    assert_eq!(g.limits.max_execution_ms, 5000);
    let accepted = AcceptedGrant::prepare(
        g.clone(),
        ledger.clone(),
        &engine,
        &g.issuer_id,
        &g.session_id,
        &g.executor_node,
    )
    .unwrap();
    let clock = ControlledClock::at(&accepted);
    let completion_clock = clock.clone();
    let deadline = accepted.deadline;
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mark = ran.clone();
    let report = accepted
        .run_operation_with_clock(
            &engine,
            async move {
                mark.store(true, std::sync::atomic::Ordering::SeqCst);
                completion_clock.advance_to(deadline);
                Ok(json!({"private":"PRIVATE_LATE_OUTPUT"}))
            },
            clock,
        )
        .await;
    assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(report.state, ReceiptState::Unknown);
    assert_eq!(
        report.error_code.as_deref(),
        Some("completion_after_deadline")
    );
    assert!(!report.success && report.persisted && report.confirms(&g));
    assert!(report.data.is_null());
    let result = ledger
        .result(
            &g.issuer_id,
            &g.grant_id,
            &definition_digest(&json!(g)).unwrap(),
        )
        .unwrap();
    assert_eq!(result["report"], json!(report));
    assert!(!result.to_string().contains("PRIVATE_LATE_OUTPUT"));
    assert!(
        AcceptedGrant::prepare(
            g.clone(),
            ledger,
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node
        )
        .is_err()
    );
}

#[tokio::test]
async fn controlled_clock_notification_before_sleep_cannot_lose_deadline() {
    let now = Instant::now();
    let clock = ControlledClock {
        state: Arc::new(Mutex::new((now, 10))),
        wake: Arc::new(tokio::sync::Notify::new()),
    };
    let deadline = now + Duration::from_millis(1000);
    assert!(!clock.reached(deadline));
    clock.advance_to(deadline);
    assert_eq!(clock.wall_ms(), 1010);
    assert_eq!(clock.elapsed(now), Duration::from_millis(1000));
    tokio::time::timeout(Duration::from_secs(3), clock.sleep_until(deadline))
        .await
        .unwrap();
}

#[test]
fn normal_clock_wrapper_does_not_duplicate_large_operation_future() {
    async fn large_operation() -> Result<Value> {
        let bytes = [7_u8; 64 * 1024];
        std::future::pending::<()>().await;
        Ok(json!({"bytes": std::hint::black_box(&bytes).len()}))
    }
    // Infer real executor future layouts without constructing a grant/engine,
    // polling an operation or allocating the large synthetic buffer at runtime.
    fn size<F, R>(_: F) -> usize
    where
        F: FnOnce(AcceptedGrant, &'static crate::NodeEngine) -> R,
    {
        std::mem::size_of::<R>()
    }
    let direct = size(|accepted, engine| {
        accepted.run_operation_with_clock(engine, large_operation(), SystemClock)
    });
    let normal = size(|accepted, engine| accepted.run_operation(engine, large_operation()));
    assert_eq!(
        normal, direct,
        "normal wrapper must return the clock future directly, not store another async frame"
    );
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
