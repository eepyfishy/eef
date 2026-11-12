use super::*;
use crate::interpreter_allowance::Phase;
use crate::task::PlanStatus;
use eefn::interpreter_recovery::Command as Recover;
use eefn::interpreter_recovery_command::{Command as RecoveryCommand, Request as RecoveryRequest};

#[path = "interpreter_recovery_restart_tests.rs"]
mod restart_tests;

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
    f.runtime()
        .engine
        .store
        .wait_workload_finalizations_async()
        .await
        .unwrap();
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

async fn recovery_request(f: &Fixture, command: RecoveryCommand) -> RecoveryRequest {
    RecoveryRequest {
        schema_version: 1,
        expected_node_id: "input".into(),
        expected_runtime_id: origin(f).await.runtime_id().into(),
        command,
    }
}

#[tokio::test]
async fn active_network_inference_remains_inspectable_without_admitting_a_recovery_or_another_model_call()
 {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let source_id = task.snapshot.job_id.clone();
    let request = eefn::interpreter_command::Request {
        schema_version: 1,
        expected_node_id: "input".into(),
        expected_runtime_id: origin(&f).await.runtime_id().into(),
        command: eefn::interpreter_command::Command {
            id: source_id.clone(),
            stage: eefn::interpreter_command::Stage::Initial,
            expected_revision: Some(task.snapshot.revision),
            operation_id: task.snapshot.operation_id.clone(),
            preferences: Default::default(),
            maximum_execution_ms: None,
        },
    };
    f.model.hold.store(true, Ordering::SeqCst);
    let service = f.services[0].clone();
    let running = tokio::spawn(async move { service.interpreter_command(request).await });
    f.called().await;
    let attempt = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(record) = f
                .runtime()
                .engine
                .store
                .interpreter_allowance_records()
                .unwrap()
                .0
                .into_iter()
                .find(|r| r.binding.grant.job_id == source_id && r.phase == Phase::Accepted)
            {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let source = f.runtime().engine.store.get(&source_id).unwrap();
    let ledger = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    let inspect = recovery_request(
        &f,
        RecoveryCommand::Inspect {
            id: source_id.clone(),
            operation_id: uuid(),
        },
    )
    .await;
    let snapshot = tokio::time::timeout(
        Duration::from_secs(5),
        f.services[0].interpreter_recovery_command(inspect.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(eefn::interpreter_recovery_command::confirmed_node(
        &snapshot, &inspect
    ));
    assert_eq!(snapshot["success"], true);
    assert_eq!(
        snapshot["result"]["attempts"][0]["attempt"]["phase"],
        "accepted"
    );
    assert_eq!(snapshot["result"]["attempts"][0]["completed_result"], false);
    assert_eq!(snapshot["result"]["attempts"][0]["source_applied"], false);
    let recover = recovery_request(
        &f,
        RecoveryCommand::Recover {
            command: Recover {
                id: source_id.clone(),
                expected_revision: source.revision,
                operation_id: uuid(),
                grant_id: attempt.id().into(),
            },
        },
    )
    .await;
    let busy = f.services[0]
        .interpreter_recovery_command(recover)
        .await
        .unwrap();
    assert_eq!(busy["error_code"], "busy");
    assert_eq!(busy["outcome_unknown"], false);
    assert_eq!(
        json!(f.runtime().engine.store.get(&source_id).unwrap()),
        json!(source)
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
    f.model.gate.add_permits(1);
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["result"]["source_applied"], true);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn explicit_recovery_commands_inspect_then_apply_all_stages_once_without_inference_or_execution()
 {
    let f = Fixture::new().await;
    for stage in ["initial", "clarification", "method", "concrete"] {
        let (_, original, command) = retained(&f, stage).await;
        let before = f.runtime().engine.store.get(&command.id).unwrap();
        let ledger = f
            .runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap();
        let inspect = recovery_request(
            &f,
            RecoveryCommand::Inspect {
                id: command.id.clone(),
                operation_id: uuid(),
            },
        )
        .await;
        let result = f.services[0]
            .interpreter_recovery_command(inspect.clone())
            .await
            .unwrap();
        assert!(eefn::interpreter_recovery_command::confirmed_node(
            &result, &inspect
        ));
        assert_eq!(result["success"], true);
        assert_eq!(result["result"]["attempts"].as_array().unwrap().len(), 1);
        let header = &result["result"]["attempts"][0];
        assert_eq!(header["attempt"]["id"], original.id());
        assert_eq!(header["completed_result"], true);
        assert_eq!(header["source_applied"], false);
        for secret in [
            "Inspect node status",
            "messages",
            "binding_sha256",
            "grant_limits",
            "source_sha256",
        ] {
            assert!(!result.to_string().contains(secret), "{secret}");
        }
        assert_eq!(
            json!(f.runtime().engine.store.get(&command.id).unwrap()),
            json!(before)
        );
        let apply = recovery_request(
            &f,
            RecoveryCommand::Recover {
                command: command.clone(),
            },
        )
        .await;
        for reused in [false, true] {
            let result = f.services[0]
                .interpreter_recovery_command(apply.clone())
                .await
                .unwrap();
            assert!(eefn::interpreter_recovery_command::confirmed_node(
                &result, &apply
            ));
            assert_eq!(result["success"], true);
            assert_eq!(result["result"]["reused"], reused);
            assert_eq!(result["result"]["recovery"]["command"], json!(command));
            for flag in [
                "model_inference_requested",
                "execution_authorized",
                "dispatched",
                "automatically_retried",
            ] {
                assert_eq!(result[flag], false);
                assert_eq!(result["result"][flag], false);
            }
            assert!(
                eefn::interpreter_recovery_command::render(&result)
                    .unwrap()
                    .contains("Saved result recovered")
            );
            assert_eq!(
                eefn::interpreter_recovery_command::render(&result)
                    .unwrap()
                    .contains("previously saved recovery decision"),
                reused
            );
        }
        let updated = f.runtime().engine.store.get(&command.id).unwrap();
        assert_eq!(updated.revision, command.expected_revision + 1);
        assert!(updated.approval.is_none() && updated.execution.is_none());
        let after = f.services[0]
            .interpreter_recovery_command(inspect)
            .await
            .unwrap();
        assert_eq!(after["result"]["attempts"][0]["source_applied"], true);
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
    }
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 4);
    f.close().await;
}

#[tokio::test]
async fn recovery_command_origin_session_runtime_and_revision_fences_preserve_source() {
    let f = Fixture::new().await;
    let (_, _, command) = retained(&f, "initial").await;
    let before = f.runtime().engine.store.get(&command.id).unwrap();
    let inspect = recovery_request(
        &f,
        RecoveryCommand::Inspect {
            id: command.id.clone(),
            operation_id: uuid(),
        },
    )
    .await;
    let mut foreign = inspect.clone();
    foreign.expected_node_id = "provider".into();
    foreign.expected_runtime_id = f.participants().await.provider.runtime_id().into();
    let rejected = f.services[1]
        .interpreter_recovery_command(foreign)
        .await
        .unwrap();
    assert_eq!(rejected["success"], false);
    assert_eq!(rejected["error_code"], "coordinator_rejected");
    assert!(!rejected.to_string().contains("chosen"));
    assert!(rejected["result"].is_null());
    let mut stale = inspect.clone();
    stale.expected_runtime_id = uuid();
    let rejected = f.services[0]
        .interpreter_recovery_command(stale)
        .await
        .unwrap();
    assert_eq!(rejected["error_code"], "runtime_changed");
    assert_eq!(rejected["outcome_unknown"], false);
    let context = RequestContext::new(
        inspect.command.operation_id().into(),
        "input".into(),
        vec![],
    )
    .unwrap();
    assert!(
        f.runtime()
            .origin_interpreter_recovery_command(
                eefn::interpreter_recovery_command::WireRequest {
                    schema_version: 1,
                    origin_runtime_id: inspect.expected_runtime_id,
                    command: inspect.command,
                },
                &context,
                &uuid()
            )
            .await
            .is_err()
    );
    let mut wrong = command.clone();
    wrong.expected_revision += 1;
    let apply = recovery_request(&f, RecoveryCommand::Recover { command: wrong }).await;
    let rejected = f.services[0]
        .interpreter_recovery_command(apply)
        .await
        .unwrap();
    assert_eq!(rejected["success"], false);
    assert_eq!(rejected["error_code"], "coordinator_rejected");
    assert_eq!(
        json!(f.runtime().engine.store.get(&command.id).unwrap()),
        json!(before)
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn model_free_recovery_inspection_does_not_start_inference_or_create_an_attempt() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let before = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    let inspect = recovery_request(
        &f,
        RecoveryCommand::Inspect {
            id: task.snapshot.job_id.clone(),
            operation_id: uuid(),
        },
    )
    .await;
    let result = f.services[0]
        .interpreter_recovery_command(inspect.clone())
        .await
        .unwrap();
    assert!(eefn::interpreter_recovery_command::confirmed_node(
        &result, &inspect
    ));
    assert_eq!(result["success"], true);
    assert_eq!(result["result"]["attempts"], json!([]));
    assert!(
        eefn::interpreter_recovery_command::render(&result)
            .unwrap()
            .contains("No saved interpreter attempts for this request")
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
    assert!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .0
            .is_empty()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&task.snapshot.job_id).unwrap()),
        json!(before)
    );
    f.close().await;
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
    f.runtime()
        .engine
        .store
        .wait_workload_finalizations_async()
        .await
        .unwrap();
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
    let inspect = recovery_request(
        &f,
        RecoveryCommand::Inspect {
            id: compiled.plan_id.clone(),
            operation_id: uuid(),
        },
    )
    .await;
    let view = f.services[0]
        .interpreter_recovery_command(inspect.clone())
        .await
        .unwrap();
    assert!(eefn::interpreter_recovery_command::confirmed_node(
        &view, &inspect
    ));
    assert_eq!(view["success"], true);
    assert_eq!(view["result"]["revision"], compiled.revision);
    let headers = view["result"]["attempts"].as_array().unwrap();
    assert_eq!(headers.len(), 2);
    assert!(headers.iter().all(|a| a["source_applied"] == true
        && a["completed_result"] == true
        && a["attempt"]["source_revision"].as_u64().unwrap() <= compiled.revision));
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
    f.runtime()
        .engine
        .store
        .wait_workload_finalizations_async()
        .await
        .unwrap();
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
