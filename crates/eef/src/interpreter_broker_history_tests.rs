//! Real journal contention at command admission; no model or authority in readers.
use super::*;

struct HeldJournal {
    release: std::sync::mpsc::SyncSender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for HeldJournal {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn hold(store: Arc<JobStore>) -> (HeldJournal, tokio::sync::oneshot::Receiver<()>) {
    let (ready, observed) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        store.with_test_journal_lock(|_| {
            let _ = ready.send(());
            let _ = waiting.recv_timeout(Duration::from_secs(15));
        });
    });
    (
        HeldJournal {
            release,
            thread: Some(thread),
        },
        observed,
    )
}
async fn reading(store: &JobStore, before: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.journal_reads_started() == before {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Command history did not enter the native journal reader");
}
fn wire(request: &Request) -> eefn::interpreter_command::WireRequest {
    eefn::interpreter_command::WireRequest {
        schema_version: 1,
        origin_runtime_id: request.expected_runtime_id.clone(),
        command: request.command.clone(),
    }
}
fn context(request: &Request) -> RequestContext {
    RequestContext::new(request.command.operation_id.clone(), "input".into(), vec![]).unwrap()
}

#[tokio::test]
async fn command_history_deadline_keeps_native_capacity_until_the_actual_reader_finishes() {
    let f = Fixture::new().await;
    let (_, request) = request(&f, "initial").await;
    let runtime = f.runtime().clone();
    let store = runtime.engine.store.clone();
    let source = json!(store.get(&request.command.id).unwrap());
    let ledger = json!(store.interpreter_allowance_records().unwrap());
    let available = store.journal_read_capacity_available();
    let before = store.journal_reads_started();
    let origin = f.participants().await.origin;
    let context = context(&request);
    let (held, observed) = hold(store.clone());
    observed.await.unwrap();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(7), async {
        tokio::join!(
            runtime.origin_interpreter_command(wire(&request), &context, origin.session_id()),
            reading(&store, before),
        )
    })
    .await
    .expect("History deadline blocked the async runtime");
    assert!(result.unwrap_err().to_string().contains("deadline"));
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.journal_read_capacity_available(), available - 1);
    drop(held);
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.journal_read_capacity_available() != available {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Retired native history read kept capacity after storage returned");
    assert_eq!(json!(store.get(&request.command.id).unwrap()), source);
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
    drop(store);
    drop(runtime);
    f.close().await;
}

#[tokio::test]
async fn command_history_wait_yields_before_any_model_call_or_source_claim() {
    let f = Fixture::new().await;
    let (_, request) = request(&f, "initial").await;
    let runtime = f.runtime().clone();
    let store = runtime.engine.store.clone();
    let origin = f.participants().await.origin;
    let context = context(&request);
    let before = store.journal_reads_started();
    let (held, observed) = hold(store.clone());
    observed.await.unwrap();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(
            runtime.origin_interpreter_command(wire(&request), &context, origin.session_id()),
            async {
                reading(&store, before).await;
                tokio::time::sleep(Duration::from_millis(10)).await;
                assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
                assert!(
                    runtime
                        .registry
                        .find("llm.infer", "run")
                        .iter()
                        .all(|p| p.in_flight == 0)
                );
                drop(held);
            }
        )
    })
    .await
    .expect("Journal contention blocked command timers");
    let report = result.unwrap();
    assert!(eefn::interpreter_command::confirmed(
        &report,
        "input",
        &request.command
    ));
    assert_eq!(report["success"], true);
    assert_eq!(report["historical_snapshot"], false);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    let saved = store.get(&request.command.id).unwrap();
    assert!(saved.approval.is_none() && saved.execution.is_none());
    drop(store);
    drop(runtime);
    f.close().await;
}

