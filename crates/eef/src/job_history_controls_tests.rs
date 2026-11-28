use super::*;
use crate::{
    event::EventBus,
    jobs::JobStore,
    task::{Goal, PlanStatus, TaskPlanner},
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

fn plan(store: &JobStore, origin: &str) -> String {
    plan_with_id(store, origin, None)
}

fn plan_with_id(store: &JobStore, origin: &str, id: Option<&str>) -> String {
    let context =
        eefn::context::RequestContext::new(uuid::Uuid::new_v4().to_string(), origin.into(), vec![])
            .unwrap();
    let mut plan = TaskPlanner
        .plan(
            serde_json::from_value::<Goal>(json!({
                "description":"Synthetic history control", "template":"generate_text",
                "params":{"prompt":"PRIVATE-input"}, "request_context":context
            }))
            .unwrap(),
        )
        .unwrap();
    if let Some(id) = id {
        plan.plan_id = id.to_owned();
    }
    let id = plan.plan_id.clone();
    store.insert(plan).unwrap();
    id
}

#[tokio::test(flavor = "current_thread")]
async fn retained_non_draft_job_ids_keep_public_pause_stop_and_removal() {
    let store = JobStore::in_memory().unwrap();
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    for supplied in ["Legacy-job_1".to_owned(), "A".repeat(128)] {
        let id = plan_with_id(&store, "owner", Some(&supplied));
        // The same ID must remain valid in the node's existing command contract.
        assert!(
            eefn::job_commands::JobCommand::Pause { id: id.clone() }
                .submission()
                .is_ok()
        );
        let paused = engine
            .control_history_job_request("jobs.pause", &id, Some("owner"))
            .await
            .unwrap();
        assert_eq!(paused["status"], json!(PlanStatus::Paused));
        assert_eq!(store.get(&id).unwrap().plan_id, supplied);
        let stopped = engine.stop_job_request(&id, Some("owner")).await.unwrap();
        assert_eq!(stopped["status"], json!(PlanStatus::Cancelled));
        let plain = eefn::job_commands::render_stop(
            &json!({"success":true,"acknowledged":true,"data":stopped}),
            &id,
        )
        .unwrap();
        assert!(plain.starts_with("Stop requested"));
        assert!(plain.contains("not proof"));
        assert_eq!(
            engine
                .control_history_job_request("jobs.remove", &id, Some("owner"))
                .await
                .unwrap(),
            json!({"removed":true})
        );
        assert!(store.get(&id).is_err());
    }
}

struct Held {
    release: Option<mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Held {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
async fn hold(store: Arc<JobStore>) -> Held {
    let (ready, observed) = oneshot::channel();
    let (release, waiting) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        store.with_test_journal_lock(|_| {
            let _ = ready.send(());
            let _ = waiting.recv_timeout(Duration::from_secs(10));
        })
    });
    observed.await.unwrap();
    Held {
        release: Some(release),
        thread: Some(thread),
    }
}
async fn available(store: &JobStore, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.history_control_available() != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn origin_and_existing_state_checks_apply_to_native_pause_and_removal() {
    let store = JobStore::in_memory().unwrap();
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    let id = plan(&store, "owner");
    let before = json!(store.get(&id).unwrap());
    let writes = store.with_test_journal_lock(|db| db.total_changes());
    for kind in ["jobs.pause", "jobs.remove"] {
        let error = engine
            .control_history_job_request(kind, &id, Some("other"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not found for this node"));
    }
    assert!(
        engine
            .control_history_job_request("jobs.remove", &id, Some("owner"))
            .await
            .is_err()
    );
    assert_eq!(json!(store.get(&id).unwrap()), before);
    assert_eq!(
        store.with_test_journal_lock(|db| db.total_changes()),
        writes
    );
    let paused = engine
        .control_history_job_request("jobs.pause", &id, Some("owner"))
        .await
        .unwrap();
    assert_eq!(paused["status"], json!(PlanStatus::Paused));
    assert_eq!(store.get(&id).unwrap().status, PlanStatus::Paused);
    engine.cancel(&id).unwrap();
    assert!(
        engine
            .control_history_job_request("jobs.remove", &id, Some("other"))
            .await
            .is_err()
    );
    assert_eq!(store.get(&id).unwrap().status, PlanStatus::Cancelled);
    assert_eq!(
        engine
            .control_history_job_request("jobs.remove", &id, Some("owner"))
            .await
            .unwrap(),
        json!({"removed":true})
    );
    assert!(store.get(&id).is_err());
    let retired = store.retirement();
    drop(engine);
    drop(store);
    retired.wait(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_and_expired_controls_hold_capacity_without_late_writes_or_stop_starvation() {
    let store = JobStore::in_memory().unwrap();
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    let paused_id = plan(&store, "owner");
    let removed_id = plan(&store, "owner");
    engine.cancel(&removed_id).unwrap();
    let before_pause = json!(store.get(&paused_id).unwrap());
    let before_remove = json!(store.get(&removed_id).unwrap());
    let writes = store.with_test_journal_lock(|db| db.total_changes());
    let held = hold(store.clone()).await;
    let worker = engine.clone();
    let id = paused_id.clone();
    let first = tokio::spawn(async move {
        worker
            .history_control_before(
                "jobs.pause",
                &id,
                Some("owner"),
                Instant::now() + Duration::from_secs(5),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while store.history_control_started() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert_eq!(store.history_control_available(), 1);
    let error = engine
        .history_control_before(
            "jobs.remove",
            &removed_id,
            Some("owner"),
            Instant::now() + Duration::from_millis(20),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("deadline elapsed"));
    assert_eq!(store.history_control_available(), 0);
    let error = engine
        .control_history_job_request("jobs.pause", &paused_id, None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("capacity unavailable"));
    assert_eq!(store.stop_intent_available(), 2);
    assert_eq!(store.journal_read_capacity_available(), 4);
    drop(held);
    available(&store, 2).await;
    assert_eq!(
        store.history_control_started(),
        1,
        "expired queued removal never entered storage"
    );
    assert_eq!(json!(store.get(&paused_id).unwrap()), before_pause);
    assert_eq!(json!(store.get(&removed_id).unwrap()), before_remove);
    assert_eq!(
        store.with_test_journal_lock(|db| db.total_changes()),
        writes
    );
}

#[tokio::test(flavor = "current_thread")]
async fn execution_operations_and_expired_admission_never_enter_history_control_worker() {
    let store = JobStore::in_memory().unwrap();
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    let id = plan(&store, "owner");
    let before = json!(store.get(&id).unwrap());
    let writes = store.with_test_journal_lock(|db| db.total_changes());
    for kind in [
        "jobs.create",
        "jobs.resume",
        "jobs.stop",
        "jobs.get",
        "jobs.unknown",
    ] {
        assert!(
            engine
                .control_history_job_request(kind, &id, None)
                .await
                .is_err()
        );
    }
    assert!(
        engine
            .history_control_before("jobs.pause", &id, None, Instant::now())
            .await
            .is_err()
    );
    assert_eq!(store.history_control_started(), 0);
    assert_eq!(store.history_control_available(), 2);
    assert_eq!(json!(store.get(&id).unwrap()), before);
    assert_eq!(
        store.with_test_journal_lock(|db| db.total_changes()),
        writes
    );
}
