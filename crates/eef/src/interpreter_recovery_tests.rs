use super::*;
use eefn::interpreter_recovery::Command as Recover;

async fn retained(f: &Fixture, stage: &str) -> (Task, Record, Recover) {
    f.model.method.store(stage == "concrete", Ordering::SeqCst);
    let task = f.task(stage).await;
    let participants = f.participants().await;
    let Exchange::Completed(mut completed) = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
        .await
        .unwrap()
    else {
        panic!("Missing completed semantic result");
    };
    let original = completed.record().clone();
    // An expired live continuation must not become recovery authority. Recovery
    // below is a separately authenticated, explicit source-edit operation.
    completed.continuation.reserved.deadline = std::time::Instant::now();
    drop(completed);
    let plan = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    let command = Recover {
        id: plan.plan_id,
        expected_revision: plan.revision,
        operation_id: uuid(),
        grant_id: original.id().into(),
    };
    (task, original, command)
}
async fn origin(f: &Fixture) -> eefn::interpreter_service::Session {
    f.runtime()
        .node_server
        .interpreter_session("input", 30)
        .await
        .unwrap()
}

#[tokio::test]
async fn retained_interpreter_recovery_applies_all_stages_offline_once_without_refunding_or_execution()
 {
    let f = Fixture::new().await;
    let mut pending = vec![];
    for stage in ["initial", "clarification", "method", "concrete"] {
        pending.push((stage, retained(&f, stage).await));
    }
    let ledger = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    f.nodes[1].abort();
    tokio::time::timeout(Duration::from_secs(5), async {
        while f
            .runtime()
            .node_server
            .interpreter_session("provider", 30)
            .await
            .is_ok()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.runtime()
        .interpreter_policy_command(eefn::interpreter_policy::Request {
            schema_version: 1,
            expected_runtime_id: f.runtime().runtime_id().into(),
            command: Command::Revoke {
                id: f.pin.id.clone(),
                revision: 1,
                operation_id: uuid(),
            },
        })
        .unwrap();
    for (stage, (task, original, command)) in pending {
        let (saved, reused) = f
            .runtime()
            .recover_interpreter_result(&command, &origin(&f).await)
            .await
            .unwrap();
        assert!(!reused);
        assert_eq!(saved.revision, command.expected_revision + 1);
        assert_eq!(
            saved.goal.request_context.as_ref(),
            Some(&task.snapshot.request_context)
        );
        assert!(saved.approval.is_none() && saved.execution.is_none());
        assert_eq!(saved.interpreter_recoveries.len(), 1);
        let receipt = &saved.interpreter_recoveries[0];
        assert_eq!(receipt.command, command);
        assert_eq!(receipt.binding_sha256, original.binding_sha256);
        assert_eq!(receipt.source_revision, task.snapshot.revision);
        assert_eq!(
            saved.status,
            if stage == "concrete" {
                PlanStatus::AwaitingApproval
            } else {
                PlanStatus::Review
            }
        );
        if stage == "initial" {
            assert_eq!(
                saved.input_interpretation.as_ref().unwrap().claim,
                crate::interpreter_recovery::initial_claim(&original).unwrap()
            );
        }
        let (history, reused) = f
            .runtime()
            .recover_interpreter_result(&command, &origin(&f).await)
            .await
            .unwrap();
        assert!(reused);
        assert_eq!(json!(history), json!(saved));
        assert_eq!(
            crate::jobs::view(&history, true)["interpreter_recoveries"][0],
            json!(receipt)
        );
        assert!(
            crate::jobs::view(&history, false)
                .get("interpreter_recoveries")
                .is_none()
        );
        let mut replacement = command.clone();
        replacement.operation_id = uuid();
        assert!(
            f.runtime()
                .recover_interpreter_result(&replacement, &origin(&f).await)
                .await
                .is_err()
        );
    }
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        json!(
            f.runtime()
                .engine
                .store
                .interpreter_allowance_records()
                .unwrap()
        ),
        json!(ledger)
    );
    f.close().await;
}

#[tokio::test]
async fn recovery_receipts_survive_later_compilation() {
    let f = Fixture::new().await;
    let (_, _, initial) = retained(&f, "initial").await;
    let (review, _) = f
        .runtime()
        .recover_interpreter_result(&initial, &origin(&f).await)
        .await
        .unwrap();
    f.model.method.store(true, Ordering::SeqCst);
    let method = Task::new(
        Snapshot {
            schema_version: 1,
            job_id: review.plan_id.clone(),
            revision: review.revision,
            operation_id: uuid(),
            request_context: review.goal.request_context.clone().unwrap(),
            allowed_capabilities: f
                .runtime()
                .disclosed_interpretation_capabilities("input")
                .await
                .unwrap(),
            source: Source::Method {
                draft: Box::new(review.request_review.clone().unwrap()),
            },
        },
        eefn::request_preparation::MAX_OUTPUT_TOKENS,
    )
    .unwrap();
    let Exchange::Completed(completed) = f
        .runtime()
        .exchange_interpreter(method, f.pin.clone(), &f.participants().await)
        .await
        .unwrap()
    else {
        panic!("Missing method result");
    };
    let id = completed.record().id().to_owned();
    drop(completed);
    let command = Recover {
        id: review.plan_id.clone(),
        expected_revision: review.revision,
        grant_id: id,
        operation_id: uuid(),
    };
    let (compiled, _) = f
        .runtime()
        .recover_interpreter_result(&command, &origin(&f).await)
        .await
        .unwrap();
    assert_eq!(compiled.status, PlanStatus::AwaitingApproval);
    assert!(compiled.approval.is_none() && compiled.execution.is_none());
    assert_eq!(compiled.interpreter_recoveries.len(), 2);
    assert_eq!(compiled.interpreter_recoveries[0].command, initial);
    assert_eq!(compiled.interpreter_recoveries[1].command, command);
    let (history, reused) = f
        .runtime()
        .recover_interpreter_result(&initial, &origin(&f).await)
        .await
        .unwrap();
    assert!(reused);
    assert_eq!(json!(history), json!(compiled));
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 2);
    f.close().await;
}

