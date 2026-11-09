use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[test]
fn route_options_and_confirmation_refuse_authority_and_preference_substitution() {
    let command = Options {
        origin_node: "input".into(),
        local_only: true,
        ..Default::default()
    };
    command.validate().unwrap();
    let mut bad = command.clone();
    bad.node_id = Some("other".into());
    assert!(bad.validate().is_err());
    bad = command.clone();
    bad.backend = Some("automatic-fallback".into());
    assert!(bad.validate().is_err());
    let runtime = uuid::Uuid::new_v4().to_string();
    let connection = Connection {
        node_id: "input".into(),
        runtime_id: uuid::Uuid::new_v4().to_string(),
        session_id: uuid::Uuid::new_v4().to_string(),
    };
    let report = Report {
        schema_version: 1,
        report_type: "interpreter_route_preview".into(),
        success: true,
        runtime_id: runtime.clone(),
        command: command.clone(),
        policy_id: uuid::Uuid::new_v4().to_string(),
        policy_generation: 1,
        policy_sha256: "a".repeat(64),
        issuer_id: "b".repeat(64),
        origin: connection.clone(),
        selected: Some(Selected {
            connection,
            backend: "ollama".into(),
            model_id: "chosen".into(),
            permission_revision: 0,
            grant_limits: Default::default(),
        }),
        grants_issued: false,
        model_inference_requested: false,
        allowance_reserved: false,
        automatic_delegation_available: false,
        automatically_retried: false,
    };
    let value = json!(report);
    assert!(confirmed(&value, &command, &runtime));
    for flag in [
        "grants_issued",
        "model_inference_requested",
        "allowance_reserved",
        "automatic_delegation_available",
        "automatically_retried",
    ] {
        let mut bad = value.clone();
        bad[flag] = json!(true);
        assert!(!confirmed(&bad, &command, &runtime));
    }
    let mut bad = value.clone();
    bad["selected"]["connection"]["node_id"] = json!("other");
    assert!(!confirmed(&bad, &command, &runtime));
    let mut bad = value;
    bad["untrusted_authority"] = json!(true);
    assert!(!confirmed(&bad, &command, &runtime));
}

