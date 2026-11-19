// Included in execution_stop::tests: exact issued grants and real temporary
// journals, without a source, model, device or new execution authority.
use crate::jobs::{GrantStopCheckpoint, JobStore};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

struct HeldGrantJournal {
    release: Option<std::sync::mpsc::SyncSender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for HeldGrantJournal {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("Synthetic journal lock holder failed");
        }
    }
}
fn hold_grant_journal(
    store: Arc<JobStore>,
) -> (HeldGrantJournal, tokio::sync::oneshot::Receiver<()>) {
    let (ready, seen) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        store.with_test_journal_lock(|_| {
            let _ = ready.send(());
            // Bounded test-only guard, never a production storage timeout.
            let _ = waiting.recv_timeout(Duration::from_secs(15));
        })
    });
    (
        HeldGrantJournal {
            release: Some(release),
            thread: Some(thread),
        },
        seen,
    )
}
fn stop_end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
async fn wait_stop_writer(store: &JobStore, before: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.grant_stop_entered() == before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Native stop writer must enter its journal wait");
}
async fn wait_stop_capacity(store: &JobStore, capacity: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while store.workload_ack_available() != capacity {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Actual stop storage must release its capacity");
}
async fn checkpoint(
    store: &Arc<JobStore>,
    g: &ExecutionGrant,
    phase: GrantStopCheckpoint,
) -> Result<bool> {
    store
        .checkpoint_grant_stop_before(&g.job_id, g.clone(), phase, stop_end())
        .await
}

#[tokio::test]
async fn grant_stop_checkpoint_preserves_scope_one_send_and_immutable_receipt() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (unstopped, g, _) = crate::request_execution::tests::issued(&engine);
    let before = json!(engine.store.get(&unstopped.plan_id).unwrap());
    assert!(
        !checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
            .await
            .unwrap()
    );
    assert_eq!(json!(engine.store.get(&unstopped.plan_id).unwrap()), before);
    let (plan, g) = stopping(&engine);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    let mut changed = g.clone();
    changed.session_id = uuid::Uuid::new_v4().to_string();
    assert!(
        checkpoint(&engine.store, &changed, GrantStopCheckpoint::Begin)
            .await
            .is_err()
    );
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
    assert!(
        checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
            .await
            .unwrap()
    );
    let reserved = json!(engine.store.get(&plan.plan_id).unwrap());
    assert!(
        !checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
            .await
            .unwrap()
    );
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), reserved);
    let receipt = report(&g);
    let mut forged = receipt.clone();
    forged.execution_stopped = true;
    let entered = engine.store.grant_stop_entered();
    assert!(
        checkpoint(&engine.store, &g, GrantStopCheckpoint::Receipt(forged))
            .await
            .is_err()
    );
    assert_eq!(
        engine.store.grant_stop_entered(),
        entered,
        "Forged receipt cannot start native storage"
    );
    assert!(
        checkpoint(
            &engine.store,
            &g,
            GrantStopCheckpoint::Receipt(receipt.clone())
        )
        .await
        .unwrap()
    );
    let saved = json!(engine.store.get(&plan.plan_id).unwrap());
    assert!(
        !checkpoint(
            &engine.store,
            &g,
            GrantStopCheckpoint::Receipt(receipt.clone())
        )
        .await
        .unwrap()
    );
    let mut replacement = receipt;
    replacement.reused = true;
    assert!(
        checkpoint(&engine.store, &g, GrantStopCheckpoint::Receipt(replacement))
            .await
            .is_err()
    );
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), saved);
    assert_eq!(
        saved["execution"]["schema_version"],
        before["execution"]["schema_version"]
    );
}

#[tokio::test]
async fn abandoned_grant_stop_writes_hold_capacity_and_cannot_commit_after_journal_release() {
    for receipt in [false, true] {
        let engine = TaskEngine::new(EventBus::new(20));
        let (plan, g) = stopping(&engine);
        if receipt {
            assert!(
                checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
                    .await
                    .unwrap()
            );
        }
        let before = json!(engine.store.get(&plan.plan_id).unwrap());
        let capacity = engine.store.workload_ack_available();
        let entered = engine.store.grant_stop_entered();
        let (held, ready) = hold_grant_journal(engine.store.clone());
        ready.await.unwrap();
        let phase = if receipt {
            GrantStopCheckpoint::Receipt(report(&g))
        } else {
            GrantStopCheckpoint::Begin
        };
        let mut saving = Box::pin(checkpoint(&engine.store, &g, phase));
        tokio::select! {biased;
            result=&mut saving=>panic!("Held journal returned: {result:?}"),
            _=wait_stop_writer(&engine.store,entered)=>{},
        }
        assert_eq!(engine.store.workload_ack_available(), capacity - 1);
        tokio::time::timeout(Duration::from_secs(1), tokio::task::yield_now())
            .await
            .unwrap();
        drop(saving);
        assert_eq!(
            engine.store.workload_ack_available(),
            capacity - 1,
            "Cancelling observation cannot free actual storage"
        );
        drop(held);
        wait_stop_capacity(&engine.store, capacity).await;
        assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
    }
}

