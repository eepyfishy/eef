//! Actual coordinator journal contention and authenticated mock-model exchanges.
use super::*;

struct HeldJournal {
    release: Option<std::sync::mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for HeldJournal {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn hold(store: Arc<JobStore>) -> (HeldJournal, tokio::sync::oneshot::Receiver<()>) {
    let (ready, seen) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        store.with_test_journal_lock(|_| {
            let _ = ready.send(());
            let _ = waiting.recv_timeout(Duration::from_secs(15));
        });
    });
    (
        HeldJournal {
            release: Some(release),
            thread: Some(thread),
        },
        seen,
    )
}

#[tokio::test]
async fn dropped_completion_retires_in_memory_before_blocked_journal_cleanup_and_keeps_reporting_capacity()
 {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let store = f.runtime().engine.store.clone();
    let available = store.workload_finalization_available();
    let participants = f.participants().await;
    let Exchange::Completed(completed) = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
        .await
        .unwrap()
    else {
        panic!("Missing completed interpretation");
    };
    let report = json!(completed.record());
    let charged = json!(store.interpreter_allowance_records().unwrap().1);
    let (held, seen) = hold(store.clone());
    seen.await.unwrap();
    let before_drop = std::time::Instant::now();
    drop(completed);
    assert!(
        before_drop.elapsed() < Duration::from_secs(1),
        "Continuation drop waited for SQLite"
    );
    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::time::sleep(Duration::from_millis(10)),
    )
    .await
    .unwrap();
    assert_eq!(store.workload_finalization_available(), available - 1);
    assert!(
        f.runtime()
            .registry
            .find("llm.infer", "run")
            .iter()
            .all(|p| p.in_flight == 0)
    );
    // The old registration is retired even though its metadata save is blocked.
    // A registration alone cannot bypass the original retained-attempt checks.
    drop(
        f.runtime()
            .engine
            .claim_workload(&task.snapshot.job_id)
            .unwrap(),
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    drop(held);
    store.wait_workload_finalizations().unwrap();
    assert_eq!(store.workload_finalization_available(), available);
    assert!(
        f.runtime()
            .registry
            .find("llm.infer", "run")
            .iter()
            .all(|p| p.in_flight == 0)
    );
    let saved = store.get(&task.snapshot.job_id).unwrap();
    assert_eq!(
        saved.input_interpretation.unwrap().state,
        eefn::request_interpretation::State::Unknown
    );
    let (records, accounts) = store.interpreter_allowance_records().unwrap();
    assert_eq!(json!(records[0]), report);
    assert_eq!(json!(accounts), charged);
    assert!(matches!(
        f.runtime()
            .exchange_interpreter(task, f.pin.clone(), &participants)
            .await
            .unwrap(),
        Exchange::Historical(_)
    ));
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    drop(store);
    f.close().await;
}

#[tokio::test]
async fn full_finalization_capacity_refuses_interpretation_before_claim_charge_or_dispatch() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let store = f.runtime().engine.store.clone();
    let available = store.workload_finalization_available();
    let source = json!(store.get(&task.snapshot.job_id).unwrap());
    let ledger = json!(store.interpreter_allowance_records().unwrap());
    let usage = store.usage().unwrap();
    let mut slots = Vec::new();
    while let Ok(slot) = store.reserve_workload_finalization() {
        slots.push(slot);
    }
    assert_eq!(slots.len(), available);
    assert!(
        f.runtime()
            .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
            .await
            .is_err()
    );
    assert_eq!(json!(store.get(&task.snapshot.job_id).unwrap()), source);
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    assert_eq!(store.usage().unwrap(), usage);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
    assert!(store.require_workload_reporting().is_ok());
    drop(slots);
    assert_eq!(store.workload_finalization_available(), available);
    drop(
        f.runtime()
            .engine
            .claim_workload(&task.snapshot.job_id)
            .unwrap(),
    );
    drop(store);
    f.close().await;
}

#[tokio::test]
async fn failed_retirement_denies_new_authority_without_rewriting_evidence_or_retrying() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let Exchange::Completed(completed) = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
        .await
        .unwrap()
    else {
        panic!("Missing completed interpretation");
    };
    let store = f.runtime().engine.store.clone();
    let source = json!(store.get(&task.snapshot.job_id).unwrap());
    let ledger = json!(store.interpreter_allowance_records().unwrap());
    store.with_test_journal_lock(|db|db.execute_batch("CREATE TRIGGER fail_interpreter_retire BEFORE UPDATE ON durable_jobs BEGIN SELECT RAISE(FAIL,'synthetic retirement fault'); END;")).unwrap();
    drop(completed);
    store.wait_workload_finalizations().unwrap();
    assert!(store.require_workload_reporting().is_err());
    assert_eq!(json!(store.get(&task.snapshot.job_id).unwrap()), source);
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    store
        .with_test_journal_lock(|db| db.execute_batch("DROP TRIGGER fail_interpreter_retire"))
        .unwrap();
    let next = f.task("initial").await;
    let next_source = json!(store.get(&next.snapshot.job_id).unwrap());
    assert!(
        f.runtime()
            .exchange_interpreter(next.clone(), f.pin.clone(), &participants)
            .await
            .is_err()
    );
    assert!(store.require_workload_reporting().is_err());
    assert_eq!(
        json!(store.get(&next.snapshot.job_id).unwrap()),
        next_source
    );
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        f.runtime()
            .exchange_interpreter(task, f.pin.clone(), &participants)
            .await
            .unwrap(),
        Exchange::Historical(_)
    ));
    drop(store);
    f.close().await;
}

#[tokio::test]
async fn failed_stop_checkpoint_still_revokes_the_original_interpreter_application() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let Exchange::Completed(completed) = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
        .await
        .unwrap()
    else {
        panic!("Missing completed interpretation");
    };
    let store = f.runtime().engine.store.clone();
    store
        .with_test_journal_lock(|db| db.execute_batch("PRAGMA query_only=ON"))
        .unwrap();
    assert!(f.runtime().engine.cancel(&task.snapshot.job_id).is_err());
    store
        .with_test_journal_lock(|db| db.execute_batch("PRAGMA query_only=OFF"))
        .unwrap();
    assert!(
        f.runtime()
            .apply_interpreter_completion(completed)
            .await
            .is_err()
    );
    store.wait_workload_finalizations().unwrap();
    let saved = store.get(&task.snapshot.job_id).unwrap();
    assert!(
        saved.request_review.is_none() && saved.planning.is_none() && saved.execution.is_none()
    );
    assert_eq!(
        saved.input_interpretation.unwrap().state,
        eefn::request_interpretation::State::Unknown
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    drop(store);
    f.close().await;
}
