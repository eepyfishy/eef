use super::*;
use eefn::interpreter_command::{Command as InputCommand, Preferences, Request, Stage};

#[path = "interpreter_broker_history_tests.rs"]
mod history_tests;

fn normal_input(operation: &str) -> eefn::request_preview::PreviewRequest {
    eefn::request_preview::PreviewRequest {
        schema_version: 1,
        expected_node_id: "input".into(),
        request_id: operation.into(),
        text: "Inspect node status".into(),
        inference_timeout_seconds: Some(20),
    }
}
fn network_input_policy() -> eefn::request_input::InputPolicy {
    eefn::request_input::InputPolicy {
        prepare_on_input: true,
        resolve_on_answer: true,
        network_interpreter: true,
    }
}

#[tokio::test]
async fn normal_model_free_input_interprets_and_prepares_through_the_authorized_service() {
    let f = Fixture::with_input_policy(network_input_policy()).await;
    let mut provider = f
        .runtime()
        .registry
        .find("llm.infer", "run")
        .into_iter()
        .find(|p| p.node_id == "provider")
        .unwrap();
    provider.max_concurrent = 1;
    f.runtime().registry.register(provider);
    f.model.method.store(true, Ordering::SeqCst);
    let operation = uuid();
    let result = f.services[0].input_request(normal_input(&operation)).await;
    assert!(eefn::request_input::confirmed(
        &result,
        "input",
        &operation,
        "Inspect node status"
    ));
    assert_eq!(result["success"], true, "{result}");
    assert_eq!(result["status"], "awaiting_approval");
    assert_eq!(
        result["interpretation"]["network_interpreter_requested"],
        true
    );
    assert_eq!(result["interpretation"]["model_inference_requested"], false);
    for stage in ["interpretation", "preparation"] {
        assert_eq!(
            result[stage]["network_interpreter"]["result"]["attempt"]["provider"],
            "provider"
        );
    }
    let id = result["result"]["id"].as_str().unwrap();
    let saved = f.runtime().engine.store.get(id).unwrap();
    assert!(saved.approval.is_none() && saved.execution.is_none());
    assert_eq!(
        saved.goal.request_context.as_ref().unwrap().origin_node(),
        "input"
    );
    assert_eq!(
        saved.request_review.as_ref().unwrap().original_text,
        "Inspect node status"
    );
    let attempts = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap()
        .0;
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts
            .iter()
            .all(|r| r.binding.grant.limits.max_execution_ms == 20_000)
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 2);
    for field in ["node_id", "policy", "interpretation"] {
        let mut forged = result.clone();
        match field {
            "node_id" => forged["node_id"] = json!("foreign"),
            "policy" => forged["policy"]["network_interpreter"] = json!(false),
            _ => {
                forged["interpretation"]["checkpoint"]["record"]["claim"]["interpreter"]["node_id"] =
                    json!("foreign")
            }
        }
        assert!(
            !eefn::request_input::confirmed(&forged, "input", &operation, "Inspect node status"),
            "{field}"
        );
    }
    f.nodes[1].abort();
    let historical = f.services[0].input_request(normal_input(&operation)).await;
    assert_eq!(historical["success"], true, "{historical}");
    assert_eq!(historical["status"], "historical_snapshot");
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 2);
    f.close().await;
}

#[tokio::test]
async fn normal_network_answer_resolves_and_prepares_without_reentering_input_admission() {
    let f = Fixture::with_input_policy(network_input_policy()).await;
    f.model.question.store(true, Ordering::SeqCst);
    let operation = uuid();
    let first = f.services[0].input_request(normal_input(&operation)).await;
    assert_eq!(first["success"], true, "{first}");
    assert_eq!(first["status"], "clarification_required");
    f.model.question.store(false, Ordering::SeqCst);
    f.model.method.store(true, Ordering::SeqCst);
    let command = eefn::request_response::RespondCommand {
        id: first["result"]["id"].as_str().unwrap().into(),
        expected_revision: first["result"]["revision"].as_u64().unwrap(),
        operation_id: uuid(),
        text: "The input node".into(),
    };
    let submitted = || eefn::request_response::RespondRequest {
        schema_version: 1,
        expected_node_id: "input".into(),
        command: command.clone(),
        inference_timeout_seconds: Some(20),
    };
    let result = f.services[0].respond_request(submitted()).await;
    assert!(eefn::request_response::confirmed(
        &result, "input", &command
    ));
    assert_eq!(result["success"], true, "{result}");
    assert_eq!(result["status"], "awaiting_approval");
    assert_eq!(
        result["resolution"]["interpretation_source"],
        "coordinator_interpreter_service"
    );
    for stage in ["resolution", "preparation"] {
        assert_eq!(
            result[stage]["network_interpreter"]["result"]["attempt"]["provider"],
            "provider"
        );
    }
    let mut forged = result.clone();
    forged["policy"]["network_interpreter"] = json!(false);
    assert!(!eefn::request_response::confirmed(
        &forged, "input", &command
    ));
    let plan = f.runtime().engine.store.get(&command.id).unwrap();
    assert!(plan.approval.is_none() && plan.execution.is_none());
    assert_eq!(
        plan.request_review.as_ref().unwrap().answers[0].text,
        command.text
    );
    assert_eq!(
        plan.goal.request_context.as_ref().unwrap().request_id(),
        operation
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 3);
    let historical = f.services[0].respond_request(submitted()).await;
    assert_eq!(historical["success"], true, "{historical}");
    assert_eq!(historical["answer_reused"], true);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 3);
    f.close().await;
}