#[tokio::test]
async fn private_model_free_origin_resolves_only_disclosed_policy_scoped_current_model_without_inference()
 {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let app = axum::Router::new()
        .route(
            "/api/tags",
            axum::routing::get(|| async { axum::Json(json!({"models":[{"name":"chosen"}]})) }),
        )
        .route(
            "/api/chat",
            axum::routing::post(move || {
                let called = called.clone();
                async move {
                    called.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!({"done":true,"message":{"content":"must not infer"}}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let model_server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
    let keys = std::collections::BTreeMap::from([
        ("input", eefn::authentication::generate_credential()),
        ("provider", eefn::authentication::generate_credential()),
        ("hidden", eefn::authentication::generate_credential()),
    ]);
    let config = dir.path().join("owner.json");
    std::fs::write(&config,serde_json::to_vec(&json!({
        "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,"tls":identity,"credentials":keys}},
        "discovery":{"grants":{"input":["provider"]},"freshness_seconds":30},
        "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
    })).unwrap()).unwrap();
    let runtime = Runtime::initialize(
        crate::config::Config::load(Some(&config)).unwrap(),
        dir.path().join("owner.db"),
        false,
    )
    .await
    .unwrap();
    // Port zero is only an in-process fixture bind request. Later credential
    // edits exercise the real saved-config validator, which requires an actual
    // endpoint. Persist the port assigned to this running fixture first.
    runtime
        .config
        .edit_for_restart(|saved| {
            saved["node"]["port"] = json!(runtime.node_server.port());
            Ok(((), true))
        })
        .unwrap();
    let mut nodes = Vec::new();
    for node in ["input", "provider", "hidden"] {
        let serving = node != "input";
        let selected:eefn::SelectedModel=serde_json::from_value(json!({"model_id":"chosen","modality":"text","selection":{"roles":["request_interpreter"]}})).unwrap();
        let mut settings = eefn::setup::defaults();
        settings["node_id"] = json!(node);
        settings["name"] = json!("Routing fixture");
        settings["permissions"]["interpreter_sharing"] = json!(serving);
        settings["models"] = json!({"provider":"ollama","ollama":{"base_url":url,"selected":if serving{vec![selected.clone()]}else{vec![]}},"llamacpp":{"slots":[]}});
        let path = dir.path().join(format!("{node}.json"));
        eefn::setup::write_json(&path, &settings).unwrap();
        let service = eefn::NodeService::new(path, node.into());
        let generation = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(generation);
        let engine = Arc::new(
            eefn::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(serde_json::from_value(settings["permissions"].clone()).unwrap())
                .unwrap()
                .with_ollama(
                    url.clone(),
                    if serving {
                        vec![selected.clone()]
                    } else {
                        vec![]
                    },
                )
                .unwrap()
                .with_service(service.clone()),
        );
        let client_config:eefn::NodeClientConfig=serde_json::from_value(json!({"node_id":node,"auth_mode":"node_credential","psk":keys[node],
            "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],
            "ollama_url":url,"ollama_models":if serving{vec![selected]}else{vec![]},"heartbeat_seconds":0.2})).unwrap();
        let client = eefn::NodeClient::new(client_config, engine)
            .unwrap()
            .with_status(service.live.clone())
            .with_registration_runtime(&generation)
            .unwrap()
            .with_grants(
                &dir.path().join(format!("{node}-grants.db")),
                Default::default(),
            );
        nodes.push(tokio::spawn(async move { client.connect_once().await }));
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if runtime.model_registry.list().len() == 2
                && runtime
                    .node_server
                    .interpreter_session("input", 30)
                    .await
                    .is_ok()
                && runtime
                    .node_server
                    .interpreter_session("provider", 30)
                    .await
                    .is_ok()
                && runtime
                    .node_server
                    .interpreter_session("hidden", 30)
                    .await
                    .is_ok()
            {
                break;
            }
            if let Some(index) = nodes.iter().position(|n| n.is_finished()) {
                let ended = nodes.swap_remove(index).await;
                panic!("Private node ended before registration: {ended:?}");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !runtime
            .node_server
            .interpreter_session("input", 30)
            .await
            .unwrap()
            .serves("ollama", "chosen")
    );
    runtime
        .model_registry
        .update_metrics("provider", 0.9, 100.0);
    runtime.model_registry.update_metrics("hidden", 0.0, 0.0);
    let approval = eefn::interpreter_policy::Command::Approve {
        operation_id: uuid::Uuid::new_v4().to_string(),
        generation: 0,
        scope: eefn::interpreter_policy::Scope {
            origin_nodes: vec!["input".into()],
            provider_nodes: vec!["provider".into(), "hidden".into()],
        },
        limits: Default::default(),
        valid_for_ms: None,
    };
    let saved = runtime
        .interpreter_policy_command(eefn::interpreter_policy::Request {
            schema_version: 1,
            expected_runtime_id: runtime.runtime_id().into(),
            command: approval.clone(),
        })
        .unwrap();
    let before = runtime.engine.store.usage().unwrap();
    let request = Request {
        schema_version: 1,
        expected_runtime_id: runtime.runtime_id().into(),
        command: Options {
            origin_node: "input".into(),
            ..Default::default()
        },
    };
    let result = runtime
        .interpreter_route_preview(request.clone())
        .await
        .unwrap();
    assert!(confirmed(&result, &request.command, runtime.runtime_id()));
    assert_eq!(
        result["selected"]["connection"]["node_id"], "provider",
        "A better-ranked hidden provider must remain undisclosed"
    );
    assert_eq!(result["selected"]["model_id"], "chosen");
    assert_eq!(result["selected"]["permission_revision"], 0);
    for (local, node, backend, model) in [
        (true, None, None, None),
        (false, Some("hidden"), None, None),
        (false, None, Some("llamacpp"), None),
        (false, None, None, Some("missing")),
    ] {
        let mut restricted = request.clone();
        restricted.command.local_only = local;
        restricted.command.node_id = node.map(str::to_owned);
        restricted.command.backend = backend.map(str::to_owned);
        restricted.command.model_id = model.map(str::to_owned);
        assert!(runtime.interpreter_route_preview(restricted).await.unwrap()["selected"].is_null());
    }
    // A registry entry cannot substitute a model absent from the current
    // authenticated registration, even if it claims the interpreter role.
    let mut forged = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
    forged.roles = Some(vec!["request_interpreter".into()]);
    runtime.model_registry.register_remote("provider",&[json!({"model_id":"invented","backend":"ollama","modality":"text","model_metadata":forged})]).unwrap();
    let mut invented = request.clone();
    invented.command.model_id = Some("invented".into());
    assert!(runtime.interpreter_route_preview(invented).await.unwrap()["selected"].is_null());
    assert_eq!(runtime.engine.store.usage().unwrap(), before);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    runtime
        .config
        .edit_for_restart(|saved| {
            saved["node"]["authentication"]["credentials"]["input"] = Value::Null;
            Ok(((), true))
        })
        .unwrap();
    assert!(
        runtime
            .interpreter_route_preview(request.clone())
            .await
            .is_err()
    );
    runtime
        .config
        .edit_for_restart(|saved| {
            saved["node"]["authentication"]["credentials"]["input"] = json!(keys["input"]);
            Ok(((), true))
        })
        .unwrap();
    runtime
        .interpreter_policy_command(eefn::interpreter_policy::Request {
            schema_version: 1,
            expected_runtime_id: runtime.runtime_id().into(),
            command: eefn::interpreter_policy::Command::Revoke {
                id: saved["authorization"]["id"].as_str().unwrap().into(),
                revision: 1,
                operation_id: uuid::Uuid::new_v4().to_string(),
            },
        })
        .unwrap();
    assert!(runtime.interpreter_route_preview(request).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for node in nodes {
        node.abort();
        let _ = node.await;
    }
    runtime.retire(Duration::from_secs(5)).await.unwrap();
    model_server.abort();
    let _ = model_server.await;
}
