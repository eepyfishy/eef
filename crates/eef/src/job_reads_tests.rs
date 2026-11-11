use super::*;
use crate::task::{Goal, TaskPlanner};
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

struct HeldLock {
    release: Option<mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for HeldLock {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("Synthetic lock holder failed");
        }
    }
}
fn hold(store: Arc<JobStore>) -> (HeldLock, oneshot::Receiver<()>) {
    let (ready, observed) = oneshot::channel();
    let (release, waiting) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let _guard = store.connection.lock().unwrap();
        let _ = ready.send(());
        // Finite test-only fallback; never a production I/O allowance.
        let _ = waiting.recv_timeout(Duration::from_secs(15));
    });
    (
        HeldLock {
            release: Some(release),
            thread: Some(thread),
        },
        observed,
    )
}
async fn started(store: &JobStore) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.reads.started.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("Owned read worker did not enter the journal query");
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn reads_keep_exact_history_and_corruption_without_retaining_an_idle_store() {
    let store = JobStore::in_memory().unwrap();
    let plan = TaskPlanner
        .plan(
            serde_json::from_value::<Goal>(json!({
                "description":"Synthetic read history","template":"generate_text",
                "params":{"prompt":"PRIVATE-retained-input"}
            }))
            .unwrap(),
        )
        .unwrap();
    let id = plan.plan_id.clone();
    store.insert(plan).unwrap();
    let before = json!(store.get(&id).unwrap());
    assert_eq!(
        json!(store.get_before(&id, deadline()).await.unwrap()),
        before
    );
    assert_eq!(json!(store.get(&id).unwrap()), before);
    assert!(store.get_before("missing", deadline()).await.is_err());
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE durable_jobs SET plan_json='PRIVATE-damaged' WHERE job_id=?1",
            [&id],
        )
        .unwrap();
    assert!(store.get_before(&id, deadline()).await.is_err());
    let raw: String = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, "PRIVATE-damaged");
    let retired = store.retirement();
    drop(store);
    retired.wait(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn blocked_read_keeps_capacity_after_cancel_and_queued_timeout_without_blocking_timers() {
    let store = JobStore::in_memory().unwrap();
    let (locked, ready) = hold(store.clone());
    ready.await.unwrap();
    let first_store = store.clone();
    let first = tokio::spawn(async move {
        first_store
            .get_before("missing", Instant::now() + Duration::from_secs(30))
            .await
    });
    started(&store).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert_eq!(store.reads.slots.available_permits(), CAPACITY - 1);

    let mut queued = Vec::new();
    for _ in 1..CAPACITY {
        let store = store.clone();
        queued.push(tokio::spawn(async move {
            store
                .get_before("missing", Instant::now() + Duration::from_millis(20))
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        for read in queued {
            let error = read.await.unwrap().unwrap_err();
            assert!(error.to_string().contains("deadline elapsed"));
        }
    })
    .await
    .expect("SQLite waits blocked the current-thread runtime's timers");
    assert_eq!(store.reads.slots.available_permits(), 0);
    assert!(
        store
            .get_before("missing", deadline())
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity unavailable")
    );
    drop(locked);
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.reads.slots.available_permits() != CAPACITY {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.reads.started.load(Ordering::SeqCst),
        1,
        "Expired/cancelled queued reads must never query later"
    );
}

#[tokio::test]
async fn expired_read_never_starts_a_worker_or_queries_the_database() {
    let store = JobStore::in_memory().unwrap();
    let error = store
        .get_before("missing", Instant::now())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("before admission"));
    assert!(store.reads.sender.lock().unwrap().is_none());
    assert_eq!(store.reads.started.load(Ordering::SeqCst), 0);
    assert_eq!(store.reads.slots.available_permits(), CAPACITY);
}

#[tokio::test]
async fn interpreter_inspection_shares_read_capacity_and_keeps_timers_live_during_journal_contention()
 {
    let store = JobStore::in_memory().unwrap();
    let (locked, ready) = hold(store.clone());
    ready.await.unwrap();
    let reading = store.clone();
    let first = tokio::spawn(async move {
        reading
            .interpreter_source_snapshot_before("missing", "input", deadline())
            .await
    });
    started(&store).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert_eq!(store.reads.slots.available_permits(), CAPACITY - 1);
    let mut queued = Vec::new();
    for _ in 1..CAPACITY {
        let reading = store.clone();
        queued.push(tokio::spawn(async move {
            reading
                .interpreter_source_snapshot_before(
                    "missing",
                    "input",
                    Instant::now() + Duration::from_millis(20),
                )
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        for read in queued {
            assert!(
                read.await
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains("deadline elapsed")
            );
        }
    })
    .await
    .expect("Interpreter inspection blocked async timers behind the journal lock");
    assert_eq!(store.reads.slots.available_permits(), 0);
    assert!(
        store
            .get_before("missing", deadline())
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity unavailable")
    );
    assert!(
        store
            .interpreter_source_snapshot_before("missing", "input", deadline())
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity unavailable")
    );
    drop(locked);
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.reads.slots.available_permits() != CAPACITY {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.reads.started.load(Ordering::SeqCst), 1);
    assert!(store.list().unwrap().is_empty());
    assert!(store.interpreter_allowance_records().unwrap().0.is_empty());
    assert!(
        store
            .interpreter_source_snapshot_before("missing", "input", deadline())
            .await
            .unwrap_err()
            .to_string()
            .contains("Job not found")
    );
    assert_eq!(store.reads.slots.available_permits(), CAPACITY);
}

#[tokio::test]
async fn expired_interpreter_inspection_starts_no_reader_and_preserves_retained_history() {
    let store = JobStore::in_memory().unwrap();
    let error = store
        .interpreter_source_snapshot_before("missing", "input", Instant::now())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("before admission"));
    assert!(store.reads.sender.lock().unwrap().is_none());
    assert_eq!(store.reads.started.load(Ordering::SeqCst), 0);
    assert_eq!(store.reads.slots.available_permits(), CAPACITY);
    let mut plan = TaskPlanner
        .plan(serde_json::from_value::<Goal>(json!({
            "description":"Historical source","template":"generate_text","params":{"prompt":"PRIVATE-source"}
        })).unwrap())
        .unwrap();
    plan.goal.request_context = Some(
        eefn::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "input".into(),
            vec![],
        )
        .unwrap(),
    );
    for task in &mut plan.tasks {
        task.request_context = plan.goal.request_context.clone();
    }
    let id = plan.plan_id.clone();
    store.insert(plan).unwrap();
    let expected = json!(store.get(&id).unwrap());
    let (view, attempts) = store
        .interpreter_source_snapshot_before(&id, "input", deadline())
        .await
        .unwrap();
    assert_eq!(json!(view), expected);
    assert!(attempts.is_empty());
    assert!(
        store
            .interpreter_source_snapshot_before(&id, "foreign", deadline())
            .await
            .is_err()
    );
    assert_eq!(json!(store.get(&id).unwrap()), expected);
    let connection = store.connection.lock().unwrap();
    connection
        .execute(
            "UPDATE durable_jobs SET plan_json='PRIVATE-damaged' WHERE job_id=?1",
            [&id],
        )
        .unwrap();
    drop(connection);
    assert!(
        store
            .interpreter_source_snapshot_before(&id, "input", deadline())
            .await
            .is_err()
    );
    let raw: String = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, "PRIVATE-damaged");
    let retired = store.retirement();
    drop(store);
    retired.wait(Duration::from_secs(1)).await.unwrap();
}

#[test]
fn runtime_shutdown_does_not_join_blocked_reads_or_release_the_owned_database_early() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    drop(rusqlite::Connection::open(&path).unwrap());
    let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
    let retired = store.retirement();
    let (locked, ready) = hold(store.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        ready.await.unwrap();
        let reading = store.clone();
        tokio::spawn(async move { reading.get_before("missing", deadline()).await });
        started(&store).await;
    });
    let stopping = std::time::Instant::now();
    drop(runtime);
    assert!(
        stopping.elapsed() < Duration::from_secs(1),
        "Runtime shutdown joined a blocked SQLite read"
    );
    assert_eq!(store.reads.slots.available_permits(), CAPACITY - 1);
    drop(store);
    assert!(JobStore::open(&path, 500, 64 * 1024 * 1024).is_err());
    drop(locked);
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    observer
        .block_on(retired.wait(Duration::from_secs(2)))
        .unwrap();
    let replacement = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
    assert!(replacement.list().unwrap().is_empty());
}