#[tokio::test]
async fn saved_network_preference_does_not_disclose_input_before_runtime_application() {
    let f = Fixture::new().await;
    let changed = f.services[0]
        .input_policy_command(eefn::request_input::PolicyRequest {
            schema_version: 1,
            expected_node_id: "input".into(),
            command: eefn::request_input::PolicyCommand::Set {
                prepare_on_input: None,
                resolve_on_answer: None,
                network_interpreter: Some(true),
            },
        })
        .unwrap();
    assert_eq!(changed["saved_policy"]["network_interpreter"], true);
    assert_eq!(changed["applied_policy"]["network_interpreter"], false);
    assert_eq!(changed["restart_required"], true);
    let result = f.services[0].input_request(normal_input(&uuid())).await;
    assert_eq!(result["success"], false);
    assert_eq!(result["source_confirmed"], true);
    assert_eq!(result["error_code"], "interpreter_unavailable");
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
    f.close().await;
}

#[tokio::test]
async fn normal_network_model_failure_retains_input_and_never_falls_back_or_replays() {
    let f = Fixture::with_input_policy(network_input_policy()).await;
    f.model.bad.store(true, Ordering::SeqCst);
    let operation = uuid();
    let result = f.services[0].input_request(normal_input(&operation)).await;
    assert_eq!(result["success"], false);
    assert_eq!(result["source_confirmed"], true);
    assert_eq!(
        result["interpretation"]["network_interpreter_requested"],
        true
    );
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    let id = result["raw_save"]["result"]["id"].as_str().unwrap();
    let saved = f.runtime().engine.store.get(id).unwrap();
    assert!(saved.request_review.is_none() && saved.tasks.is_empty() && saved.execution.is_none());
    let _inspected = f.services[0].input_request(normal_input(&operation)).await;
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .0
            .len(),
        1
    );
    f.close().await;
}

#[tokio::test]
async fn retiring_normal_network_input_keeps_uncertainty_and_cannot_continue_preparation() {
    let mut f = Fixture::with_input_policy(network_input_policy()).await;
    f.model.hold.store(true, Ordering::SeqCst);
    let operation = uuid();
    let local = f.services[0].clone();
    let submitted = operation.clone();
    let running = tokio::spawn(async move { local.input_request(normal_input(&submitted)).await });
    f.called().await;
    drop(f._preview_guards.remove(0));
    let result = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["outcome_unknown"], true);
    assert!(result["preparation"].is_null());
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    let current = f.services[0].input_request(normal_input(&operation)).await;
    assert_eq!(current["error_code"], "runtime_unavailable");
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .0
            .len(),
        1
    );
    f.close().await;
}

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
        let plain = eefn::interpreter_command::render(&result).unwrap();
        assert!(plain.contains("Interpreter node: provider"));
        if stage == "method" {
            assert!(plain.contains("Question: Which node?"));
            assert!(plain.contains(&format!(
                "eefn requests respond {} --revision {}",
                task.snapshot.job_id, result["result"]["view"]["revision"]
            )));
        } else if stage == "concrete" {
            assert!(plain.contains(&format!(
                "Review plan: eefn requests plan {}",
                task.snapshot.job_id
            )));
            assert!(!plain.contains("Question:"));
        }
        let mut unfinished = result.clone();
        unfinished["result"]["attempt"]["phase"] = json!("accepted");
        assert!(!eefn::interpreter_command::confirmed_node(
            &unfinished,
            &command
        ));
        if stage == "initial" {
            let mut stale = result.clone();
            stale["result"]["initial_checkpoint"]["record"]["claim"]["origin_runtime_id"] =
                json!(uuid());
            assert!(!eefn::interpreter_command::confirmed_node(&stale, &command));
        }
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
    let plain = eefn::interpreter_command::render(&result).unwrap();
    assert!(
        plain.contains("outcome is unconfirmed") && plain.contains("Inspect before trying again")
    );
    assert!(!plain.contains("Answer: eefn") && !plain.contains("Review plan:"));
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
