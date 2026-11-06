// Opt-in Windows acceptance: real owner/origin commands, coordinator driver,
// credential/TLS NodeClient and shipping source adapter. Only the microphone
// dependency lives in a disposable venv; no production test hook or device opens.
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
    assert!(
        tokio::time::timeout(Duration::from_secs(30), command.status())
            .await
            .unwrap()
            .unwrap()
            .success(),
        "Disposable stdlib venv setup failed"
    );
    let executable = target.join("Scripts/python.exe");
    assert!(executable.is_file());
    executable
}
fn full_source_dependency(venv_python: &std::path::Path, trace: &std::path::Path, mode: &str) {
    let site = venv_python
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Lib/site-packages");
    assert!(site.is_dir());
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
async fn full_source_plan(runtime: &crate::Runtime, client: &reqwest::Client, base: &str) -> Plan {
    let source = runtime
        .node_server
        .workload_sources("origin", 5)
        .await
        .unwrap();
    assert!(source.execution_available);
    let mut input = template_input();
    let mut template = Template::from_params(&input.params).unwrap();
    template.executor_node = "origin".into();
    template.source = Source::MicrophoneEnergy {
        resource_id: "origin::mic".into(),
        resource_sha256: source.resources[0].resource_sha256.clone().unwrap(),
        adapter_sha256: source.adapter_sha256,
        parameters: EnergyParameters {
            sample_rate: 16000,
            channels: 1,
            block_frames: 80,
            max_blocks: 5,
            activation_level: 1000,
            deactivation_level: 500,
        },
    };
    template.limits.max_source_frames = 400;
    input.params = json!({"workload":template});
    let (review, input) = review_input(&runtime.engine, input);
    let compile = PlanningCommand::Compile {
        id: review.plan_id.clone(),
        expected_revision: review.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    let result = full_source_post(
        client,
        base,
        "/api/commands/requests/planning",
        json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":compile}),
    )
    .await;
    assert!(crate::request_planning::confirmed_report(
        &result,
        runtime.runtime_id(),
        &compile
    ));
    let planned: Plan = serde_json::from_value(result["plan"].clone()).unwrap();
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
    assert_eq!(w.observations.len(), 4);
    assert_eq!(last, 4);
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
        let metadata:eefn::context::NodeMetadata=serde_json::from_value(json!({"resources":[{"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_DEVICE"}}]})).unwrap();
        let node_path = dir.path().join("node.json");
        std::fs::write(&node_path,serde_json::to_vec(&json!({"node_id":"origin","metadata":metadata,"python":python,
            "permissions":{"media":{"microphone":true}},"listening":{"enabled":true},"workload_policy":{"enabled":true}})).unwrap()).unwrap();
        let service = eefn::NodeService::new(node_path, "origin".into());
        let mut policy = eefn::NodePolicy::default();
        policy.media.microphone = true;
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
        let plan = full_source_plan(&runtime, &http, &base).await;
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
            full_source_finish(&runtime, &plan, &trace).await
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
            // Wait independently before a NEW explicit job proves microphone reuse.
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
            full_source_dependency(&python, &next_trace, "finite");
            let next = full_source_plan(&runtime, &http, &base).await;
            let next_command = execute(&next);
            full_source_post(&http,&base,"/api/commands/requests/execute",json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":next_command})).await;
            let completed = full_source_finish(&runtime, &next, &next_trace).await;
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
