// Opt-in Windows units: production dispatch/preparation/adapter/job/receipts.
// The encrypted duplex peer is explicit, not credential/TLS network acceptance.
use std::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context as TaskContext, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

fn network_source_policy(f: &crate::workload_runtime::tests::Fixture, enabled: bool) {
    let mut request = crate::workload_policy::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: f.runtime.clone(),
        command: crate::workload_policy::Command::Show {},
    };
    let shown = f
        .service
        .workload_policy_command(request.clone(), false)
        .unwrap();
    request.command = crate::workload_policy::Command::Set {
        policy: crate::workload_leases::Policy {
            enabled,
            ..Default::default()
        },
        expected_revision: shown["revision"].as_u64().unwrap(),
        expected_epoch: shown["runtime_epoch"].as_u64().unwrap(),
    };
    f.service.workload_policy_command(request, false).unwrap();
}

#[tokio::test]
async fn actual_credential_tls_node_source_completes_stops_and_refreshes_policy_without_reconnect()
{
    for mode in ["finite", "blocked"] {
        let (mut f, trace) = crate::workload_runtime::tests::native_fixture(mode);
        network_source_policy(&f, true);
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
                .with_grants(&f.dir.path().join("tls-delivery.db"), Default::default()),
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
                    "Real node connection ended before readiness"
                );
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(session.runtime, f.runtime);
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
        let mut marker = false;
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
                        assert!(acknowledged, "Renewal follows consumed ACK");
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
                    if query.sequence >= 2 && !marker {
                        let owned =
                            crate::process_lifetime::tests::Processes::observe(trace.path()).await;
                        owned.alive();
                        assert!(f.service.microphone_pool.reserve().is_err());
                        processes = Some(owned);
                        tokio::fs::write(
                            trace.path().join("observed"),
                            b"real TLS and consumed ACK observed",
                        )
                        .await
                        .unwrap();
                        marker = true;
                        if mode == "blocked" {
                            timeout(Duration::from_secs(5), async {
                                loop {
                                    if std::fs::read_to_string(trace.path().join("trace.jsonl"))
                                        .unwrap_or_default()
                                        .contains("blocked")
                                    {
                                        break;
                                    }
                                    sleep(Duration::from_millis(10)).await;
                                }
                            })
                            .await
                            .unwrap();
                            issuer.stop();
                            let stop = server.stop_workload(&d).await.unwrap();
                            assert!(stop.confirms_scope(&d.scope().unwrap()));
                            assert!(
                                stop.persisted
                                    && stop.cancellation_signalled
                                    && !stop.execution_stopped
                            );
                            loop {
                                let rejected = timeout(Duration::from_secs(5), link.next())
                                    .await
                                    .unwrap()
                                    .unwrap();
                                if rejected.message()["type"] == "workload_rejected" {
                                    break;
                                }
                                assert_eq!(
                                    rejected.message()["type"],
                                    "workload_probe",
                                    "No source event after blocked input"
                                );
                            }
                            break;
                        }
                    }
                }
                "workload_ack" => {
                    let receipt: crate::grants::workload::Receipt =
                        serde_json::from_value(frame.message()["receipt"].clone()).unwrap();
                    receipt.validate().unwrap();
                    assert_eq!(receipt.definition, d);
                    assert_eq!(
                        ledger
                            .workload_receipt(&d.issuer_id, &d.grant_id)
                            .unwrap()
                            .receipt
                            .state,
                        "accepted"
                    );
                    issuer.acknowledge_consumed(&d.scope().unwrap()).unwrap();
                    acknowledged = true;
                }
                "workload_event" => {
                    let event: crate::workload_network::Event =
                        serde_json::from_value(frame.message()["event"].clone()).unwrap();
                    let history = ledger
                        .workload_events(&d.issuer_id, &d.grant_id, 0, 100)
                        .unwrap();
                    assert!(
                        serde_json::to_string(&history)
                            .unwrap()
                            .contains("microphone_activity")
                    );
                    tokio::fs::write(
                        trace.path().join(format!("ack-{}", event.source_sequence)),
                        b"durable network observation",
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
                    assert_eq!(report.event_count, summary.count);
                    assert_eq!(report.event_bytes, summary.bytes);
                    assert_eq!(report.event_prefix_sha256, summary.prefix_sha256);
                    assert!(
                        report.lease_sequence >= 2
                            && report.owned_process_exit_observed
                            && !report.physical_effects_stopped
                    );
                    break;
                }
                _ => panic!("Unexpected actual node source frame"),
            }
        }
        assert!(acknowledged && marker);
        timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(reusable) = f.service.microphone_pool.reserve() {
                    assert!(processes.as_ref().unwrap().exited_now());
                    drop(reusable);
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
        network_source_policy(&f, false);
        timeout(Duration::from_secs(5), async {
            loop {
                let state = server.node_state("node").await.unwrap();
                if state.registration["workload_execution"]["available"] == false {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(server.workload_session(&d.template(), 5).await.is_err());
        network_source_policy(&f, true);
        let refreshed = timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(current) = server.workload_session(&d.template(), 5).await {
                    break current;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            refreshed.session, session.session,
            "Policy refresh keeps the authenticated connection"
        );
        assert_eq!(refreshed.runtime, f.runtime);
        assert_eq!(
            serde_json::to_value(ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap())
                .unwrap(),
            serde_json::to_value(retained).unwrap(),
            "Policy enable never replays source history"
        );
        assert!(processes.as_ref().unwrap().exited_now());
        connection.abort();
        let _ = connection.await;
        server.stop().await;
    }
}

struct AckWitness {
    io: tokio::io::DuplexStream,
    flushes: usize,
    ack_observed: Arc<AtomicBool>,
    root: PathBuf,
    ledger: Arc<crate::grants::GrantLedger>,
    definition: Definition,
}
impl AsyncRead for AckWitness {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for AckWitness {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.io).poll_flush(cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.flushes += 1;
            if self.flushes == 2 {
                // Initial challenge is flush1; consumed ACK is flush2. Source
                // polling is causally downstream of this successful flush.
                let receipt = self
                    .ledger
                    .workload_receipt(&self.definition.issuer_id, &self.definition.grant_id)
                    .unwrap();
                assert_eq!(receipt.definition, self.definition);
                assert_eq!(receipt.receipt.state, "accepted");
                assert!(
                    !self.root.join("pids.json").exists(),
                    "No source opened before consumed ACK flush"
                );
                self.ack_observed.store(true, Ordering::SeqCst);
            }
        }
        result
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

fn native_delivery_client(
    f: &mut crate::workload_runtime::tests::Fixture,
    trace: &std::path::Path,
) -> (
    NodeClient,
    Writer,
    BufReader<tokio::io::DuplexStream>,
    Arc<AtomicBool>,
) {
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    f.definition.issuer_id = identity.fingerprint().unwrap();
    let request = crate::workload_policy::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: f.runtime.clone(),
        command: crate::workload_policy::Command::Show {},
    };
    let shown = f
        .service
        .workload_policy_command(request.clone(), false)
        .unwrap();
    let mut set = request;
    set.command = crate::workload_policy::Command::Set {
        policy: crate::workload_leases::Policy {
            enabled: true,
            ..Default::default()
        },
        expected_revision: shown["revision"].as_u64().unwrap(),
        expected_epoch: shown["runtime_epoch"].as_u64().unwrap(),
    };
    f.service.workload_policy_command(set, false).unwrap();
    let config: NodeClientConfig = serde_json::from_value(json!({"node_id":"node",
        "auth_mode":"node_credential","psk":crate::authentication::generate_credential(),
        "coordinator_certificate":identity.certificate,"endpoints":["127.0.0.1"]}))
    .unwrap();
    let client = NodeClient::new(config, f.engine.clone())
        .unwrap()
        .with_status(f.service.live.clone())
        .with_registration_runtime(&f.runtime)
        .unwrap()
        .with_grants(&f.dir.path().join("delivery.db"), Default::default());
    let ack_observed = Arc::new(AtomicBool::new(false));
    let (io, peer) = tokio::io::duplex(128 * 1024);
    let stream: crate::tls::NodeStream = Box::new(AckWitness {
        io,
        flushes: 0,
        ack_observed: ack_observed.clone(),
        root: trace.into(),
        ledger: client.grant_ledger.clone().unwrap(),
        definition: f.definition.clone(),
    });
    let (_input, output) = tokio::io::split(stream);
    (
        client,
        Arc::new(Mutex::new(output)),
        BufReader::new(peer),
        ack_observed,
    )
}
async fn delivery_frame(
    client: &NodeClient,
    peer: &mut BufReader<tokio::io::DuplexStream>,
) -> Value {
    timeout(Duration::from_secs(10), read_message(&client.crypto, peer))
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}
fn delivery_reply(
    frame: &Value,
    issuer: &mut crate::workload_leases::IssuerWindow,
    requests: &Arc<Requests>,
    d: &Definition,
) -> u64 {
    assert_eq!(frame["type"], "workload_probe");
    assert_eq!(frame["id"], d.grant_id);
    let query: crate::workload_leases::Query =
        serde_json::from_value(frame["query"].clone()).unwrap();
    let sequence = query.sequence;
    let reply = issuer
        .reply(&query, &d.issuer_id, &d.session_id, &d.executor_node)
        .unwrap();
    requests.complete(json!(reply));
    sequence
}

#[tokio::test]
async fn native_dispatch_flushes_consumption_before_source_and_renews_while_source_waits() {
    let (mut f, trace) = crate::workload_runtime::tests::native_fixture("finite");
    let (client, writer, mut peer, ack_observed) = native_delivery_client(&mut f, trace.path());
    let d = f.definition.clone();
    let requests = Arc::new(Requests::default());
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        d.lease_contract().unwrap(),
        &crate::workload_leases::Policy {
            enabled: true,
            ..Default::default()
        },
        Instant::now() + Duration::from_secs(50),
    )
    .unwrap();
    let failed = tokio::sync::watch::channel(false).0;
    let mut tasks = tokio::task::JoinSet::new();
    client
        .dispatch_workload(
            json!(crate::workload_network::Offer {
                kind: "execute_workload".into(),
                schema_version: 2,
                id: d.grant_id.clone(),
                expected_runtime_id: f.runtime.clone(),
                definition: d.clone()
            }),
            &writer,
            &failed,
            &mut tasks,
            Some(&d.issuer_id),
            Some(&d.session_id),
            requests.clone(),
        )
        .await
        .unwrap();
    let probe = delivery_frame(&client, &mut peer).await;
    assert_eq!(delivery_reply(&probe, &mut issuer, &requests, &d), 1);
    let ack = delivery_frame(&client, &mut peer).await;
    assert_eq!(ack["type"], "workload_ack");
    let saved: crate::grants::workload::Receipt =
        serde_json::from_value(ack["receipt"].clone()).unwrap();
    saved.validate().unwrap();
    assert_eq!(saved.definition, d);
    assert!(ack_observed.load(Ordering::SeqCst));
    issuer.acknowledge_consumed(&d.scope().unwrap()).unwrap();
    let processes = crate::process_lifetime::tests::Processes::observe(trace.path()).await;
    processes.alive();
    assert!(f.service.microphone_pool.reserve().is_err());
    // The real pinned source waits for this test's observation marker. Do not
    // release it until the same handler has delivered a real second challenge.
    let renewal = delivery_frame(&client, &mut peer).await;
    assert_eq!(delivery_reply(&renewal, &mut issuer, &requests, &d), 2);
    tokio::fs::write(
        trace.path().join("observed"),
        b"held process handles and renewed",
    )
    .await
    .unwrap();
    let mut events = 0;
    let completed = loop {
        let frame = delivery_frame(&client, &mut peer).await;
        match frame["type"].as_str().unwrap() {
            "workload_probe" => {
                delivery_reply(&frame, &mut issuer, &requests, &d);
            }
            "workload_event" => {
                events += 1;
                let event: crate::workload_events::Event =
                    serde_json::from_value(frame["event"].clone()).unwrap();
                assert_eq!(event.sequence, events);
                let history = client
                    .grant_ledger
                    .as_ref()
                    .unwrap()
                    .workload_events(&d.issuer_id, &d.grant_id, 0, 100)
                    .unwrap();
                assert!(
                    serde_json::to_string(&history)
                        .unwrap()
                        .contains("microphone_activity")
                );
                tokio::fs::write(
                    trace.path().join(format!("ack-{}", event.source_sequence)),
                    b"durable framed event",
                )
                .await
                .unwrap();
            }
            "workload_complete" => {
                break serde_json::from_value::<crate::grants::workload::Receipt>(
                    frame["receipt"].clone(),
                )
                .unwrap();
            }
            other => panic!("Unexpected source frame {other}: {frame}"),
        }
    };
    completed.validate().unwrap();
    assert_eq!(completed.receipt.state, "completed");
    assert_eq!(completed.definition, d);
    let report = completed.completion.unwrap();
    assert_eq!(report.event_count, events);
    assert!(report.lease_sequence >= 2);
    assert!(report.owned_process_exit_observed && !report.physical_effects_stopped);
    assert!(processes.exited_now());
    let _reusable = f.service.microphone_pool.reserve().unwrap();
    timeout(Duration::from_secs(3), tasks.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        client.status.lock().unwrap()["last_workload"]["state"],
        "completed"
    );
}

#[tokio::test]
async fn native_dispatch_stop_retires_blocked_owned_source_without_detached_renewal() {
    let (mut f, trace) = crate::workload_runtime::tests::native_fixture("blocked");
    let (client, writer, mut peer, ack_observed) = native_delivery_client(&mut f, trace.path());
    let d = f.definition.clone();
    let requests = Arc::new(Requests::default());
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        d.lease_contract().unwrap(),
        &crate::workload_leases::Policy {
            enabled: true,
            ..Default::default()
        },
        Instant::now() + Duration::from_secs(50),
    )
    .unwrap();
    let failed = tokio::sync::watch::channel(false).0;
    let mut tasks = tokio::task::JoinSet::new();
    client
        .dispatch_workload(
            json!(crate::workload_network::Offer {
                kind: "execute_workload".into(),
                schema_version: 2,
                id: d.grant_id.clone(),
                expected_runtime_id: f.runtime.clone(),
                definition: d.clone()
            }),
            &writer,
            &failed,
            &mut tasks,
            Some(&d.issuer_id),
            Some(&d.session_id),
            requests.clone(),
        )
        .await
        .unwrap();
    let probe = delivery_frame(&client, &mut peer).await;
    delivery_reply(&probe, &mut issuer, &requests, &d);
    assert_eq!(
        delivery_frame(&client, &mut peer).await["type"],
        "workload_ack"
    );
    assert!(ack_observed.load(Ordering::SeqCst));
    let processes = crate::process_lifetime::tests::Processes::observe(trace.path()).await;
    processes.alive();
    tokio::fs::write(trace.path().join("observed"), b"held process handles")
        .await
        .unwrap();
    let ledger = client.grant_ledger.as_ref().unwrap();
    let stop = ledger
        .request_stop_scope(&d.scope().unwrap(), &d.issuer_id, &d.session_id, "node")
        .unwrap();
    assert!(stop.cancellation_signalled && !stop.execution_stopped);
    let rejected = delivery_frame(&client, &mut peer).await;
    assert_eq!(rejected["type"], "workload_rejected");
    assert_eq!(rejected["code"], "source_unconfirmed");
    timeout(Duration::from_secs(3), tasks.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(tasks.is_empty());
    timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(next) = f.service.microphone_pool.reserve() {
                assert!(processes.exited_now());
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
    assert_eq!(
        ledger
            .workload_receipt(&d.issuer_id, &d.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(
        timeout(
            Duration::from_millis(100),
            read_message(&client.crypto, &mut peer)
        )
        .await
        .is_err(),
        "No detached renewal continues after the session task ended"
    );
}
