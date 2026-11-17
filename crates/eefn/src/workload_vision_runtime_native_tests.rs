//! Real node/lease/journal/process composition; fake camera, JPEG and model only.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<NodeService>,
    engine: Arc<NodeEngine>,
    runtime: String,
    definition: Definition,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(url: String, quiet: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("capture")).unwrap();
        let resource = crate::workload_leases::definition::camera_tests::resource();
        let metadata = crate::context::NodeMetadata {
            resources: vec![resource],
            ..Default::default()
        };
        let selected: crate::SelectedModel =
            serde_json::from_value(json!({"model_id":"selected","modality":"vlm"})).unwrap();
        let policy = crate::workload_leases::Policy {
            enabled: true,
            vision_analysis: true,
            ..Default::default()
        };
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"node","metadata":metadata,
            "permissions":{"media":{"camera":true}},"listening":{"enabled":true},"workload_policy":policy,
            "models":{"provider":"ollama","ollama":{"base_url":url,"selected":[selected]}}})).unwrap();
        let service = NodeService::new(path, "node".into());
        let executable = std::path::PathBuf::from(
            std::env::var_os("EEF_TEST_PYTHON")
                .expect("Explicit pinned Python required; no skipped native test"),
        );
        assert!(executable.is_absolute() && executable.is_file());
        let prelude = format!(
            "_eef_vision_fake_source={}\n_eef_vision_root={}\n_eef_vision_quiet={}\n{}",
            serde_json::to_string(include_str!("../../../python/tests/camera_motion_fakes.py"))
                .unwrap(),
            serde_json::to_string(&dir.path().join("capture")).unwrap(),
            if quiet { "True" } else { "False" },
            include_str!("../../../python/tests/prepared_vision_device.py")
        );
        let python = PythonRuntime::discover(Some(executable))
            .unwrap()
            .with_stream_test_prelude(prelude);
        let mut permissions = crate::NodePolicy::default();
        permissions.media.camera = true;
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(permissions)
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap()
                .with_service(service.clone())
                .with_ollama(url, [selected])
                .unwrap()
                .with_python_runtime(Arc::new(python)),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let mut definition = crate::workload_leases::definition::vision::tests::vision();
        definition.executor_node = "node".into();
        let Source::CameraVision { resource_id, .. } = &mut definition.source else {
            unreachable!()
        };
        *resource_id = "node::camera".into();
        definition.validate().unwrap();
        Self {
            dir,
            service,
            engine,
            runtime,
            definition,
            _guard: guard,
        }
    }
    fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("capture")
    }
    fn prepare(&self) -> Result<PreparedSource> {
        let (_, _, source) = self
            .engine
            .prepare_workload(&self.runtime, self.definition.clone())?;
        Ok(source)
    }
    fn vision_policy(&self, enabled: bool) {
        let request = crate::workload_policy::Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command: crate::workload_policy::Command::Show {},
        };
        let current = self
            .service
            .workload_policy_command(request.clone(), false)
            .unwrap();
        let mut policy: crate::workload_leases::Policy =
            serde_json::from_value(current["policy"].clone()).unwrap();
        policy.vision_analysis = enabled;
        self.service
            .workload_policy_command(
                crate::workload_policy::Request {
                    command: crate::workload_policy::Command::Set {
                        policy,
                        expected_revision: current["revision"].as_u64().unwrap(),
                        expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                    },
                    ..request
                },
                false,
            )
            .unwrap();
    }
    async fn consume(
        &self,
    ) -> (
        Arc<crate::grants::GrantLedger>,
        crate::grants::workload::Consumption,
    ) {
        let d = &self.definition;
        let ledger = crate::grants::GrantLedger::open(
            &self.dir.path().join("receipts.db"),
            Default::default(),
            "node",
        )
        .unwrap();
        let proof = crate::grants::workload::tests::proof_for(d, 50_000).await;
        let policy =
            crate::workload_policy::from_config(&self.service.read_config().unwrap()).unwrap();
        let consumed = ledger
            .consume_workload(
                d.clone(),
                &policy,
                proof,
                &d.issuer_id,
                &d.session_id,
                &d.executor_node,
                Instant::now() + std::time::Duration::from_secs(5),
            )
            .await
            .unwrap();
        (ledger, consumed)
    }
}
fn tags() -> Value {
    json!({"models":[{"name":"selected","digest":"a".repeat(64),"size":64}]})
}
fn answer() -> Value {
    json!({"message":{"content":"movement detected","thinking":"PRIVATE_REASONING"},"done":true,"done_reason":"stop"})
}
async fn provider(
    listener: tokio::net::TcpListener,
    responses: Vec<Value>,
    root: std::path::PathBuf,
    hold: Option<Arc<tokio::sync::Semaphore>>,
    started: Option<tokio::sync::oneshot::Sender<()>>,
) -> Vec<String> {
    let mut paths = Vec::new();
    let mut started = started;
    for body in responses {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = BufReader::new(socket);
        let mut first = String::new();
        socket.read_line(&mut first).await.unwrap();
        let mut bytes = 0usize;
        loop {
            let mut header = String::new();
            assert!(socket.read_line(&mut header).await.unwrap() > 0);
            if header == "\r\n" {
                break;
            }
            if let Some((key, value)) = header.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    bytes = value.trim().parse().unwrap()
                }
            }
        }
        assert!(bytes <= 65536);
        let mut request = vec![0; bytes];
        socket.read_exact(&mut request).await.unwrap();
        if first.starts_with("POST ") {
            let parsed: Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(
                parsed["messages"][0]["images"],
                json!([STANDARD.encode([0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9])])
            );
            assert!(!String::from_utf8_lossy(&request).contains("PRIVATE_CAMERA_PASSWORD"));
            let db = rusqlite::Connection::open(root.join("receipts.db")).unwrap();
            let pending: i64 = db
                .query_row(
                    "SELECT count(*) FROM execution_workload_analyses WHERE result IS NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(pending, 1, "Image reached model before durable intent");
            if let Some(started) = started.take() {
                let _ = started.send(());
            }
            if let Some(hold) = &hold {
                hold.acquire().await.unwrap().forget();
            }
        }
        paths.push(first.trim().to_owned());
        let body = body.to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.get_mut().write_all(reply.as_bytes()).await;
    }
    paths
}
async fn listener() -> (tokio::net::TcpListener, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (listener, url)
}
async fn reusable(f: &Fixture, processes: &crate::process_lifetime::tests::Processes) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(next) = f.service.camera_pool.reserve() {
                assert!(processes.exited_now());
                drop(next);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
async fn retired(ledger: &crate::grants::GrantLedger, d: &Definition) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if ledger.workload_reporting_ready()
                && ledger
                    .workload_receipt(&d.issuer_id, &d.grant_id)
                    .unwrap()
                    .receipt
                    .state
                    == "unknown"
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn admitted_vision_saves_intent_before_model_and_completes_exact_source_and_analysis_prefix()
{
    let (listener, url) = listener().await;
    let f = Fixture::new(url, false);
    let readiness: crate::vision_sources::Report = serde_json::from_value(
        f.service
            .vision_workload_sources(crate::vision_sources::Request {
                schema_version: 2,
                expected_node_id: "node".into(),
                expected_runtime_id: f.runtime.clone(),
            })
            .unwrap(),
    )
    .unwrap();
    assert!(readiness.configuration_ready);
    assert!(
        !readiness.model_contacted && !readiness.capture_started && !readiness.execution_available
    );
    readiness
        .confirms_template(&f.definition.template())
        .unwrap();
    let source = f.prepare().unwrap();
    assert!(!f.root().join("trace").exists());
    let (ledger, consumed) = f.consume().await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let (started, ready) = tokio::sync::oneshot::channel();
    let peer = provider(
        listener,
        vec![tags(), answer(), tags()],
        f.dir.path().into(),
        Some(hold.clone()),
        Some(started),
    );
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = events.clone();
    let saved = ledger.clone();
    let d = f.definition.clone();
    let run = async {
        let result = source
            .run(consumed, renewals, move |event| {
                let seen = seen.clone();
                let saved = saved.clone();
                let d = d.clone();
                async move {
                    let retained =
                        saved.workload_events(&d.issuer_id, &d.grant_id, event.sequence - 1, 1)?;
                    assert_eq!(retained["durable_events"][0]["event"], json!(event));
                    seen.lock().unwrap().push(event);
                    Ok(())
                }
            })
            .await;
        assert!(result.is_ok(), "Admitted vision failed: {result:?}");
        result.unwrap()
    };
    let inspect = async {
        ready.await.unwrap();
        let processes = crate::process_lifetime::tests::Processes::observe(&f.root()).await;
        processes.alive();
        assert!(f.service.camera_pool.reserve().is_err());
        assert!(f.service.microphone_pool.reserve().is_ok());
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(
            std::fs::read_to_string(f.root().join("trace")).unwrap(),
            "open\nread\nread\n"
        );
        hold.add_permits(1);
        processes
    };
    let (receipt, paths, processes) =
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            tokio::join!(run, peer, inspect)
        })
        .await
        .unwrap();
    assert_eq!(receipt.receipt.state, "completed");
    let completion = receipt.completion.as_ref().unwrap();
    assert_eq!(completion.schema_version, 3);
    assert_eq!(completion.source_observations, 3);
    let analysis = completion.analysis.as_ref().unwrap();
    assert_eq!(
        (analysis.calls, analysis.completed, analysis.result_bytes),
        (1, 1, 19)
    );
    assert!(completion.owned_process_exit_observed && !completion.physical_effects_stopped);
    assert_eq!(
        paths,
        [
            "GET /api/tags HTTP/1.1",
            "POST /api/chat HTTP/1.1",
            "GET /api/tags HTTP/1.1"
        ]
    );
    assert_eq!(events.lock().unwrap().len(), 3);
    reusable(&f, &processes).await;
    let reopened = crate::grants::GrantLedger::open(
        &f.dir.path().join("receipts.db"),
        Default::default(),
        "node",
    )
    .unwrap();
    assert_eq!(
        json!(
            reopened
                .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
                .unwrap()
        ),
        json!(receipt)
    );
    let history = reopened
        .workload_analyses(&f.definition.issuer_id, &f.definition.grant_id, 0, 1)
        .unwrap();
    assert_eq!(
        history["analyses"][0]["result"]["text"],
        "movement detected"
    );
    let raw = history.to_string() + &serde_json::to_string(&receipt).unwrap();
    for private in [
        "PRIVATE_CAMERA_PASSWORD",
        "PRIVATE_REASONING",
        "image_base64",
        &STANDARD.encode([0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9]),
    ] {
        assert!(!raw.contains(private));
    }
}