#[tokio::test]
async fn grant_stop_deadline_and_capacity_refusal_do_not_queue_replacement_writes() {
    let mut store = JobStore::in_memory().unwrap();
    Arc::get_mut(&mut store).unwrap().test_workload_ack_limit(1);
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let (plan, g) = stopping(&engine);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    let entered = engine.store.grant_stop_entered();
    assert!(
        engine
            .store
            .checkpoint_grant_stop_before(
                &plan.plan_id,
                g.clone(),
                GrantStopCheckpoint::Begin,
                Instant::now()
            )
            .await
            .is_err()
    );
    assert_eq!(engine.store.grant_stop_entered(), entered);
    let (held, ready) = hold_grant_journal(engine.store.clone());
    ready.await.unwrap();
    let mut saving = Box::pin(engine.store.checkpoint_grant_stop_before(
        &plan.plan_id,
        g.clone(),
        GrantStopCheckpoint::Begin,
        Instant::now() + Duration::from_secs(2),
    ));
    tokio::select! {biased;
        result=&mut saving=>panic!("Held journal returned: {result:?}"),
        _=wait_stop_writer(&engine.store,entered)=>{},
    }
    let error = checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("capacity unavailable"));
    assert_eq!(engine.store.grant_stop_entered(), entered + 1);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut saving)
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("deadline elapsed")
    );
    drop(saving);
    assert_eq!(engine.store.workload_ack_available(), 0);
    drop(held);
    wait_stop_capacity(&engine.store, 1).await;
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
}

#[tokio::test]
async fn stop_observation_uses_bounded_reader_before_polling_the_operation() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, g) = stopping(&engine);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    let server = eefn::NodeServer::new("private-stop-read-fixture", "127.0.0.1", 0).unwrap();
    let driver = Driver::peer_fixture(engine.clone(), server, Default::default());
    let capacity = engine.store.journal_read_capacity_available();
    let entered = engine.store.journal_reads_started();
    let (held, ready) = hold_grant_journal(engine.store.clone());
    ready.await.unwrap();
    let called = std::sync::atomic::AtomicBool::new(false);
    let grants = [g];
    let mut waiting = Box::pin(driver.wait_with_stop(&plan.plan_id, &grants, async {
        called.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }));
    tokio::select! {biased;
        result=&mut waiting=>panic!("Held read returned: {result:?}"),
        result=tokio::time::timeout(Duration::from_secs(2),async {
            while engine.store.journal_reads_started()==entered {tokio::task::yield_now().await;}
        })=>result.unwrap(),
    }
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(engine.store.journal_read_capacity_available(), capacity - 1);
    drop(waiting);
    assert_eq!(engine.store.journal_read_capacity_available(), capacity - 1);
    drop(held);
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.store.journal_read_capacity_available() != capacity {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
}

#[test]
fn runtime_shutdown_does_not_join_an_abandoned_native_stop_checkpoint() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, g) = stopping(&engine);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    let capacity = engine.store.workload_ack_available();
    let entered = engine.store.grant_stop_entered();
    let (held, ready) = hold_grant_journal(engine.store.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        ready.await.unwrap();
        let store = engine.store.clone();
        tokio::spawn(async move { checkpoint(&store, &g, GrantStopCheckpoint::Begin).await });
        wait_stop_writer(&engine.store, entered).await;
    });
    let start = std::time::Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(engine.store.workload_ack_available(), capacity - 1);
    drop(held);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(wait_stop_capacity(&engine.store, capacity));
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
}

#[tokio::test]
async fn workload_reporting_failure_does_not_disable_one_shot_stop_control() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (_plan, g) = stopping(&engine);
    engine.store.deny_workload_reporting();
    assert!(engine.store.require_workload_reporting().is_err());
    assert!(
        checkpoint(&engine.store, &g, GrantStopCheckpoint::Begin)
            .await
            .unwrap()
    );
    assert!(
        checkpoint(&engine.store, &g, GrantStopCheckpoint::Receipt(report(&g)))
            .await
            .unwrap()
    );
    assert!(
        engine.store.require_workload_reporting().is_err(),
        "Stopping does not clear a reporting fault"
    );
}

