// Real command services, original coordinator issuer, credential/TLS node and
// shipping source adapter. Disposable cv2/model stand-ins, never hardware.
async fn full_vision_model(
    listener: tokio::net::TcpListener,
    journal: std::path::PathBuf,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let mut entered = Some(entered);
    let mut release = Some(release);
    for expected in [
        "GET /api/tags HTTP/1.1",
        "POST /api/chat HTTP/1.1",
        "GET /api/tags HTTP/1.1",
    ] {
        let (socket, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept())
            .await
            .unwrap()
            .unwrap();
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut socket = BufReader::new(socket);
        let mut first = String::new();
        socket.read_line(&mut first).await.unwrap();
        assert_eq!(first.trim(), expected);
        let mut bytes = 0usize;
        let mut headers = 0;
        loop {
            let mut line = String::new();
            assert!(socket.read_line(&mut line).await.unwrap() > 0);
            headers += line.len();
            assert!(headers <= 16384);
            if line == "\r\n" {
                break;
            }
            if let Some((key, value)) = line.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    bytes = value.trim().parse().unwrap();
                }
            }
        }
        assert!(bytes <= 96 * 1024);
        let mut body = vec![0; bytes];
        socket.read_exact(&mut body).await.unwrap();
        let response = if expected.starts_with("POST") {
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["model"], "selected");
            assert_eq!(request["messages"][0]["content"], "Describe movement");
            assert_eq!(request["messages"][0]["images"], json!(["/9gBAgME/9k="]));
            let db = rusqlite::Connection::open_with_flags(
                &journal,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let pending: i64 = db
                .query_row(
                    "SELECT count(*) FROM execution_workload_analyses WHERE result IS NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                pending, 1,
                "Model exchange preceded the durable node intent"
            );
            entered.take().unwrap().send(()).unwrap();
            release.take().unwrap().await.unwrap();
            json!({"message":{"content":"movement detected"},"done":true,"done_reason":"stop"})
        } else {
            json!({"models":[{"name":"selected","digest":"a".repeat(64),"size":64}]})
        };
        let response = response.to_string();
        socket
            .get_mut()
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn reviewed_vision_commands_execute_once_and_deliver_durable_results_with_executor_offline() {
    // Keep native setup and the actual awaited workflow off nested test frames.
    Box::pin(full_vision_commands()).await;
}
async fn full_vision_commands() {
    use clap::Parser;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Parser)]
    struct VisionArgs {
        #[command(flatten)]
        input: eefn::vision_submission::VisionSubmission,
    }
    let dir = tempfile::tempdir().unwrap();
    let python = full_source_venv(dir.path()).await;
    let trace = dir.path().join("vision-trace");
    std::fs::create_dir(&trace).unwrap();
    let site = python
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Lib/site-packages");
    let stub = format!(
        "_eef_vision_fake_source={}\n_eef_vision_root={}\n_eef_vision_quiet=False\n{}",
        serde_json::to_string(include_str!("../../../python/tests/camera_motion_fakes.py"))
            .unwrap(),
        serde_json::to_string(&trace).unwrap(),
        include_str!("../../../python/tests/prepared_vision_device.py")
    );
    std::fs::write(site.join("cv2.py"), stub).unwrap();
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_url = format!("http://{}", provider_listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let model = tokio::spawn(full_vision_model(
        provider_listener,
        dir.path().join("node-grants.db"),
        calls.clone(),
        entered,
        released,
    ));
    let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
    let credential = eefn::authentication::generate_credential();
    let owner_path = dir.path().join("owner.json");
    std::fs::write(&owner_path,serde_json::to_vec(&json!({
        "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,"tls":identity,"credentials":{"origin":credential}}},
        "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
    })).unwrap()).unwrap();
    let runtime = Box::pin(crate::Runtime::initialize(
        crate::config::Config::load(Some(&owner_path)).unwrap(),
        dir.path().join("owner.db"),
        false,
    ))
    .await
    .unwrap();
    let metadata: eefn::context::NodeMetadata = serde_json::from_value(
        json!({"resources":[{"id":"camera","capability":"camera.capture",
        "parameters":{"device":"rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid/live"}}]}),
    )
    .unwrap();
    let selected: eefn::SelectedModel =
        serde_json::from_value(json!({"model_id":"selected","modality":"vlm"})).unwrap();
    let node_path = dir.path().join("node.json");
    std::fs::write(&node_path,serde_json::to_vec(&json!({"node_id":"origin","metadata":metadata,"python":python,
        "permissions":{"media":{"camera":true}},"listening":{"enabled":true},"workload_policy":{"enabled":true,"vision_analysis":true},
        "models":{"provider":"ollama","ollama":{"base_url":provider_url,"selected":[selected]}}})).unwrap()).unwrap();
    let service = eefn::NodeService::new(node_path, "origin".into());
    let mut policy = eefn::NodePolicy::default();
    policy.media.camera = true;
    let engine = Arc::new(
        eefn::NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_policy(policy)
            .unwrap()
            .with_resources("origin".into(), metadata)
            .unwrap()
            .with_service(service.clone())
            .with_ollama(provider_url, [selected])
            .unwrap()
            .with_python_runtime(Arc::new(
                eefn::python::PythonRuntime::discover(Some(python)).unwrap(),
            )),
    );
    let generation = uuid::Uuid::new_v4().to_string();
    let _binding = service.attach_preview_runtime(&engine, generation.clone());
    service.live.lock().unwrap()["runtime_id"] = json!(generation);
    let config:eefn::NodeClientConfig=serde_json::from_value(json!({"node_id":"origin","auth_mode":"node_credential","psk":credential,
        "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],"ollama_url":"http://127.0.0.1:9",
        "heartbeat_seconds":0.2})).unwrap();
    let node = eefn::NodeClient::new(config, engine)
        .unwrap()
        .with_submissions(service.submissions.clone())
        .with_registration_runtime(&generation)
        .unwrap()
        .with_grants(&dir.path().join("node-grants.db"), Default::default())
        .with_status(service.live.clone());
    let mut connection = tokio::spawn(async move { node.connect_once().await });
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if runtime
                .node_server
                .node_state("origin")
                .await
                .is_some_and(|s| s.registration["workload_vision_execution"]["available"] == true)
                && service.submissions.state() == eefn::submission::InputChannelState::Ready
            {
                break;
            }
            if connection.is_finished() {
                panic!(
                    "Vision node ended before readiness: {:?}",
                    (&mut connection).await
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = super::router_with_ui(runtime.clone(), false)
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    let api = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let digest = "a".repeat(64);
    let author = VisionArgs::try_parse_from([
        "probe",
        "--model",
        "selected",
        "--backend",
        "ollama",
        "--model-digest",
        &digest,
        "--prompt",
        "Describe movement",
        "--camera",
        "camera",
        "--frames",
        "3",
        "--max-result-bytes",
        "256",
        "--analysis-timeout-ms",
        "8000",
        "--max-duration-ms",
        "15000",
    ])
    .unwrap()
    .input
    .command()
    .unwrap();
    let authored = service
        .camera_request(eefn::camera_submission::CameraRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: author.clone(),
        })
        .await
        .unwrap();
    assert!(
        eefn::camera_submission::confirmed_node(&authored, "origin", &author),
        "{authored}"
    );
    assert_eq!(authored["success"], true, "{authored}");
    let id = authored["result"]["result"]["plan"]["id"].as_str().unwrap();
    let planned = runtime.engine.store.get(id).unwrap();
    let approval = ApprovalCommand {
        id: id.into(),
        expected_revision: planned.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        plan_sha256: planned.planning.as_ref().unwrap().plan_sha256.clone(),
        valid_for_ms: 120000,
    };
    let approved = full_source_post(
        &http,
        &base,
        "/api/commands/requests/approve-session",
        json!({"schema_version":2,
        "expected_runtime_id":runtime.runtime_id(),"command":approval}),
    )
    .await;
    assert!(crate::workload_approval::confirmed(
        &approved,
        runtime.runtime_id(),
        &approval
    ));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "Review/approval contacted the model"
    );
    assert!(
        !trace.join("trace").exists(),
        "Review/approval opened a camera"
    );
    let plan: Plan = serde_json::from_value(approved["plan"].clone()).unwrap();
    let command = execute(&plan);
    let submitted = service
        .execute_command(eefn::request_execution::ExecuteRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(submitted["success"], true, "{submitted}");
    tokio::time::timeout(Duration::from_secs(15), ready)
        .await
        .unwrap()
        .unwrap();
    let pending = full_source_wait_plan(&runtime, id, |p| {
        p.execution
            .as_ref()
            .and_then(|e| e.workload.as_ref())
            .is_some_and(|w| w.acknowledgement.is_some() && w.observations.len() >= 2)
    })
    .await;
    assert!(
        pending
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .completion
            .is_none()
    );
    tokio::time::sleep(Duration::from_millis(3600)).await;
    release.send(()).unwrap();
    let done = full_source_wait_plan(&runtime, id, |p| p.status == PlanStatus::Completed).await;
    tokio::time::timeout(Duration::from_secs(5), model)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "Model exchange retried or added unreviewed calls"
    );
    let w = done.execution.as_ref().unwrap().workload.as_ref().unwrap();
    let completed = w.completion.as_ref().unwrap();
    let history = completed.vision.as_ref().unwrap();
    assert_eq!(w.definition.schema_version, 3);
    assert_eq!(w.observations.len(), 3);
    assert_eq!(completed.report.schema_version, 3);
    assert!(
        completed.report.lease_sequence >= 2
            && completed.report.owned_process_exit_observed
            && !completed.report.physical_effects_stopped
    );
    assert_eq!(history.analyses[0].text(), Some("movement detected"));
    let result_command = eefn::request_result::ResultCommand {
        id: id.into(),
        grant_id: w.definition.grant_id.clone(),
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let delivered = service
        .request_result(eefn::request_result::ResultRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            request_id: request_id.clone(),
            command: result_command.clone(),
        })
        .await
        .unwrap();
    assert!(eefn::request_result::confirmed_node(
        &delivered,
        &result_command,
        "origin",
        &request_id
    ));
    assert_eq!(delivered["success"], true, "{delivered}");
    assert_eq!(
        delivered["result"]["vision"]["analyses"][0]["result"]["text"],
        "movement detected"
    );
    let saved = json!(done);
    let retry = service
        .execute_command(eefn::request_execution::ExecuteRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(retry["success"], true, "{retry}");
    assert_eq!(retry["result"]["reused"], true);
    assert_eq!(json!(runtime.engine.store.get(id).unwrap()), saved);
    connection.abort();
    let _ = connection.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.node_server.node_state("origin").await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let offline = full_source_post(
        &http,
        &base,
        "/api/commands/requests/result",
        json!({"schema_version":1,
        "expected_runtime_id":runtime.runtime_id(),"command":result_command}),
    )
    .await;
    assert!(eefn::request_result::confirmed(
        &offline,
        &result_command,
        Some("origin"),
        Some(runtime.runtime_id())
    ));
    assert!(!eefn::request_result::confirmed(
        &offline,
        &result_command,
        Some("another-origin"),
        Some(runtime.runtime_id())
    ));
    assert_eq!(offline, delivered["result"]);
    let raw = saved.to_string() + &offline.to_string();
    for forbidden in [
        "PRIVATE_CAMERA_PASSWORD",
        "rtsp://",
        "/9gBAgME/9k=",
        "\"image_base64\"",
    ] {
        assert!(
            !raw.contains(forbidden),
            "Private camera data entered coordinator storage/output"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(json!(runtime.engine.store.get(id).unwrap()), saved);
    api.abort();
    let _ = api.await;
    runtime.shutdown().await;
}