#[tokio::test]
async fn quiet_admitted_vision_completes_without_opening_model_connection() {
    let (listener, url) = listener().await;
    let f = Fixture::new(url, true);
    let source = f.prepare().unwrap();
    let (_ledger, consumed) = f.consume().await;
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let receipt = source
        .run(consumed, renewals, |_| async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(
        receipt
            .completion
            .as_ref()
            .unwrap()
            .analysis
            .as_ref()
            .unwrap()
            .calls,
        0
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    assert!(f.service.camera_pool.reserve().is_ok());
    assert_eq!(
        std::fs::read_to_string(f.root().join("trace")).unwrap(),
        "open\nread\nread\nread\nclose\n"
    );
}

#[tokio::test]
async fn incomplete_model_result_retires_source_and_retains_unreplayable_pending_analysis() {
    let (listener, url) = listener().await;
    let f = Fixture::new(url, false);
    let source = f.prepare().unwrap();
    let (ledger, consumed) = f.consume().await;
    let mut partial = answer();
    partial["done_reason"] = json!("length");
    partial["message"]["content"] = json!("PRIVATE_PARTIAL");
    let peer = provider(
        listener,
        vec![tags(), partial],
        f.dir.path().into(),
        None,
        None,
    );
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let (result, paths) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(source.run(consumed, renewals, |_| async { Ok(()) }), peer)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(paths.len(), 2);
    retired(&ledger, &f.definition).await;
    let history = ledger
        .workload_analyses(&f.definition.issuer_id, &f.definition.grant_id, 0, 1)
        .unwrap();
    assert_eq!(history["summary"]["calls"], 1);
    assert_eq!(history["summary"]["completed"], 0);
    assert_eq!(history["analyses"][0]["result"], Value::Null);
    assert!(!history.to_string().contains("PRIVATE_PARTIAL"));
    assert!(
        ledger
            .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
            .unwrap()
            .completion
            .is_none()
    );
    let proof = crate::grants::workload::tests::proof_for(&f.definition, 50_000).await;
    let d = &f.definition;
    let policy = crate::workload_policy::from_config(&f.service.read_config().unwrap()).unwrap();
    assert!(
        ledger
            .consume_workload(
                d.clone(),
                &policy,
                proof,
                &d.issuer_id,
                &d.session_id,
                &d.executor_node,
                Instant::now() + std::time::Duration::from_secs(3)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn owner_revocation_cancels_held_model_exchange_and_original_capture_tree_without_revival() {
    for model_change in [false, true] {
        let (listener, url) = listener().await;
        let mut f = Fixture::new(url, false);
        if let Source::CameraVision { parameters, .. } = &mut f.definition.source {
            parameters.inference.timeout_ms = 8000;
        }
        let source = f.prepare().unwrap();
        let (ledger, consumed) = f.consume().await;
        let hold = Arc::new(tokio::sync::Semaphore::new(0));
        let (started, ready) = tokio::sync::oneshot::channel();
        let (done, ended) = tokio::sync::oneshot::channel();
        let peer = provider(
            listener,
            vec![tags(), answer()],
            f.dir.path().into(),
            Some(hold.clone()),
            Some(started),
        );
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        let run = async {
            let result = source.run(consumed, renewals, |_| async { Ok(()) }).await;
            let _ = done.send(());
            result
        };
        let revoke = async {
            ready.await.unwrap();
            let processes = crate::process_lifetime::tests::Processes::observe(&f.root()).await;
            processes.alive();
            let original = f.service.read_config().unwrap();
            if model_change {
                let mut next = original.clone();
                next["models"]["ollama"]["selected"][0]["modality"] = json!("text");
                f.service.save_configuration(next).unwrap();
            } else {
                f.vision_policy(false);
            }
            tokio::time::timeout(std::time::Duration::from_secs(3), ended)
                .await
                .unwrap()
                .unwrap();
            if model_change {
                f.service.save_configuration(original).unwrap();
            } else {
                f.vision_policy(true);
            }
            hold.add_permits(1);
            processes
        };
        let (result, paths, processes) =
            tokio::time::timeout(std::time::Duration::from_secs(15), async {
                tokio::join!(run, peer, revoke)
            })
            .await
            .unwrap();
        assert!(result.is_err());
        assert_eq!(paths.len(), 2);
        reusable(&f, &processes).await;
        retired(&ledger, &f.definition).await;
        let history = ledger
            .workload_analyses(&f.definition.issuer_id, &f.definition.grant_id, 0, 1)
            .unwrap();
        assert_eq!(history["summary"]["completed"], 0);
        assert_eq!(history["state"], "unknown");
    }
}

#[tokio::test]
async fn vision_preparation_refuses_unapproved_or_remote_model_scope_before_capture() {
    let f = Fixture::new("http://127.0.0.1:1".into(), false);
    f.vision_policy(false);
    assert!(f.prepare().is_err());
    assert!(f.service.camera_pool.reserve().is_ok());
    assert!(!f.root().join("trace").exists());
    let remote = Fixture::new("http://example.invalid".into(), false);
    assert!(remote.prepare().is_err());
    assert!(remote.service.camera_pool.reserve().is_ok());
    assert!(!remote.root().join("trace").exists());
}