#[tokio::test]
async fn unrelated_stop_notifications_cannot_retry_failed_stop_admission() {
    use std::{future::Future, pin::Pin};
    let mut store = JobStore::in_memory().unwrap();
    Arc::get_mut(&mut store).unwrap().test_workload_ack_limit(0);
    let engine = TaskEngine::with_store(EventBus::new(20), store);
    let (plan, g, _) = crate::request_execution::tests::issued(&engine);
    let server = eefn::NodeServer::new("private-stop-latch-fixture", "127.0.0.1", 0).unwrap();
    let driver = Driver::peer_fixture(engine.clone(), server, Default::default());
    let (finish, mut finishing) = tokio::sync::oneshot::channel::<()>();
    let (polls, mut observed) = tokio::sync::watch::channel(0usize);
    let id = plan.plan_id.clone();
    let executing = tokio::spawn(async move {
        driver
            .wait_with_stop(
                &id,
                &[g],
                std::future::poll_fn(move |cx| {
                    polls.send_modify(|count| *count += 1);
                    Pin::new(&mut finishing)
                        .poll(cx)
                        .map(|value| value.map_err(anyhow::Error::from))
                }),
            )
            .await
    });
    async fn polled(observed: &mut tokio::sync::watch::Receiver<usize>, minimum: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while *observed.borrow_and_update() < minimum {
                observed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    polled(&mut observed, 1).await;
    engine
        .store
        .change(&plan.plan_id, "Exact synthetic stop intent", |p| {
            p.status = PlanStatus::Stopping;
            let e = p.execution.as_mut().unwrap();
            e.stop_requested_at_ms = Some(eefn::protocol::now_ms().max(e.started_at_ms));
            Ok(())
        })
        .unwrap();
    engine.stop_changes.send_modify(|v| *v = v.wrapping_add(1));
    // A second operation poll proves the changed-notification branch has
    // completed its failed zero-capacity stop attempt and returned to select.
    polled(&mut observed, 2).await;
    let reads = engine.store.journal_reads_started();
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    engine.stop_changes.send_modify(|v| *v = v.wrapping_add(1));
    finish.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), executing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        engine.store.journal_reads_started(),
        reads,
        "Unrelated notification cannot restart stop inspection/admission"
    );
    assert_eq!(engine.store.grant_stop_entered(), 0);
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
}

#[tokio::test]
async fn public_stop_checks_the_authenticated_origin_and_preserves_runner_retirement() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, grant, _) = crate::request_execution::tests::issued(&engine);
    let id = &plan.plan_id;
    let owner = engine.claim(id, || Ok(())).unwrap();
    let mut changes = engine.stop_changes.subscribe();
    let origin = grant.request_context.origin_node();
    assert_ne!(origin, "other-origin");
    assert!(
        engine
            .stop_job_request(id, Some("other-origin"))
            .await
            .is_err()
    );
    assert_eq!(json!(engine.store.get(id).unwrap()), json!(plan));
    assert!(!changes.has_changed().unwrap());

    let report = engine.stop_job_request(id, Some(origin)).await.unwrap();
    assert_eq!(report["status"], "stopping");
    assert_eq!(report["stop_requested"], true);
    assert_eq!(report["remote_stop_confirmed"], false);
    changes.changed().await.unwrap();
    let saved = engine.store.get(id).unwrap();
    assert_eq!(saved.status, PlanStatus::Stopping);
    assert!(saved.execution.unwrap().stop_requested_at_ms.is_some());
    drop(owner);
    let retired = engine.store.get(id).unwrap();
    assert_eq!(retired.status, PlanStatus::Interrupted);
    let execution = retired.execution.unwrap();
    assert_eq!(execution.attempts[0].grant, grant);
    assert!(execution.attempts[0].stop.is_none());
}

#[tokio::test]
async fn abandoned_public_stops_keep_native_capacity_without_revoking_or_writing() {
    use std::future::Future;
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = crate::workload_jobs::tests::planned(&engine);
    let id = &plan.plan_id;
    let owner = engine.claim_workload(id).unwrap();
    let (held, ready) = hold_grant_journal(engine.store.clone());
    ready.await.unwrap();
    let capacity = engine.store.stop_intent_available();
    let mut first = Box::pin(engine.cancel_before(id, None, stop_end()));
    let mut second = Box::pin(engine.cancel_before(id, None, stop_end()));
    std::future::poll_fn(|cx| {
        assert!(first.as_mut().poll(cx).is_pending());
        assert!(second.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.store.stop_intent_started() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(engine.store.stop_intent_available(), 0);
    assert!(
        engine
            .cancel_before(id, None, stop_end())
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity unavailable")
    );
    drop(first);
    drop(second);
    assert_eq!(engine.store.stop_intent_available(), 0);
    owner.check(id).unwrap();
    drop(held);
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.store.stop_intent_available() != capacity {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        engine.store.stop_intent_started(),
        1,
        "An abandoned queued request never reads the journal"
    );
    assert_eq!(json!(engine.store.get(id).unwrap()), json!(plan));
    owner.check(id).unwrap();
}
