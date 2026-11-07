// Actual credential/TLS workload route with the pinned camera source and fake device API.
fn camera_network_policy(service: &crate::NodeService, runtime: &str, enabled: bool) {
    let request = crate::workload_policy::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: runtime.into(),
        command: crate::workload_policy::Command::Show {},
    };
    let current = service
        .workload_policy_command(request.clone(), false)
        .unwrap();
    service
        .workload_policy_command(
            crate::workload_policy::Request {
                command: crate::workload_policy::Command::Set {
                    policy: crate::workload_leases::Policy {
                        enabled,
                        ..Default::default()
                    },
                    expected_revision: current["revision"].as_u64().unwrap(),
                    expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                },
                ..request
            },
            false,
        )
        .unwrap();
}
fn camera_network_enable(service: &crate::NodeService, runtime: &str) {
    let request = crate::listening::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: runtime.into(),
        command: crate::listening::Command::Show {},
    };
    let current = service.listening_command(request.clone(), false).unwrap();
    service
        .listening_command(
            crate::listening::Request {
                command: crate::listening::Command::Set {
                    enabled: true,
                    expected_revision: current["revision"].as_u64().unwrap(),
                    expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                    expected_camera_epoch: current
                        .pointer("/camera/runtime_epoch")
                        .and_then(Value::as_u64),
                },
                ..request
            },
            false,
        )
        .unwrap();
}

