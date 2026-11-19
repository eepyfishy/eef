// Real owner HTTP and credential/pinned-TLS framing; the peer reports synthetic
// receipt/event metadata. This is not a source, physical microphone or node test.
#[tokio::test]
async fn owner_commands_drive_workload_frames_completion_stop_and_disconnect_without_replay() {
    use eefn::{
        protocol::*,
        workload_leases::{Query, definition::CAPABILITY},
        workload_network::*,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    for mode in ["complete", "stop", "lost_stop_reply", "disconnect"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("owner.json");
        let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
        let key = eefn::authentication::generate_credential();
        std::fs::write(&config, serde_json::to_vec(&json!({
            "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,
                "credentials":{"executor":key},"tls":identity}},
            "update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}
        })).unwrap()).unwrap();
        let runtime = crate::Runtime::initialize(
            crate::config::Config::load(Some(&config)).unwrap(),
            dir.path().join("jobs.db"),
            false,
        )
        .await
        .unwrap();
        let (planned, _) = planned(&runtime.engine);
        let approved = approve(
            &runtime.engine,
            &ApprovalCommand {
                id: planned.plan_id.clone(),
                expected_revision: planned.revision,
                operation_id: uuid::Uuid::new_v4().to_string(),
                plan_sha256: planned.planning.as_ref().unwrap().plan_sha256.clone(),
                valid_for_ms: 120000,
            },
            &identity.fingerprint().unwrap(),
        )
        .unwrap();
        let command = execute(&approved);
        let template =
            eefn::workload_leases::definition::Template::from_params(&approved.goal.params)
                .unwrap();
        let node_runtime = uuid::Uuid::new_v4().to_string();
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", runtime.node_server.port()))
            .await
            .unwrap();
        eefn::authentication::write_selector(&mut socket, "executor")
            .await
            .unwrap();
        let secured = eefn::tls::connect(
            socket,
            &identity.certificate,
            eefn::tls::client_config(&identity.certificate).unwrap(),
        )
        .await
        .unwrap();
        let (input, mut output) = tokio::io::split(secured);
        let mut input = tokio::io::BufReader::new(input);
        let crypto = eefn::NodeCrypto::new(&key).unwrap();
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
            "Synthetic workload peer",
            eefn::VERSION,
            &[CAPABILITY.into()],
            &json!({}),
            &[],
        );
        registration["registration_receipt"] =
            json!(eefn::registration::Offer::new(&node_runtime).unwrap());
        registration["execution_grants"] = json!(eefn::grants::GrantAdvertisement {
            schema_version: 1,
            available: true,
            limits: Some(Default::default()),
            deadline_guarantee: "rpc_future_only".into(),
            timing_mode: Some(eefn::grant_time::MODE.into()),
            retained_results: true
        });
        registration["grant_stop"] = json!(eefn::grants::stop::Advertisement::new(true));
        registration["grant_stop_read"] =
            json!(eefn::grants::stop::inspection::Advertisement::new(true));
        if mode == "lost_stop_reply" {
            registration["workload_stop_exit_read"] =
                json!({"schema_version":1,"available":true,"read_only":true});
        }
        registration["workload_execution"] = json!(Advertisement {
            schema_version: 2,
            available: true,
            runtime_id: Some(node_runtime.clone()),
            policy: Some(policy()),
            source_kind: "microphone_energy".into(),
            event_delivery: "bounded_metadata".into(),
            restart_authorized: false,
            completion_payload_prefix: true,
            source_kinds: Vec::new()
        });
        write_message(&crypto, &mut output, &registration)
            .await
            .unwrap();
        let accepted = read_message(&crypto, &mut input).await.unwrap().unwrap();
        assert_eq!(accepted["type"], "registration_accepted");
        let source = eefn::workload_sources::Report {
            schema_version: 3,
            report_type: "workload_sources".into(),
            success: true,
            node_id: "executor".into(),
            runtime_id: node_runtime,
            source_kind: "microphone_energy".into(),
            adapter_sha256: "e".repeat(64),
            resources: vec![eefn::workload_sources::Resource {
                resource_id: template.resource_id().into(),
                name: "Synthetic source metadata".into(),
                area: vec!["home".into()],
                available: true,
                resource_sha256: Some("d".repeat(64)),
            }],
            local_bindings_current: true,
            listening_enabled: true,
            listening_revision: 1,
            listening_epoch: 0,
            listening_blocked: false,
            saved_microphone_permission: true,
            applied_microphone_permission: true,
            python_configured: true,
            capture_started: false,
            execution_authorized: false,
            execution_available: true,
        };
        let offered = Arc::new(AtomicUsize::new(0));
        let offers = offered.clone();
        let stopped = Arc::new(AtomicUsize::new(0));
        let stops = stopped.clone();
        let inspecting = runtime.clone();
        let peer = tokio::spawn(async move {
            let mut definition = None;
            while let Some(message) = read_message(&crypto, &mut input).await.unwrap() {
                match message["type"].as_str() {
                    Some("request") => {
                        assert_eq!(message["capability"], "node.configure");
                        assert_eq!(message["action"], "workload_sources");
                        write_message(
                            &crypto,
                            &mut output,
                            &success_response(message["id"].as_str().unwrap(), json!(source)),
                        )
                        .await
                        .unwrap();
                    }
                    Some("execute_workload") => {
                        assert_eq!(offers.fetch_add(1, Ordering::SeqCst), 0, "No second offer");
                        let offer: Offer = serde_json::from_value(message).unwrap();
                        let d = offer.definition;
                        assert_eq!(
                            inspecting
                                .engine
                                .store
                                .get(&d.job_id)
                                .unwrap()
                                .execution
                                .as_ref()
                                .unwrap()
                                .workload
                                .as_ref()
                                .unwrap()
                                .state,
                            crate::request_execution::AttemptState::Issued,
                            "Durable issuance precedes offer"
                        );
                        for sequence in 1..=2 {
                            let query = Query {
                                kind: "workload_lease_probe".into(),
                                schema_version: 2,
                                id: uuid::Uuid::new_v4().to_string(),
                                contract: d.lease_contract().unwrap(),
                                sequence,
                            };
                            write_message(&crypto,&mut output,&json!({"type":"workload_probe","schema_version":2,"id":d.grant_id,"query":query})).await.unwrap();
                            let lease = read_message(&crypto, &mut input).await.unwrap().unwrap();
                            assert_eq!(lease["type"], "workload_lease");
                            assert_eq!(lease["id"], query.id);
                            assert_eq!(lease["sequence"], sequence);
                            if sequence == 1 {
                                write_message(&crypto,&mut output,&json!({"type":"workload_ack","schema_version":2,"id":d.grant_id,"receipt":synthetic_ack(&d)})).await.unwrap();
                            } else {
                                assert_eq!(
                                    inspecting
                                        .engine
                                        .store
                                        .get(&d.job_id)
                                        .unwrap()
                                        .execution
                                        .as_ref()
                                        .unwrap()
                                        .workload
                                        .as_ref()
                                        .unwrap()
                                        .state,
                                    crate::request_execution::AttemptState::Accepted,
                                    "Durable ACK precedes renewal two"
                                );
                            }
                        }
                        let event = Event {
                            schema_version: 2,
                            kind: EventKind::MicrophoneActivity,
                            sequence: 1,
                            source_sequence: 1,
                            source_frames: 80,
                            elapsed_ms: 0,
                            active: Some(true),
                            level: Some(1000),
                            discontinuity: false,
                        };
                        write_message(&crypto,&mut output,&json!({"type":"workload_event","schema_version":2,"id":d.grant_id,"event":event})).await.unwrap();
                        if mode == "complete" {
                            let summary = summarize_events(&d, &[event]).unwrap();
                            let mut receipt = synthetic_ack(&d);
                            receipt.schema_version = 3;
                            receipt.receipt.state = "completed".into();
                            receipt.receipt.updated_at_ms += 1;
                            receipt.completion = Some(eefn::grants::workload::CompletionReport {
                                schema_version: 2,
                                grant_sha256: d.scope().unwrap().grant_sha256,
                                adapter_sha256: "e".repeat(64),
                                source_observations: 100,
                                source_frames: 8000,
                                event_count: summary.count,
                                event_bytes: summary.bytes,
                                event_prefix_sha256: "f".repeat(64),
                                event_payload_prefix_sha256: Some(summary.prefix_sha256),
                                analysis:None,
                                lease_sequence: 2,
                                recorded_at_ms: receipt.receipt.updated_at_ms,
                                owned_process_exit_observed: true,
                                physical_effects_stopped: false,
                            });
                            write_message(&crypto,&mut output,&json!({"type":"workload_complete","schema_version":2,"id":d.grant_id,"receipt":receipt})).await.unwrap();
                        } else if mode == "disconnect" {
                            return;
                        }
                        definition = Some(d);
                    }
                    Some("stop_grant") => {
                        assert_eq!(
                            stops.fetch_add(1, Ordering::SeqCst),
                            0,
                            "No automatic stop retry"
                        );
                        let d = definition.as_ref().unwrap();
                        let scope = d.scope().unwrap();
                        assert_eq!(message["scope"], json!(scope));
                        if mode == "lost_stop_reply" {
                            continue;
                        }
                        let report = eefn::grants::stop::Report {
                            schema_version: 1,
                            report_type: "grant_stop_receipt".into(),
                            grant_id: scope.grant_id,
                            grant_sha256: scope.grant_sha256,
                            issuer_id: scope.issuer_id,
                            session_id: scope.session_id,
                            executor_node: scope.executor_node,
                            job_id: scope.job_id,
                            task_id: scope.task_id,
                            attempt: scope.attempt,
                            recorded_at_ms: now_ms().max(1),
                            receipt_preexisted: true,
                            reused: false,
                            cancellation_signalled: true,
                            persisted: true,
                            stop_requested: true,
                            execution_stopped: false,
                            external_effects_stopped: false,
                            execution_authorized: false,
                            automatically_retried: false,
                        };
                        write_message(
                            &crypto,
                            &mut output,
                            &success_response(message["id"].as_str().unwrap(), json!(report)),
                        )
                        .await
                        .unwrap();
                    }
                    Some("inspect_grant_stop") => {
                        use eefn::grants::stop::inspection;
                        assert_eq!(mode, "lost_stop_reply");
                        assert_eq!(stops.load(Ordering::SeqCst), 1);
                        let scope = definition.as_ref().unwrap().scope().unwrap();
                        assert_eq!(message["scope"], json!(scope));
                        let result = inspection::Inspection {
                            schema_version: 1,
                            report_type: "grant_stop_inspection".into(),
                            node_id: scope.executor_node.clone(),
                            grant_id: scope.grant_id.clone(),
                            scope: Some(scope.clone()),
                            availability: inspection::Availability::Retained,
                            evidence: Some(inspection::Evidence {
                                schema_version: 1,
                                scope,
                                recorded_at_ms: now_ms().max(1),
                                receipt_preexisted: true,
                                persisted: true,
                                stop_requested: true,
                                execution_stopped: false,
                                external_effects_stopped: false,
                            }),
                            read_only: true,
                            execution_requested: false,
                            stop_requested: false,
                            automatically_retried: false,
                            reconciled: false,
                            note: inspection::NOTE.into(),
                        };
                        write_message(
                            &crypto,
                            &mut output,
                            &success_response(message["id"].as_str().unwrap(), json!(result)),
                        )
                        .await
                        .unwrap();
                    }
                    Some("workload_stop_exit") => {
                        assert_eq!(mode, "lost_stop_reply");
                        assert_eq!(stops.load(Ordering::SeqCst), 1);
                        let scope = definition.as_ref().unwrap().scope().unwrap();
                        assert_eq!(message["scope"], json!(scope));
                        assert!(message.get("expected_issuer_id").is_none());
                        assert!(message.get("expected_node_id").is_none());
                        let at = now_ms().max(1);
                        let result = json!({
                            "schema_version":1,"report_type":"workload_stop_exit",
                            "scope":scope,"stop_recorded_at_ms":at,"read_only":true,
                            "observation":{"schema_version":1,"kind":"owned_process_tree_empty_after_stop_request",
                                "mode":"windows_owned_job","observed_at_ms":at,"stop_requested":true,
                                "owned_process_tree_empty":true,"external_effects_stopped":false,"finished_actions_undone":false}
                        });
                        write_message(
                            &crypto,
                            &mut output,
                            &success_response(message["id"].as_str().unwrap(), result),
                        )
                        .await
                        .unwrap();
                    }
                    _ => panic!("Unexpected peer frame type"),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = super::router_with_ui(runtime.clone(), false)
            .into_make_service_with_connect_info::<std::net::SocketAddr>();
        let http = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let body = json!({"schema_version":1,"expected_runtime_id":runtime.runtime_id(),"command":command});
        let response = client
            .post(format!("http://{address}/api/commands/requests/execute"))
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "Owner execution refused: {}",
            response.text().await.unwrap()
        );
        let report: Value = response.json().await.unwrap();
        assert_eq!(report["execution_requested"], true);
        assert_eq!(report["reused"], false);
        if matches!(mode, "stop" | "lost_stop_reply") {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let p = runtime.engine.store.get(&approved.plan_id).unwrap();
                    if p.execution
                        .as_ref()
                        .unwrap()
                        .workload
                        .as_ref()
                        .unwrap()
                        .observations
                        .len()
                        == 1
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(
                client
                    .post(format!(
                        "http://{address}/api/jobs/{}/stop",
                        approved.plan_id
                    ))
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                    .unwrap()
                    .status()
                    .is_success()
            );
        }
        let final_plan = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let p = runtime.engine.store.get(&approved.plan_id).unwrap();
                let e = p.execution.as_ref().unwrap();
                let w = e.workload.as_ref().unwrap();
                if mode == "complete" && p.status == crate::task::PlanStatus::Completed
                    || mode == "disconnect" && p.status == crate::task::PlanStatus::Interrupted
                    || mode == "stop"
                        && p.status == crate::task::PlanStatus::Interrupted
                        && w.stop.as_ref().is_some_and(|s| s.report.is_some())
                    || mode == "lost_stop_reply"
                        && p.status == crate::task::PlanStatus::Interrupted
                        && w.stop.as_ref().is_some_and(|s| s.outcome_unknown)
                        && stopped.load(Ordering::SeqCst) == 1
                {
                    break p;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        if mode == "complete" {
            assert_eq!(
                final_plan
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .observations
                    .len(),
                1
            );
        }
        if mode == "stop" {
            assert_eq!(stopped.load(Ordering::SeqCst), 1);
            assert!(
                !final_plan
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .stop
                    .as_ref()
                    .unwrap()
                    .report
                    .as_ref()
                    .unwrap()
                    .execution_stopped
            );
        }
        let before = json!(final_plan);
        if mode == "lost_stop_reply" {
            let workload = final_plan
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap();
            let command = eefn::request_stop_receipt::Command {
                id: approved.plan_id.clone(),
                grant_id: workload.definition.grant_id.clone(),
            };
            assert!(
                runtime
                    .inspect_request_stop_receipt(command.clone(), Some("other-origin"))
                    .await
                    .is_err()
            );
            let result: Value = client
                .post(format!("http://{address}/api/commands/requests/stop-receipt"))
                .json(&json!({
                    "schema_version": 1,
                    "expected_runtime_id": runtime.runtime_id(),
                    "command": command,
                }))
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(eefn::request_stop_receipt::confirmed(
                &result,
                &command,
                None,
                Some(runtime.runtime_id())
            ));
            assert_eq!(result["coordinator_attempt_state"], "unknown");
            assert_eq!(result["inspection"]["availability"], "retained");
            assert_eq!(
                result["inspection"]["evidence"]["scope"],
                json!(workload.definition.scope().unwrap())
            );
            assert_eq!(result["inspection"]["evidence"]["execution_stopped"], false);
            assert_eq!(stopped.load(Ordering::SeqCst), 1);
        }
        if matches!(mode, "stop" | "lost_stop_reply") {
            let context = final_plan.goal.request_context.as_ref().unwrap();
            let query = eefn::process_stop_status::Command {
                id: approved.plan_id.clone(),
            };
            assert!(runtime
                .inspect_process_stop_status(query.clone(), Some("other-origin"))
                .await
                .is_err());
            let status = runtime
                .inspect_process_stop_status(query, Some(context.origin_node()))
                .await
                .unwrap();
            assert_eq!(status["stop_requested"], true);
            assert_eq!(status["issued_attempts"], 1);
            assert_eq!(status["external_effects_stopped"], false);
            assert_eq!(status["revision"], final_plan.revision);
            if mode == "stop" {
                assert_eq!(status["not_inspected_issued_attempts"], 1);
                assert_eq!(status["grants"], json!([]));
            } else {
                assert_eq!(status["not_inspected_issued_attempts"], 0);
                assert_eq!(status["grants"][0]["availability"], "owned_process_tree_empty");
                assert_eq!(status["grants"][0]["intent_recorded"], true);
                assert_eq!(stopped.load(Ordering::SeqCst), 1);
            }
        }
        let reused: Value = client
            .post(format!("http://{address}/api/commands/requests/execute"))
            .json(&body)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(reused["reused"], true);
        assert_eq!(offered.load(Ordering::SeqCst), 1);
        assert_eq!(
            json!(runtime.engine.store.get(&approved.plan_id).unwrap()),
            before
        );
        http.abort();
        let _ = http.await;
        peer.abort();
        let _ = peer.await;
        runtime.shutdown().await;
    }
}
