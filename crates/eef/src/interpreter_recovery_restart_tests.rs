//! Real temporary journals and credential/TLS reconnect; no installed runtime.
use super::*;

async fn restart_origin(f: &mut Fixture) {
    for node in f.nodes.drain(..) {
        node.abort();
        let _ = node.await;
    }
    f._preview_guards.clear();
    f.services.clear();
    f.runtime
        .take()
        .unwrap()
        .retire(Duration::from_secs(5))
        .await
        .unwrap();
    let owner_path = f._dir.path().join("owner.json");
    let owner: Value = serde_json::from_slice(&std::fs::read(&owner_path).unwrap()).unwrap();
    let runtime = Runtime::initialize(
        crate::config::Config::load(Some(&owner_path)).unwrap(),
        f._dir.path().join("owner.db"),
        false,
    )
    .await
    .unwrap();
    let path = f._dir.path().join("input.json");
    let settings: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let url = settings["models"]["ollama"]["base_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let service = eefn::NodeService::new(path, "input".into());
    let generation = uuid();
    service.live.lock().unwrap()["runtime_id"] = json!(generation);
    let engine = Arc::new(
        eefn::NodeEngine::new(false, vec![], f._dir.path().into())
            .unwrap()
            .with_policy(serde_json::from_value(settings["permissions"].clone()).unwrap())
            .unwrap()
            .with_ollama(url.clone(), vec![])
            .unwrap()
            .with_service(service.clone()),
    );
    let config: eefn::NodeClientConfig = serde_json::from_value(json!({
        "node_id":"input","auth_mode":"node_credential",
        "psk":owner["node"]["authentication"]["credentials"]["input"],
        "coordinator_certificate":owner["node"]["authentication"]["tls"]["certificate"],
        "endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],
        "ollama_url":url,"ollama_models":[],"heartbeat_seconds":0.2,
    }))
    .unwrap();
    f._preview_guards
        .push(service.attach_preview_runtime_with_policy(
            &engine,
            generation.clone(),
            serde_json::from_value(settings["input_workflow"].clone()).unwrap(),
        ));
    let client = eefn::NodeClient::new(config, engine)
        .unwrap()
        .with_submissions(service.submissions.clone())
        .with_status(service.live.clone())
        .with_registration_runtime(&generation)
        .unwrap()
        .with_grants(&f._dir.path().join("input-grants.db"), Default::default());
    // Open the existing journal and reconnect only the model-free source node.
    // No provider is restarted and no saved inference policy is broadened.
    f.nodes
        .push(tokio::spawn(async move { client.connect_once().await }));
    f.services.push(service);
    f.runtime = Some(runtime);
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if f.runtime()
                .node_server
                .interpreter_session("input", 30)
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                !f.nodes[0].is_finished(),
                "Restarted origin ended before authenticated registration"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn interpreter_recovery_and_read_only_decisions_survive_real_journal_and_origin_restart() {
    let mut f = Fixture::new().await;
    let (_, original, pending) = retained(&f, "initial").await;
    let (_, _, applied) = retained(&f, "initial").await;
    let previous = recovery_request(
        &f,
        RecoveryCommand::Recover {
            command: applied.clone(),
        },
    )
    .await;
    let first = f.services[0]
        .interpreter_recovery_command(previous.clone())
        .await
        .unwrap();
    assert_eq!(first["success"], true);
    assert_eq!(first["result"]["reused"], false);
    let applied_before = f.runtime().engine.store.get(&applied.id).unwrap();
    let pending_before = f.runtime().engine.store.get(&pending.id).unwrap();
    let ledger = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap();
    let old_runtime = f.runtime().runtime_id().to_owned();
    let old_origin = origin(&f).await.runtime_id().to_owned();
    restart_origin(&mut f).await;
    assert_ne!(f.runtime().runtime_id(), old_runtime);
    assert_ne!(origin(&f).await.runtime_id(), old_origin);
    assert!(f.runtime().model_registry.list().is_empty());
    assert!(
        f.runtime()
            .node_server
            .interpreter_session("provider", 30)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&applied.id).unwrap()),
        json!(applied_before)
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&pending.id).unwrap()),
        json!(pending_before)
    );
    let stale = f.services[0]
        .interpreter_recovery_command(previous)
        .await
        .unwrap();
    assert_eq!(stale["error_code"], "runtime_changed");
    assert_eq!(stale["outcome_unknown"], false);
    let inspect = recovery_request(
        &f,
        RecoveryCommand::Inspect {
            id: pending.id.clone(),
            operation_id: uuid(),
        },
    )
    .await;
    let snapshot = f.services[0]
        .interpreter_recovery_command(inspect.clone())
        .await
        .unwrap();
    assert!(eefn::interpreter_recovery_command::confirmed_node(
        &snapshot, &inspect
    ));
    assert_eq!(
        snapshot["result"]["attempts"][0]["attempt"]["id"],
        original.id()
    );
    assert_eq!(snapshot["result"]["attempts"][0]["completed_result"], true);
    for (command, reused) in [(pending.clone(), false), (applied.clone(), true)] {
        let req = recovery_request(
            &f,
            RecoveryCommand::Recover {
                command: command.clone(),
            },
        )
        .await;
        let response = f.services[0]
            .interpreter_recovery_command(req.clone())
            .await
            .unwrap();
        assert!(eefn::interpreter_recovery_command::confirmed_node(
            &response, &req
        ));
        assert_eq!(response["success"], true);
        assert_eq!(response["result"]["reused"], reused);
        assert_eq!(response["result"]["runtime_id"], f.runtime().runtime_id());
        assert_eq!(response["result"]["recovery"]["command"], json!(command));
    }
    let recovered = f.runtime().engine.store.get(&pending.id).unwrap();
    assert_eq!(recovered.revision, pending.expected_revision + 1);
    assert_eq!(
        recovered
            .input_interpretation
            .unwrap()
            .claim
            .origin_runtime_id,
        old_origin
    );
    assert_eq!(
        json!(f.runtime().engine.store.get(&applied.id).unwrap()),
        json!(applied_before)
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
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 2);
    f.close().await;
}