#[tokio::test]
async fn origin_loss_during_history_read_refuses_retained_reply_without_inference_or_edits() {
    let mut f = Fixture::new().await;
    let (_, request) = request(&f, "initial").await;
    assert_eq!(
        f.services[0]
            .interpreter_command(request.clone())
            .await
            .unwrap()["success"],
        true
    );
    let runtime = f.runtime().clone();
    let store = runtime.engine.store.clone();
    store.wait_workload_finalizations_async().await.unwrap();
    let source = json!(store.get(&request.command.id).unwrap());
    let ledger = json!(store.interpreter_allowance_records().unwrap());
    let context = context(&request);
    let origin = f.participants().await.origin;
    let before = store.journal_reads_started();
    let input = f.nodes.remove(0);
    let (held, observed) = hold(store.clone());
    observed.await.unwrap();
    let (result, ()) = tokio::join!(
        runtime.origin_interpreter_command(wire(&request), &context, origin.session_id()),
        async {
            reading(&store, before).await;
            input.abort();
            let _ = input.await;
            tokio::time::timeout(Duration::from_secs(3), async {
                while runtime
                    .node_server
                    .interpreter_session("input", 30)
                    .await
                    .is_ok()
                {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .expect("Origin did not disconnect while journal read was blocked");
            drop(held);
        }
    );
    assert!(
        result.is_err(),
        "Retired origin received a historical command reply"
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(json!(store.get(&request.command.id).unwrap()), source);
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    drop(store);
    drop(runtime);
    f.close().await;
}

#[tokio::test]
async fn history_lookup_keeps_cross_job_operation_conflicts_before_fresh_inference() {
    let f = Fixture::new().await;
    let (_, first) = request(&f, "initial").await;
    assert_eq!(
        f.services[0]
            .interpreter_command(first.clone())
            .await
            .unwrap()["success"],
        true
    );
    let (_, mut other) = request(&f, "initial").await;
    other.command.operation_id = first.command.operation_id.clone();
    let store = f.runtime().engine.store.clone();
    store.wait_workload_finalizations_async().await.unwrap();
    let source = json!(store.get(&other.command.id).unwrap());
    let ledger = json!(store.interpreter_allowance_records().unwrap());
    let origin = f.participants().await.origin;
    let context = context(&other);
    let error = f
        .runtime()
        .origin_interpreter_command(wire(&other), &context, origin.session_id())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("receipt conflicts with retained source")
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(json!(store.get(&other.command.id).unwrap()), source);
    assert_eq!(
        json!(store.interpreter_allowance_records().unwrap()),
        ledger
    );
    drop(store);
    f.close().await;
}

#[tokio::test]
async fn history_lookup_rejects_unrelated_corrupted_account_without_repair_or_new_work() {
    let f = Fixture::new().await;
    let (_, first) = request(&f, "initial").await;
    assert_eq!(
        f.services[0].interpreter_command(first).await.unwrap()["success"],
        true
    );
    let (_, other) = request(&f, "initial").await;
    let store = f.runtime().engine.store.clone();
    store.wait_workload_finalizations_async().await.unwrap();
    let source = json!(store.get(&other.command.id).unwrap());
    let original: String = store.with_test_journal_lock(|db| {
        let original = db
            .query_row(
                "SELECT record_json FROM interpreter_allowance_accounts",
                [],
                |row| row.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE interpreter_allowance_accounts SET record_json='{}'",
            [],
        )
        .unwrap();
        original
    });
    let origin = f.participants().await.origin;
    let context = context(&other);
    assert!(
        f.runtime()
            .origin_interpreter_command(wire(&other), &context, origin.session_id())
            .await
            .is_err()
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(json!(store.get(&other.command.id).unwrap()), source);
    store.with_test_journal_lock(|db| {
        let retained: String = db
            .query_row(
                "SELECT record_json FROM interpreter_allowance_accounts",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, "{}");
        let attempts: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM interpreter_allowance_attempts",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 1);
        // Explicit fixture cleanup only, after proving refusal preserved damage.
        db.execute(
            "UPDATE interpreter_allowance_accounts SET record_json=?1",
            [&original],
        )
        .unwrap();
    });
    drop(store);
    f.close().await;
}
