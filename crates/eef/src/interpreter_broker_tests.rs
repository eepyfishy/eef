use super::*;
use eefn::interpreter_command::{Command as InputCommand, Preferences, Request, Stage};

async fn request(f: &Fixture, stage: &str) -> (Task, Request) {
    let task = f.task(stage).await;
    let parties = f.participants().await;
    let request = Request {
        schema_version: 1,
        expected_node_id: "input".into(),
        expected_runtime_id: parties.origin.runtime_id().into(),
        command: InputCommand {
            id: task.snapshot.job_id.clone(),
            stage: match stage {
                "initial" => Stage::Initial,
                "clarification" => Stage::Clarification,
                _ => Stage::Method,
            },
            expected_revision: if stage == "initial" {
                None
            } else {
                Some(task.snapshot.revision)
            },
            operation_id: task.snapshot.operation_id.clone(),
            preferences: Preferences::default(),
            maximum_execution_ms: None,
        },
    };
    (task, request)
}

#[tokio::test]
async fn caller_time_limit_reaches_the_original_grant_and_cannot_be_rewritten_by_replay() {
    let f = Fixture::new().await;
    let (_, mut command) = request(&f, "initial").await;
    command.command.maximum_execution_ms = Some(20_000);
    let result = f.services[0]
        .interpreter_command(command.clone())
        .await
        .unwrap();
    assert!(eefn::interpreter_command::confirmed_node(&result, &command));
    assert_eq!(result["success"], true);
    assert_eq!(result["result"]["attempt"]["execution_limit_ms"], 20_000);
    let (attempts, accounts) = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].binding.grant.limits.max_execution_ms, 20_000);
    assert_eq!(accounts[&f.pin.id].charged.execution_ms, 20_000);
    let mut shorter = command.clone();
    shorter.command.maximum_execution_ms = Some(19_999);
    let refused = f.services[0].interpreter_command(shorter).await.unwrap();
    assert_eq!(refused["success"], false);
    assert_eq!(refused["error_code"], "coordinator_rejected");
    let historical = f.services[0].interpreter_command(command).await.unwrap();
    assert_eq!(historical["success"], true);
    assert_eq!(historical["result"]["historical_snapshot"], true);
    assert_eq!(historical["result"]["model_inference_requested"], false);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .1[&f.pin.id]
            .charged
            .execution_ms,
        20_000
    );
    f.close().await;
}

#[tokio::test]
async fn model_free_node_commands_use_private_grants_for_all_stages_and_reuse_history_offline() {
    let f = Fixture::new().await;
    let mut requests = vec![];
    for (index, stage) in ["initial", "clarification", "method", "concrete"]
        .into_iter()
        .enumerate()
    {
        f.model.method.store(stage == "concrete", Ordering::SeqCst);
        let (task, command) = request(&f, stage).await;
        let result = f.services[0]
            .interpreter_command(command.clone())
            .await
            .unwrap();
        assert!(eefn::interpreter_command::confirmed_node(&result, &command));
        assert_eq!(result["success"], true, "{stage}: {result}");
        assert_eq!(result["coordinator_acknowledged"], true);
        assert_eq!(result["outcome_unknown"], false);
        assert_eq!(result["result"]["historical_snapshot"], false);
        assert_eq!(result["result"]["model_inference_requested"], true);
        assert_eq!(result["result"]["source_applied"], true);
        assert_eq!(result["result"]["attempt"]["provider"], "provider");
        assert_eq!(
            result["result"]["view"]["request_context"],
            json!(task.snapshot.request_context)
        );
        assert_eq!(result["result"]["view"]["id"], task.snapshot.job_id);
        assert_eq!(
            result["result"]["view"]["status"],
            if stage == "concrete" {
                "awaiting_approval"
            } else {
                "review"
            }
        );
        assert_eq!(f.model.calls.load(Ordering::SeqCst), index + 1);
        assert!(!result.to_string().contains("grant_sha256"));
        let saved = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
        assert!(saved.approval.is_none() && saved.execution.is_none() && saved.results.is_empty());
        requests.push(command);
    }
    f.nodes[1].abort();
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
    for command in requests {
        let result = f.services[0]
            .interpreter_command(command.clone())
            .await
            .unwrap();
        assert!(eefn::interpreter_command::confirmed_node(&result, &command));
        assert_eq!(
            result["success"], true,
            "History stage {:?}: {result}",
            command.command.stage
        );
        assert_eq!(result["result"]["historical_snapshot"], true);
        assert_eq!(result["result"]["model_inference_requested"], false);
        assert_eq!(result["result"]["view"]["reused"], false);
        let mut forged = result.clone();
        forged["result"]["view"]["reused"] = json!(true);
        assert!(!eefn::interpreter_command::confirmed_node(
            &forged, &command
        ));
        let mut conflict = command.clone();
        conflict.command.preferences.provider = Some("input".into());
        let denied = f.services[0].interpreter_command(conflict).await.unwrap();
        assert_eq!(denied["success"], false);
        assert_eq!(denied["error_code"], "coordinator_rejected");
    }
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .1[&f.pin.id]
            .charged
            .calls,
        4
    );
    f.close().await;
}