#[tokio::test]
async fn actual_camera_workload_negotiates_exact_source_persists_events_and_withdraws_without_reconnect()
 {
    for mode in ["finite", "blocked"] {
        let (mut f, trace) = crate::workload_runtime::camera_tests::native_fixture(mode);
        camera_network_policy(&f.service, &f.runtime, true);
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let credential = crate::authentication::generate_credential();
        let server = crate::NodeServer::with_authentication(
            "",
            "127.0.0.1",
            0,
            crate::authentication::AuthenticationPolicy {
                allow_legacy: false,
                tls: Some(identity.clone()),
                credentials: std::collections::BTreeMap::from([(
                    "node".into(),
                    Some(credential.clone()),
                )]),
            },
        )
        .unwrap();
        server.start().await.unwrap();
        let config:NodeClientConfig=serde_json::from_value(json!({"node_id":"node","auth_mode":"node_credential","psk":credential,
            "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",server.port())],"ollama_url":"http://127.0.0.1:9"})).unwrap();
        let client = Arc::new(
            NodeClient::new(config, f.engine.clone())
                .unwrap()
                .with_status(f.service.live.clone())
                .with_registration_runtime(&f.runtime)
                .unwrap()
                .with_grants(&f.dir.path().join("camera-tls.db"), Default::default()),
        );
        let running = client.clone();
        let connection = tokio::spawn(async move { running.connect_once().await });
        let session = timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(session) = server.workload_session(&f.definition.template(), 5).await {
                    break session;
                }
                assert!(
                    !connection.is_finished(),
                    "Camera node ended before source readiness"
                );
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let state = server.node_state("node").await.unwrap();
        assert_eq!(
            state.registration["workload_execution"]["schema_version"],
            3
        );
        assert_eq!(
            state.registration["workload_execution"]["source_kinds"],
            json!(["camera_motion"])
        );
        let inspected = server
            .workload_source(&f.definition.template(), 5)
            .await
            .unwrap();
        inspected
            .confirms_execution(&f.definition.template(), &session.runtime)
            .unwrap();
        assert!(!f.engine.applied_permissions().media.microphone);
        let mut microphone = crate::grants::workload::tests::definition();
        microphone.executor_node = "node".into();
        if let crate::workload_leases::definition::Source::MicrophoneEnergy {
            resource_id, ..
        } = &mut microphone.source
        {
            *resource_id = "node::mic".into();
        }
        let refused = server
            .workload_session(&microphone.template(), 5)
            .await
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("reviewed workload source kind")
        );
        f.definition.issuer_id = identity.fingerprint().unwrap();
        f.definition.session_id = session.session.clone();
        let d = f.definition.clone();
        let ledger = client.grant_ledger.as_ref().unwrap();
        assert!(ledger.workload_receipt(&d.issuer_id, &d.grant_id).is_err());
        assert!(!trace.path().join("pids.json").exists());
        let mut issuer = crate::workload_leases::IssuerWindow::new(
            d.lease_contract().unwrap(),
            &session.policy,
            Instant::now() + Duration::from_secs(50),
        )
        .unwrap();
        let mut link = server
            .open_workload(
                d.clone(),
                tokio::time::Instant::now() + Duration::from_secs(50),
            )
            .await
            .unwrap();
        let mut acknowledged = false;
        let mut released = false;
        let mut processes = None;
        let mut events = Vec::new();
        loop {
            let frame = timeout(Duration::from_secs(10), link.next())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(frame.peer().session(), d.session_id);
            assert_eq!(frame.peer().runtime(), f.runtime);
            match frame.message()["type"].as_str().unwrap() {
                "workload_probe" => {
                    let query: crate::workload_leases::Query =
                        serde_json::from_value(frame.message()["query"].clone()).unwrap();
                    if query.sequence == 1 {
                        assert!(!trace.path().join("pids.json").exists());
                    }
                    if query.sequence >= 2 {
                        assert!(acknowledged);
                    }
                    let reply = issuer
                        .reply(
                            &query,
                            frame.peer().issuer(),
                            frame.peer().session(),
                            frame.peer().node(),
                        )
                        .unwrap();
                    link.reply_lease(&query, reply).unwrap();
                    if query.sequence >= 2 && !released {
                        let owned =
                            crate::process_lifetime::tests::Processes::observe(trace.path()).await;
                        owned.alive();
                        assert!(f.service.camera_pool.reserve().is_err());
                        assert!(f.service.microphone_pool.reserve().is_ok());
                        processes = Some(owned);
                        tokio::fs::write(
                            trace.path().join("observed"),
                            b"pinned TLS and ACK observed",
                        )
                        .await
                        .unwrap();
                        released = true;
                        if mode == "blocked" {
                            timeout(Duration::from_secs(5), async {
                                loop {
                                    if std::fs::read_to_string(trace.path().join("trace.jsonl"))
                                        .is_ok_and(|raw| {
                                            raw.lines().any(|line| {
                                                serde_json::from_str::<Value>(line)
                                                    .is_ok_and(|row| row["kind"] == "blocked")
                                            })
                                        })
                                    {
                                        break;
                                    }
                                    sleep(Duration::from_millis(10)).await;
                                }
                            })
                            .await
                            .unwrap();
                            let stopped = server.stop_workload(&d).await.unwrap();
                            assert!(stopped.confirms_scope(&d.scope().unwrap()));
                            break;
                        }
                    }
                }
                "workload_ack" => {
                    let receipt: crate::grants::workload::Receipt =
                        serde_json::from_value(frame.message()["receipt"].clone()).unwrap();
                    receipt.validate().unwrap();
                    assert_eq!(receipt.definition, d);
                    issuer.acknowledge_consumed(&d.scope().unwrap()).unwrap();
                    acknowledged = true;
                }
                "workload_event" => {
                    let event: crate::workload_network::Event =
                        serde_json::from_value(frame.message()["event"].clone()).unwrap();
                    assert_eq!(event.kind, crate::workload_events::Kind::CameraMotion);
                    let history = ledger
                        .workload_events(&d.issuer_id, &d.grant_id, event.sequence - 1, 1)
                        .unwrap();
                    assert_eq!(history["durable_events"][0]["event"], json!(event));
                    tokio::fs::write(
                        trace.path().join(format!("ack-{}", event.source_sequence)),
                        b"durable camera network event",
                    )
                    .await
                    .unwrap();
                    events.push(event);
                }
                "workload_complete" => {
                    assert_eq!(mode, "finite");
                    let receipt: crate::grants::workload::Receipt =
                        serde_json::from_value(frame.message()["receipt"].clone()).unwrap();
                    receipt.validate().unwrap();
                    assert_eq!(receipt.definition, d);
                    assert_eq!(receipt.receipt.state, "completed");
                    let summary = crate::workload_network::summarize_events(&d, &events).unwrap();
                    let report = receipt.completion.unwrap();
                    assert_eq!(
                        (
                            report.source_observations,
                            report.source_frames,
                            report.event_count
                        ),
                        (5, 5, 5)
                    );
                    assert_eq!(report.event_count, summary.count);
                    assert_eq!(report.event_bytes, summary.bytes);
                    assert_eq!(
                        report.event_payload_prefix_sha256.as_deref(),
                        Some(summary.prefix_sha256.as_str())
                    );
                    assert!(
                        report.lease_sequence >= 2
                            && report.owned_process_exit_observed
                            && !report.physical_effects_stopped
                    );
                    break;
                }
                _ => panic!(
                    "Unexpected camera workload frame: {}",
                    frame.message()["type"]
                ),
            }
        }
        assert!(acknowledged && released);
        timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(next) = f.service.camera_pool.reserve() {
                    assert!(processes.as_ref().unwrap().exited_now());
                    drop(next);
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        timeout(Duration::from_secs(5), async {
            while !ledger.workload_reporting_ready() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let retained = ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap();
        assert_eq!(
            retained.receipt.state,
            if mode == "finite" {
                "completed"
            } else {
                "unknown"
            }
        );
        let mut config = f.service.read_config().unwrap();
        config["permissions"]["media"]["camera"] = json!(false);
        f.service.save_configuration(config).unwrap();
        timeout(Duration::from_secs(5),async {loop {if server.node_state("node").await.unwrap().registration["workload_execution"]["available"]==false{break;}sleep(Duration::from_millis(10)).await;}}).await.unwrap();
        assert!(server.workload_session(&d.template(), 5).await.is_err());
        let mut config = f.service.read_config().unwrap();
        config["permissions"]["media"]["camera"] = json!(true);
        f.service.save_configuration(config).unwrap();
        assert!(
            !f.service
                .camera_workload_sources(crate::camera_sources::Request {
                    schema_version: 2,
                    expected_node_id: "node".into(),
                    expected_runtime_id: f.runtime.clone()
                })
                .unwrap()["execution_available"]
                .as_bool()
                .unwrap()
        );
        camera_network_enable(&f.service, &f.runtime);
        let refreshed = timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(session) = server.workload_session(&d.template(), 5).await {
                    break session;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(refreshed.session, session.session);
        assert_eq!(refreshed.runtime, f.runtime);
        assert_eq!(
            json!(ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap()),
            json!(retained)
        );
        let trace_text = std::fs::read_to_string(trace.path().join("trace.jsonl")).unwrap();
        assert_eq!(
            trace_text
                .lines()
                .filter(|line| serde_json::from_str::<Value>(line).unwrap()["kind"] == "opened")
                .count(),
            1,
            "Readiness refresh cannot restart capture"
        );
        server.stop().await;
        assert!(
            timeout(Duration::from_secs(5), connection)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }
}