#[tokio::test]
async fn interpreter_recovery_refuses_live_stale_foreign_and_ordinary_promotion_paths() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let parties = f.participants().await;
    let Exchange::Completed(completed) = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &parties)
        .await
        .unwrap()
    else {
        panic!("Missing live result");
    };
    let original = completed.record().clone();
    let live = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    let mut command = Recover {
        id: live.plan_id.clone(),
        expected_revision: live.revision,
        grant_id: original.id().into(),
        operation_id: uuid(),
    };
    assert!(
        f.runtime()
            .recover_interpreter_result(&command, &parties.origin)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(live)
    );
    drop(completed);
    let before = f.runtime().engine.store.get(&command.id).unwrap();
    assert!(
        f.runtime()
            .recover_interpreter_result(&command, &parties.origin)
            .await
            .is_err()
    );
    command.expected_revision = before.revision;
    assert!(
        f.runtime()
            .recover_interpreter_result(&command, &parties.provider)
            .await
            .is_err()
    );
    let claim = crate::interpreter_recovery::initial_claim(&original).unwrap();
    let proposal: eefn::interpretation::Proposal = serde_json::from_value(
        original
            .report
            .as_ref()
            .unwrap()
            .data
            .pointer("/candidate/candidate/proposal")
            .unwrap()
            .clone(),
    )
    .unwrap();
    for transition in [
        crate::request_interpretation::Transition::Promote {
            claim: claim.clone(),
            expected_revision: before.revision,
            proposal: proposal.clone(),
        },
        crate::request_interpretation::Transition::Recover {
            claim,
            expected_revision: before.revision,
            proposal,
        },
    ] {
        assert!(
            f.runtime()
                .engine
                .transition_input_interpretation(
                    &command.id,
                    transition,
                    Some("input"),
                    &task.snapshot.allowed_capabilities
                )
                .is_err()
        );
    }
    let mut conflict = command.clone();
    conflict.operation_id = task.snapshot.operation_id.clone();
    assert!(
        f.runtime()
            .recover_interpreter_result(&conflict, &origin(&f).await)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(before)
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn interpreter_recovery_refuses_cancelled_sources_without_creating_a_receipt() {
    let f = Fixture::new().await;
    let (_, _, mut command) = retained(&f, "initial").await;
    f.runtime().engine.cancel(&command.id).unwrap();
    let stopped = f.runtime().engine.store.get(&command.id).unwrap();
    command.expected_revision = stopped.revision;
    assert!(
        f.runtime()
            .recover_interpreter_result(&command, &origin(&f).await)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(stopped)
    );
    assert!(stopped.interpreter_recoveries.is_empty());
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn interpreter_recovery_write_failure_is_atomic_and_explicit_retry_never_reinfers() {
    let f = Fixture::new().await;
    let (_, _, command) = retained(&f, "initial").await;
    let before = f.runtime().engine.store.get(&command.id).unwrap();
    let ledger = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    f.runtime().engine.store.with_test_journal_lock(|db| db.execute_batch(&format!(
        "CREATE TEMP TRIGGER reject_recovery BEFORE UPDATE ON durable_jobs WHEN OLD.job_id='{}' BEGIN SELECT RAISE(ABORT,'forced recovery write failure'); END;", command.id))).unwrap();
    assert!(
        f.runtime()
            .recover_interpreter_result(&command, &origin(&f).await)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(before)
    );
    assert_eq!(
        json!(
            f.runtime()
                .engine
                .store
                .interpreter_allowance_records()
                .unwrap()
        ),
        json!(ledger)
    );
    f.runtime()
        .engine
        .store
        .with_test_journal_lock(|db| db.execute_batch("DROP TRIGGER reject_recovery;"))
        .unwrap();
    let (saved, reused) = f
        .runtime()
        .recover_interpreter_result(&command, &origin(&f).await)
        .await
        .unwrap();
    assert!(!reused && saved.interpreter_recoveries.len() == 1);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn interpreter_recovery_full_storage_preserves_source_evidence_and_charges() {
    let f = Fixture::with_limits(Default::default(), Some(1024 * 1024)).await;
    let (_, _, command) = retained(&f, "initial").await;
    let before = f.runtime().engine.store.get(&command.id).unwrap();
    let ledger = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    let filler = crate::task::TaskPlanner
        .plan(
            serde_json::from_value(json!({
                "description":"quota fixture","template":"system_info","params":{},
                "request_context":{"request_id":uuid(),"origin_node":"input","origin_area":[]}
            }))
            .unwrap(),
        )
        .unwrap();
    let filler = f.runtime().engine.store.admit_request(filler).unwrap().plan;
    f.runtime()
        .engine
        .store
        .change(&filler.plan_id, "Pause synthetic quota fixture", |p| {
            p.status = PlanStatus::Paused;
            Ok(())
        })
        .unwrap();
    let available = f.runtime().engine.store.usage().unwrap()["available_bytes"]
        .as_u64()
        .unwrap() as usize;
    assert!(available > 2048);
    f.runtime()
        .engine
        .store
        .change(&filler.plan_id, "Consume fixture quota", |p| {
            p.goal.description = "x".repeat(available - 512);
            Ok(())
        })
        .unwrap();
    let error = f
        .runtime()
        .recover_interpreter_result(&command, &origin(&f).await)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("storage is full"), "{error}");
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(before)
    );
    assert_eq!(
        json!(
            f.runtime()
                .engine
                .store
                .interpreter_allowance_records()
                .unwrap()
        ),
        json!(ledger)
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn interpreter_recovery_scope_and_saved_receipts_reject_changed_evidence_and_erasure() {
    let f = Fixture::new().await;
    let (_, original, command) = retained(&f, "initial").await;
    assert!(crate::interpreter_recovery::validate_capabilities(&original, &[]).is_err());
    let mut forged = original.clone();
    forged.report.as_mut().unwrap().data["execution_authorized"] = json!(true);
    assert!(crate::interpreter_recovery::completed(&forged).is_err());
    let (saved, _) = f
        .runtime()
        .recover_interpreter_result(&command, &origin(&f).await)
        .await
        .unwrap();
    assert!(
        f.runtime()
            .engine
            .store
            .change(&command.id, "Cannot erase owner recovery", |p| {
                p.interpreter_recoveries.clear();
                Ok(())
            })
            .is_err()
    );
    let encoded = serde_json::to_string(&saved).unwrap();
    let mut tampered = saved.clone();
    tampered.interpreter_recoveries[0].report_sha256 = "f".repeat(64);
    f.runtime()
        .engine
        .store
        .with_test_journal_lock(|db| {
            db.execute(
                "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
                rusqlite::params![command.id, serde_json::to_string(&tampered).unwrap()],
            )
        })
        .unwrap();
    assert!(f.runtime().engine.store.get(&command.id).is_err());
    f.runtime()
        .engine
        .store
        .with_test_journal_lock(|db| {
            db.execute(
                "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
                rusqlite::params![command.id, encoded],
            )
        })
        .unwrap();
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(saved)
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}