#[tokio::test]
async fn broker_rejects_foreign_sources_stale_sessions_wrong_runtime_and_unavailable_choices_before_inference()
 {
    let f = Fixture::new().await;
    let (task, command) = request(&f, "initial").await;
    let before = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    let mut foreign = command.clone();
    foreign.expected_node_id = "provider".into();
    foreign.expected_runtime_id = f.participants().await.provider.runtime_id().into();
    let result = f.services[1].interpreter_command(foreign).await.unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["error_code"], "coordinator_rejected");
    assert!(!result.to_string().contains("Inspect node status"));
    let mut stale = command.clone();
    stale.expected_runtime_id = uuid();
    let result = f.services[0].interpreter_command(stale).await.unwrap();
    assert_eq!(result["error_code"], "runtime_changed");
    assert_eq!(result["outcome_unknown"], false);
    let context =
        RequestContext::new(command.command.operation_id.clone(), "input".into(), vec![]).unwrap();
    let wire = eefn::interpreter_command::WireRequest {
        schema_version: 1,
        origin_runtime_id: command.expected_runtime_id.clone(),
        command: command.command.clone(),
    };
    assert!(
        f.runtime()
            .origin_interpreter_command(wire, &context, &uuid())
            .await
            .is_err()
    );
    for choice in ["local", "model", "provider"] {
        let mut bad = command.clone();
        match choice {
            "local" => bad.command.preferences.local_only = true,
            "model" => bad.command.preferences.model_id = Some("absent".into()),
            _ => bad.command.preferences.provider = Some("absent".into()),
        }
        let denied = f.services[0].interpreter_command(bad).await.unwrap();
        assert_eq!(denied["success"], false, "{choice}");
    }
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
async fn broker_stop_retains_uncertainty_and_repeated_command_cannot_repeat_inference() {
    let f = Fixture::new().await;
    let (task, command) = request(&f, "initial").await;
    f.model.hold.store(true, Ordering::SeqCst);
    let service = f.services[0].clone();
    let submitted = command.clone();
    let running = tokio::spawn(async move { service.interpreter_command(submitted).await });
    f.called().await;
    f.runtime().engine.cancel(&task.snapshot.job_id).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["outcome_unknown"], true);
    let reused = f.services[0]
        .interpreter_command(command.clone())
        .await
        .unwrap();
    assert!(eefn::interpreter_command::confirmed_node(&reused, &command));
    assert_eq!(reused["success"], false);
    assert_eq!(reused["result"]["historical_snapshot"], true);
    assert_eq!(reused["result"]["model_inference_requested"], false);
    let records = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap()
        .0;
    assert_eq!(records.len(), 1);
    assert!(records[0].stop.is_some());
    assert!(records[0].holds_slot());
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    let saved = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    assert!(saved.request_review.is_none() && saved.tasks.is_empty() && saved.execution.is_none());
    f.close().await;
}
