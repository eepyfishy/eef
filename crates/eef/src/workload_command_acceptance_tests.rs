// Command integration uses a real coordinator API and authenticated origin
// connections. The executor is an explicit synthetic receipt responder, not a
// source or a physical node. Node journal/TLS tests cover its separate reader.
#[tokio::test]
async fn owner_http_and_running_origin_receipt_commands_share_workload_history_without_execution() {
    use eefn::protocol::{
        build_auth, build_register, read_message, success_response, write_message,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("owner.json");
    let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
    let executor_key = eefn::authentication::generate_credential();
    let origin_key = eefn::authentication::generate_credential();
    let other_key = eefn::authentication::generate_credential();
    std::fs::write(&config_path,serde_json::to_vec(&json!({
        "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,
            "credentials":{"executor":executor_key,"origin":origin_key,"other-origin":other_key},"tls":identity}},
        "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
    })).unwrap()).unwrap();
    let runtime = crate::Runtime::initialize(
        crate::config::Config::load(Some(&config_path)).unwrap(),
        dir.path().join("owner.db"),
        false,
    )
    .await
    .unwrap();

    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", runtime.node_server.port()))
        .await
        .unwrap();
    eefn::authentication::write_selector(&mut socket, "executor")
        .await
        .unwrap();
    let stream = eefn::tls::connect(
        socket,
        &identity.certificate,
        eefn::tls::client_config(&identity.certificate).unwrap(),
    )
    .await
    .unwrap();
    let (input, mut output) = tokio::io::split(stream);
    let mut input = tokio::io::BufReader::new(input);
    let crypto = eefn::NodeCrypto::new(&executor_key).unwrap();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    write_message(&crypto, &mut output, &json!({"type":"probe","nonce":nonce}))
        .await
        .unwrap();
    let pong = read_message(&crypto, &mut input).await.unwrap().unwrap();
    assert_eq!(pong["type"], "pong");
    assert_eq!(pong["nonce"], nonce);
    write_message(
        &crypto,
        &mut output,
        &build_auth(&crypto, "executor", eefn::VERSION),
    )
    .await
    .unwrap();
    let auth = read_message(&crypto, &mut input).await.unwrap().unwrap();
    assert_eq!(auth["type"], "ok");
    let mut registration = build_register(
        "executor",
        "Synthetic receipt responder",
        eefn::VERSION,
        &[],
        &json!({}),
        &[],
    );
    registration["execution_grants"] = json!(eefn::grants::GrantAdvertisement {
        schema_version: 1,
        available: true,
        limits: Some(Default::default()),
        deadline_guarantee: "rpc_future_only".into(),
        timing_mode: Some(eefn::grant_time::MODE.into()),
        retained_results: true
    });
    registration["workload_receipts"] = json!(eefn::workload_inspection::Advertisement::new(true));
    write_message(&crypto, &mut output, &registration)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while runtime.node_server.grant_session("executor").await.is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let (plan, _) = planned(&runtime.engine);
    let approval = ApprovalCommand {
        id: plan.plan_id.clone(),
        expected_revision: plan.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
        valid_for_ms: 120000,
    };
    let approved = approve(&runtime.engine, &approval, &identity.fingerprint().unwrap()).unwrap();
    let issued = issue(
        &runtime.engine.store,
        &execute(&approved),
        auth["grant_session_id"].as_str().unwrap(),
        &policy(),
    )
    .unwrap();
    let d = issued.definition().clone();
    let receipt = synthetic_ack(&d);
    let served = Arc::new(AtomicUsize::new(0));
    let observed = served.clone();
    let expected = d.scope().unwrap();
    let responder = tokio::spawn(async move {
        while let Some(message) = read_message(&crypto, &mut input).await.unwrap() {
            assert_eq!(
                message["type"], "workload_receipt",
                "Receipt commands cannot execute, renew or stop a source"
            );
            assert_eq!(message["scope"], json!(expected));
            observed.fetch_add(1, Ordering::SeqCst);
            write_message(
                &crypto,
                &mut output,
                &success_response(message["id"].as_str().unwrap(), json!(receipt)),
            )
            .await
            .unwrap();
        }
    });
    let mut origins = Vec::new();
    let mut origin_tasks = Vec::new();
    for (node, key) in [("origin", origin_key), ("other-origin", other_key)] {
        let service = eefn::NodeService::new(dir.path().join(format!("{node}.json")), node.into());
        let engine = Arc::new(eefn::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let config:eefn::NodeClientConfig=serde_json::from_value(json!({"node_id":node,"psk":key,"auth_mode":"node_credential",
            "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],"ollama_url":"http://127.0.0.1:1"})).unwrap();
        let client = eefn::NodeClient::new(config, engine)
            .unwrap()
            .with_submissions(service.submissions.clone());
        origin_tasks.push(tokio::spawn(async move { client.connect_once().await }));
        tokio::time::timeout(Duration::from_secs(10), async {
            while runtime.node_server.node_state(node).await.is_none()
                || service.submissions.state() != eefn::submission::InputChannelState::Ready
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        origins.push(service);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = super::router_with_ui(runtime.clone(), false)
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    let http = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let command = eefn::request_receipt::ReceiptCommand {
        id: d.job_id.clone(),
        grant_id: d.grant_id.clone(),
    };
    let before = json!(runtime.engine.store.get(&d.job_id).unwrap());
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response=client.post(format!("http://{address}/api/commands/requests/receipt")).timeout(Duration::from_secs(10))
        .json(&json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":command})).send().await.unwrap();
    assert!(response.status().is_success());
    let owner_report: serde_json::Value = response.json().await.unwrap();
    assert!(eefn::request_receipt::confirmed(
        &owner_report,
        &command,
        None,
        Some(runtime.runtime_id())
    ));
    let mut origin_report = None;
    for (index, service) in origins.iter().enumerate() {
        let request_id = uuid::Uuid::new_v4().to_string();
        let report = tokio::time::timeout(
            Duration::from_secs(10),
            service.request_receipt(eefn::request_receipt::ReceiptRequest {
                schema_version: 1,
                expected_node_id: if index == 0 { "origin" } else { "other-origin" }.into(),
                request_id: request_id.clone(),
                command: command.clone(),
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(eefn::request_receipt::confirmed_node(
            &report,
            &command,
            if index == 0 { "origin" } else { "other-origin" },
            &request_id
        ));
        assert_eq!(report["success"], index == 0);
        if index == 0 {
            origin_report = Some(report["result"].clone())
        } else {
            assert_eq!(report["error_code"], "receipt_unavailable")
        }
    }
    assert_eq!(origin_report.unwrap(), owner_report);
    assert_eq!(
        served.load(Ordering::SeqCst),
        2,
        "Other origin is refused before contacting the executor"
    );
    assert_eq!(json!(runtime.engine.store.get(&d.job_id).unwrap()), before);
    assert_eq!(
        owner_report["coordinator_attempt_state"], "issued",
        "Read cannot acknowledge or reconcile the saved job"
    );
    assert_eq!(owner_report["receipt"]["receipt"]["state"], "accepted");
    assert!(
        !responder.is_finished(),
        "Synthetic executor must stay connected throughout both command paths"
    );
    http.abort();
    let _ = http.await;
    for task in origin_tasks {
        task.abort();
        let _ = task.await;
    }
    responder.abort();
    let _ = responder.await;
    drop(issued);
    runtime.shutdown().await;
}
