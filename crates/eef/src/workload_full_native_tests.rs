// Opt-in Windows acceptance: real owner/origin commands, coordinator driver,
// credential/TLS NodeClient and shipping source adapters. Only simulated device
// dependencies live in a disposable venv; no production test hook or device opens.
#[derive(Clone, Copy)]
enum FullSourceKind {
    Microphone,
    Camera,
}
impl FullSourceKind {
    fn camera(self) -> bool {
        matches!(self, Self::Camera)
    }
    fn capability(self) -> &'static str {
        if self.camera() {
            "camera.capture"
        } else {
            "audio.capture"
        }
    }
    fn event_count(self) -> usize {
        if self.camera() { 5 } else { 4 }
    }
}
async fn full_source_venv(root: &std::path::Path) -> std::path::PathBuf {
    let python = std::path::PathBuf::from(
        std::env::var_os("EEF_TEST_PYTHON")
            .expect("Explicit pinned Python is required; no skipped pass"),
    );
    assert!(python.is_absolute() && python.is_file());
    let target = root.join("source-venv");
    let mut command = tokio::process::Command::new(python);
    command
        .args(["-I", "-B", "-m", "venv", "--without-pip"])
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .kill_on_drop(true);
    let started = std::time::Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut child = command.spawn().unwrap_or_else(|error| {
        panic!(
            "Disposable stdlib venv spawn failed: kind={:?}",
            error.kind()
        )
    });
    let spawn_ms = started.elapsed().as_millis();
    let result = tokio::time::timeout_at(deadline, child.wait()).await;
    match result {
        Ok(Ok(status)) => assert!(
            status.success(),
            "Disposable stdlib venv exited unsuccessfully: code={:?}, spawn_ms={spawn_ms}, elapsed_ms={}",
            status.code(),
            started.elapsed().as_millis()
        ),
        Ok(Err(error)) => panic!(
            "Disposable stdlib venv wait failed: kind={:?}, spawn_ms={spawn_ms}, elapsed_ms={}",
            error.kind(),
            started.elapsed().as_millis()
        ),
        Err(_) => {
            // Observation only; an expired setup remains failed even if its
            // exact owned child finishes during the bounded failure cleanup.
            let elapsed_ms = started.elapsed().as_millis();
            let exited_before_kill = child.try_wait().ok().flatten().is_some();
            let _ = child.start_kill();
            let cleanup_observed = matches!(
                tokio::time::timeout(Duration::from_secs(3), child.wait()).await,
                Ok(Ok(_))
            );
            panic!(
                "Disposable stdlib venv setup deadline expired: original_budget_ms=30000, elapsed_ms={elapsed_ms}, spawn_ms={spawn_ms}, config_present={}, executable_present={}, site_packages_present={}, exited_before_kill={exited_before_kill}, cleanup_observed={cleanup_observed}",
                target.join("pyvenv.cfg").is_file(),
                target.join("Scripts/python.exe").is_file(),
                target.join("Lib/site-packages").is_dir()
            );
        }
    }
    let executable = target.join("Scripts/python.exe");
    assert!(executable.is_file());
    executable
}
fn full_source_dependency(
    venv_python: &std::path::Path,
    trace: &std::path::Path,
    mode: &str,
    kind: FullSourceKind,
) {
    let site = venv_python
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Lib/site-packages");
    assert!(site.is_dir());
    if kind.camera() {
        let source = format!(
            "_eef_camera_root = {}\n_eef_camera_mode = {}\n_eef_camera_fake_source = {}\n{}",
            serde_json::to_string(trace).unwrap(),
            serde_json::to_string(mode).unwrap(),
            serde_json::to_string(include_str!("../../../python/tests/camera_motion_fakes.py"))
                .unwrap(),
            include_str!("../../../python/tests/prepared_camera_device.py")
        );
        std::fs::write(site.join("cv2.py"), source).unwrap();
        return;
    }
    let source = format!(
        "_eef_fixture_root = {}\n_eef_fixture_mode = {}\n{}",
        serde_json::to_string(trace).unwrap(),
        serde_json::to_string(mode).unwrap(),
        include_str!("../../../python/tests/prepared_source_device.py")
    );
    std::fs::write(site.join("sounddevice.py"), source).unwrap();
}
async fn full_source_post(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    let response = client
        .post(format!("{base}{path}"))
        .json(&body)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap();
    assert!(status.is_success(), "Command {path} failed: {value}");
    value
}
async fn full_source_plan(
    runtime: &crate::Runtime,
    service: &eefn::NodeService,
    client: &reqwest::Client,
    base: &str,
    kind: FullSourceKind,
    phase: &str,
) -> Plan {
    let started = std::time::Instant::now();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let mut template = Template::from_params(&template_input().params).unwrap();
    template.limits.max_source_frames = 400;
    let (result, confirmed) = if kind.camera() {
        let submission = eefn::camera_submission::CameraCommand {
            operation_id: operation_id.clone(),
            input: eefn::camera_submission::CameraInput {
                schema_version: 1,
                original_text: "Observe camera motion for an explicitly bounded session".into(),
                title: "Bounded camera source".into(),
                source_node: None,
                camera: None,
                priority: 0,
                parameters: eefn::workload_leases::definition::MotionParameters {
                    width: 640,
                    height: 480,
                    sample_width: 64,
                    sample_height: 48,
                    max_frames: 5,
                    interval_ms: 200,
                    activation_level: 15,
                    deactivation_level: 5,
                    io_timeout_ms: 2000,
                    // This fixture deliberately holds the first frame through
                    // a lease renewal and waits for each durable event save.
                    // Give that synthetic handshake its own reviewed bound;
                    // the 10s renewable lease and 60s session stay unchanged.
                    max_duration_ms: 30000,
                },
                limits: template.limits,
                events: template.events,
            },
        };
        let result = service
            .camera_request(eefn::camera_submission::CameraRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                command: submission.clone(),
            })
            .await
            .unwrap();
        let confirmed = eefn::camera_submission::confirmed_node(&result, "origin", &submission);
        (result, confirmed)
    } else {
        let submission = eefn::microphone_submission::MicrophoneCommand {
            operation_id: operation_id.clone(),
            input: eefn::microphone_submission::MicrophoneInput {
                schema_version: 1,
                original_text: "Observe microphone activity for an explicitly bounded session"
                    .into(),
                title: "Bounded microphone source".into(),
                source_node: None,
                microphone: None,
                priority: 0,
                parameters: EnergyParameters {
                    sample_rate: 16000,
                    channels: 1,
                    block_frames: 80,
                    max_blocks: 5,
                    activation_level: 1000,
                    deactivation_level: 500,
                },
                limits: template.limits,
                events: template.events,
            },
        };
        let result = service
            .microphone_request(eefn::microphone_submission::MicrophoneRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                command: submission.clone(),
            })
            .await
            .unwrap();
        let confirmed = eefn::microphone_submission::confirmed_node(
            &result,
            "origin",
            &submission
        );
        (result, confirmed)
    };
    let observation = if result["success"] != true || !confirmed {
        // Failure-only, read-only inspection. Never resubmit, compile, approve,
        // infer or inspect a device to improve a failed authoring outcome.
        let plans = runtime.engine.store.list();
        let store_readable = plans.is_ok();
        let retained: Vec<_> = plans.unwrap_or_default().into_iter().filter(|p| {
            p.goal.request_context.as_ref().is_some_and(|c| {
                c.origin_node() == "origin" && c.request_id() == operation_id.as_str()
            })
        }).take(2).map(|p| json!({
            "revision":p.revision,"status":p.status,"review_present":p.request_review.is_some(),
            "compiled":p.planning.is_some(),"execution_present":p.execution.is_some()
        })).collect();
        let code = result["error_code"].as_str().filter(|code| matches!(*code,
            "microphone_authoring_unconfirmed" | "microphone_authoring_not_sent" |
            "camera_authoring_unconfirmed" | "camera_authoring_not_sent"
        )).unwrap_or("other_or_missing");
        json!({"source":if kind.camera(){"camera"}else{"microphone"},"phase":phase,
            "elapsed_ms":started.elapsed().as_millis(),"node_error_code":code,
            "coordinator_acknowledged":result["coordinator_acknowledged"]==true,
            "outcome_unknown":result["outcome_unknown"]==true,"report_confirmed":confirmed,
            "input_channel":service.submissions.state(),"store_readable":store_readable,
            "source_snapshot_present":runtime.node_server.node_state("origin").await.is_some(),
            "source_session_present":runtime.node_server.grant_session("origin").await.is_ok(),
            "retained_checkpoints":retained})
    } else {
        serde_json::Value::Null
    };
    assert!(confirmed, "Origin submission report was unconfirmed: {observation}");
    assert_eq!(
        result["success"], true,
        "Origin submission failed: {observation}"
    );
    let id = result["result"]["result"]["plan"]["id"].as_str().unwrap();
    let planned = runtime.engine.store.get(id).unwrap();
    assert!(planned.approval.is_none() && planned.execution.is_none());
    let approval = ApprovalCommand {
        id: planned.plan_id.clone(),
        expected_revision: planned.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        plan_sha256: planned.planning.as_ref().unwrap().plan_sha256.clone(),
        valid_for_ms: 120000,
    };
    let result = full_source_post(
        client,
        base,
        "/api/commands/requests/approve-session",
        json!({"schema_version":2,"expected_runtime_id":runtime.runtime_id(),"command":approval}),
    )
    .await;
    assert!(crate::workload_approval::confirmed(
        &result,
        runtime.runtime_id(),
        &approval
    ));
    serde_json::from_value(result["plan"].clone()).unwrap()
}
async fn full_source_wait_plan(
    runtime: &crate::Runtime,
    id: &str,
    condition: impl Fn(&Plan) -> bool,
) -> Plan {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let plan = runtime.engine.store.get(id).unwrap();
            if condition(&plan) {
                break plan;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

async fn full_source_finish(
    runtime: &crate::Runtime,
    plan: &Plan,
    trace: &std::path::Path,
    kind: FullSourceKind,
) -> Plan {
    full_source_wait_plan(runtime, &plan.plan_id, |p| {
        p.execution
            .as_ref()
            .and_then(|e| e.workload.as_ref())
            .is_some_and(|w| w.acknowledgement.is_some())
    })
    .await;
    // Keep the real source waiting while the original lease renewer runs.
    // Completion below must report sequence >=2; this adds no source deadline.
    tokio::time::sleep(Duration::from_millis(3600)).await;
    tokio::fs::write(
        trace.join("observed"),
        b"coordinator consumed ACK and active renewal interval",
    )
    .await
    .unwrap();
    let mut last = 0u64;
    let done = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let current = runtime.engine.store.get(&plan.plan_id).unwrap();
            if let Some(w) = current.execution.as_ref().and_then(|e| e.workload.as_ref()) {
                for event in &w.observations {
                    if event.sequence > last {
                        tokio::fs::write(
                            trace.join(format!("ack-{}", event.source_sequence)),
                            b"saved in coordinator job",
                        )
                        .await
                        .unwrap();
                        last = event.sequence;
                    }
                }
            }
            if current.status == PlanStatus::Completed {
                break current;
            }
            assert_ne!(
                current.status,
                PlanStatus::Interrupted,
                "Source became uncertain: {}",
                json!(current.execution)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let w = done.execution.as_ref().unwrap().workload.as_ref().unwrap();
    let completed = w.completion.as_ref().unwrap();
    assert_eq!(w.observations.len(), kind.event_count());
    assert_eq!(last, kind.event_count() as u64);
    let summary = eefn::workload_network::summarize_events(&w.definition, &w.observations).unwrap();
    assert_eq!(completed.report.event_count, summary.count);
    assert_eq!(completed.report.event_bytes, summary.bytes);
    assert_eq!(
        completed.report.event_payload_prefix_sha256.as_deref(),
        Some(summary.prefix_sha256.as_str())
    );
    assert!(
        completed.report.lease_sequence >= 2
            && completed.report.owned_process_exit_observed
            && !completed.report.physical_effects_stopped
    );
    let actual = runtime
        .node_server
        .inspect_workload_receipt(&w.definition)
        .await
        .unwrap();
    assert_eq!(actual.receipt.state, "completed");
    assert_eq!(json!(actual.completion), json!(Some(&completed.report)));
    done
}

#[tokio::test]
async fn real_workload_commands_origin_tls_source_completion_stop_and_reconnect_share_durable_evidence()
 {
    full_source_cases(FullSourceKind::Microphone).await;
}

#[tokio::test]
async fn real_camera_commands_origin_tls_completion_stop_and_reconnect_share_durable_evidence() {
    full_source_cases(FullSourceKind::Camera).await;
}

async fn full_source_cases(kind: FullSourceKind) {
    let fixture = tempfile::tempdir().unwrap();
    let python = full_source_venv(fixture.path()).await;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for mode in ["complete", "stop", "reconnect"] {
        let dir = tempfile::tempdir().unwrap();
        let trace = dir.path().join("trace");
        std::fs::create_dir(&trace).unwrap();
        full_source_dependency(
            &python,
            &trace,
            if mode == "complete" {
                "finite"
            } else {
                "blocked"
            },
            kind,
        );
        let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
        let credential = eefn::authentication::generate_credential();
        let owner_path = dir.path().join("owner.json");
        std::fs::write(&owner_path,serde_json::to_vec(&json!({
            "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,"tls":identity,"credentials":{"origin":credential}}},
            "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
        })).unwrap()).unwrap();
        let runtime = crate::Runtime::initialize(
            crate::config::Config::load(Some(&owner_path)).unwrap(),
            dir.path().join("owner.db"),
            false,
        )
        .await
        .unwrap();
        let metadata:eefn::context::NodeMetadata=serde_json::from_value(if kind.camera() {
            json!({"resources":[{"id":"camera","capability":"camera.capture","parameters":{"device":"rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live"}}]})
        } else {
            json!({"resources":[{"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_DEVICE"}}]})
        }).unwrap();
        let node_path = dir.path().join("node.json");
        std::fs::write(&node_path,serde_json::to_vec(&json!({"node_id":"origin","metadata":metadata,"python":python,
            "permissions":{"media":{"microphone":!kind.camera(),"camera":kind.camera()}},"listening":{"enabled":true},"workload_policy":{"enabled":true}})).unwrap()).unwrap();
        let service = eefn::NodeService::new(node_path, "origin".into());
        let mut policy = eefn::NodePolicy::default();
        policy.media.microphone = !kind.camera();
        policy.media.camera = kind.camera();
        let engine = Arc::new(
            eefn::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_resources("origin".into(), metadata)
                .unwrap()
                .with_service(service.clone())
                .with_python_runtime(Arc::new(
                    eefn::python::PythonRuntime::discover(Some(python.clone())).unwrap(),
                )),
        );
        let generation = uuid::Uuid::new_v4().to_string();
        let _guard = service.attach_preview_runtime(&engine, generation.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(generation);
        let config:eefn::NodeClientConfig=serde_json::from_value(json!({"node_id":"origin","auth_mode":"node_credential","psk":credential,
            "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],"ollama_url":"http://127.0.0.1:9"})).unwrap();
        let node = Arc::new(
            eefn::NodeClient::new(config, engine.clone())
                .unwrap()
                .with_status(service.live.clone())
                .with_submissions(service.submissions.clone())
                .with_registration_runtime(&generation)
                .unwrap()
                .with_grants(&dir.path().join("node-grants.db"), Default::default()),
        );
        let running = node.clone();
        let mut connection = tokio::spawn(async move { running.connect_once().await });
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let state = runtime.node_server.node_state("origin").await;
                if state.is_some_and(|s| s.registration["workload_execution"]["available"] == true)
                    && service.submissions.state() == eefn::submission::InputChannelState::Ready
                {
                    break;
                }
                assert!(!connection.is_finished(), "Node ended before readiness");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let session = runtime.node_server.grant_session("origin").await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = super::router_with_ui(runtime.clone(), false)
            .into_make_service_with_connect_info::<std::net::SocketAddr>();
        let api = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(
            !engine.capabilities().contains(&kind.capability().into()),
            "Built-in source admission must not depend on the one-shot plugin"
        );
        let plan = full_source_plan(&runtime, &service, &http, &base, kind, mode).await;
        let command = execute(&plan);
        assert!(
            !trace.join("pids.json").exists(),
            "Review/approval/readiness cannot open a source"
        );
        let started = service
            .execute_command(eefn::request_execution::ExecuteRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                command: command.clone(),
            })
            .await
            .unwrap();
        assert_eq!(
            started["success"], true,
            "Running-origin execution not confirmed: {started}"
        );
        assert!(eefn::request_execution::confirmed(
            &started["result"],
            &command,
            Some("origin"),
            Some(runtime.runtime_id())
        ));
        let done = if mode == "complete" {
            full_source_finish(&runtime, &plan, &trace, kind).await
        } else {
            full_source_wait_plan(&runtime, &plan.plan_id, |p| {
                p.execution
                    .as_ref()
                    .and_then(|e| e.workload.as_ref())
                    .is_some_and(|w| w.acknowledgement.is_some())
            })
            .await;
            tokio::fs::write(
                trace.join("observed"),
                b"accepted source before explicit stop or link loss",
            )
            .await
            .unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if std::fs::read_to_string(trace.join("trace.jsonl"))
                        .unwrap_or_default()
                        .contains("blocked")
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            if mode == "stop" {
                full_source_post(
                    &http,
                    &base,
                    &format!("/api/jobs/{}/stop", plan.plan_id),
                    json!({}),
                )
                .await;
            } else {
                connection.abort();
                let _ = (&mut connection).await;
            }
            let stopped = full_source_wait_plan(&runtime, &plan.plan_id, |p| {
                p.status == PlanStatus::Interrupted
                    && (mode != "stop"
                        || p.execution
                            .as_ref()
                            .unwrap()
                            .workload
                            .as_ref()
                            .unwrap()
                            .stop
                            .as_ref()
                            .is_some_and(|s| s.report.is_some()))
            })
            .await;
            let w = stopped
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap();
            assert_eq!(w.state, AttemptState::Unknown);
            assert!(w.completion.is_none());
            if mode == "stop" {
                let report = w.stop.as_ref().unwrap().report.as_ref().unwrap();
                assert!(report.confirms_scope(&w.definition.scope().unwrap()));
                assert!(!report.execution_stopped && !report.external_effects_stopped);
            }
            if mode == "reconnect" {
                let running = node.clone();
                connection = tokio::spawn(async move { running.connect_once().await });
                tokio::time::timeout(Duration::from_secs(15), async {
                    loop {
                        if let Ok(current) = runtime.node_server.grant_session("origin").await {
                            if current != session
                                && service.submissions.state()
                                    == eefn::submission::InputChannelState::Ready
                                && runtime
                                    .node_server
                                    .node_state("origin")
                                    .await
                                    .is_some_and(|s| {
                                        s.registration["workload_execution"]["available"] == true
                                    })
                            {
                                break;
                            }
                        }
                        assert!(!connection.is_finished());
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
            }
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if runtime
                        .node_server
                        .inspect_workload_receipt(&w.definition)
                        .await
                        .is_ok_and(|r| r.receipt.state == "unknown")
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            // Existing owned-capture cleanup observes for at most three seconds.
            // Wait independently before a NEW explicit job proves source reuse.
            tokio::time::sleep(Duration::from_secs(5)).await;
            stopped
        };
        let retained = json!(done);
        let retry=full_source_post(&http,&base,"/api/commands/requests/execute",json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":command})).await;
        assert_eq!(retry["reused"], true);
        assert_eq!(
            json!(runtime.engine.store.get(&plan.plan_id).unwrap()),
            retained
        );
        if mode != "complete" {
            let next_trace = dir.path().join("next-trace");
            std::fs::create_dir(&next_trace).unwrap();
            full_source_dependency(&python, &next_trace, "finite", kind);
            let phase = if mode == "stop" { "stop_reuse" } else { "reconnect_reuse" };
            let next = full_source_plan(&runtime, &service, &http, &base, kind, phase).await;
            let next_command = execute(&next);
            full_source_post(&http,&base,"/api/commands/requests/execute",json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":next_command})).await;
            let completed = full_source_finish(&runtime, &next, &next_trace, kind).await;
            assert_ne!(
                completed
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .definition
                    .grant_id,
                done.execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .definition
                    .grant_id
            );
            assert_eq!(
                json!(runtime.engine.store.get(&plan.plan_id).unwrap()),
                retained,
                "A new explicit job cannot rewrite or replay uncertainty"
            );
        }
        connection.abort();
        let _ = connection.await;
        api.abort();
        let _ = api.await;
        runtime.shutdown().await;
    }
}
