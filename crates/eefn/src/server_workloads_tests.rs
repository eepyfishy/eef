use super::*;
use crate::workload_leases::{Policy, definition::*};

fn policy() -> Policy {
    Policy {
        enabled: true,
        ..Default::default()
    }
}
fn definition(issuer: &str, session: &str) -> Definition {
    Definition {
        schema_version: 2,
        grant_id: Uuid::new_v4().to_string(),
        issuer_id: issuer.into(),
        session_id: session.into(),
        executor_node: "owner".into(),
        job_id: Uuid::new_v4().simple().to_string(),
        task_id: Uuid::new_v4().simple().to_string(),
        attempt: 1,
        plan_sha256: "b".repeat(64),
        request_context: crate::context::RequestContext::new(
            Uuid::new_v4().to_string(),
            "origin".into(),
            vec!["home".into()],
        )
        .unwrap(),
        priority: 0,
        source: Source::MicrophoneEnergy {
            resource_id: "owner::mic".into(),
            resource_sha256: "d".repeat(64),
            adapter_sha256: "e".repeat(64),
            parameters: EnergyParameters {
                sample_rate: 16000,
                channels: 1,
                block_frames: 80,
                max_blocks: 100,
                activation_level: 1000,
                deactivation_level: 500,
            },
        },
        limits: Limits {
            max_session_ms: 60000,
            max_lease_ms: 10000,
            max_job_memory_mib: 128,
            max_source_frames: 8000,
        },
        events: Events {
            mode: EventMode::ActivityTransitions,
            min_interval_ms: 100,
            max_events: 100,
            max_event_bytes: 1024,
            max_total_bytes: 65536,
            max_pending: 4,
            overflow: Overflow::Stop,
        },
    }
}
fn support(runtime: &str) -> Advertisement {
    Advertisement {
        schema_version: 2,
        available: true,
        runtime_id: Some(runtime.into()),
        policy: Some(policy()),
        source_kind: "microphone_energy".into(),
        event_delivery: "bounded_metadata".into(),
        restart_authorized: false,
    }
}
fn server() -> (NodeServer, crate::tls::CoordinatorIdentity) {
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    let server = NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            tls: Some(identity.clone()),
            credentials: std::collections::BTreeMap::from([("owner".into(), Some("a".repeat(64)))]),
        },
    )
    .unwrap();
    (server, identity)
}
async fn fixture() -> (NodeServer, Definition, mpsc::Receiver<Value>) {
    let (server, identity) = server();
    let session = Uuid::new_v4();
    let runtime = Uuid::new_v4().to_string();
    let d = definition(&identity.fingerprint().unwrap(), &session.to_string());
    let (sender, receiver) = mpsc::channel(64);
    server.inner.connections.write().await.insert("owner".into(), Connection {
        credential_transport: true, session_id: session,
        info: ConnectionInfo { node_id: "owner".into(), source_ip: "127.0.0.1".into(), source_port: 1, connected_since_ms: now_ms() },
        sender, advertisement: None, last_seen: Instant::now(),
        state: Some(NodeStateSnapshot { registration: json!({"node_id":"owner", "capabilities":[CAPABILITY],
            "registration_receipt":crate::registration::Offer::new(&runtime).unwrap(), "workload_execution":support(&runtime)}), heartbeat: None }),
    });
    (server, d, receiver)
}
fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(10)
}
fn ack(d: &Definition) -> crate::grants::workload::Receipt {
    crate::grants::workload::Receipt {
        schema_version: 2,
        definition: d.clone(),
        completion: None,
        execution_authorized: false,
        restart_authorized: false,
        receipt: crate::request_receipt::ConsumedReceipt {
            schema_version: 1,
            node_id: d.executor_node.clone(),
            issuer_id: d.issuer_id.clone(),
            grant_id: d.grant_id.clone(),
            grant_sha256: d.scope().unwrap().grant_sha256,
            job_id: d.job_id.clone(),
            task_id: d.task_id.clone(),
            attempt: 1,
            state: "accepted".into(),
            accepted_at_ms: 1,
            updated_at_ms: 1,
            pipeline_progress: None,
        },
    }
}
fn acknowledgement(d: &Definition) -> Value {
    json!({"type":"workload_ack","schema_version":2,"id":d.grant_id,"receipt":ack(d)})
}
fn event(d: &Definition, sequence: u64) -> Value {
    let source_sequence = 1 + (sequence - 1) * 20;
    json!({"type":"workload_event","schema_version":2,"id":d.grant_id,"event":{
        "schema_version":2,"kind":"microphone_activity","sequence":sequence,"source_sequence":source_sequence,
        "source_frames":source_sequence*80,"elapsed_ms":(sequence-1)*100,"active":sequence%2==1,"level":1000,"discontinuity":false
    }})
}
fn complete(d: &Definition, count: u64) -> Value {
    // Synthetic peer metadata only. It does not construct owned-source evidence.
    let mut receipt = ack(d);
    receipt.schema_version = 3;
    receipt.receipt.state = "completed".into();
    receipt.receipt.updated_at_ms = 2;
    receipt.completion = Some(crate::grants::workload::CompletionReport {
        schema_version: 1,
        grant_sha256: d.scope().unwrap().grant_sha256,
        adapter_sha256: "e".repeat(64),
        source_observations: 100,
        source_frames: 8000,
        event_count: count,
        event_bytes: usize::try_from(count).unwrap().checked_mul(300).unwrap(),
        event_prefix_sha256: "f".repeat(64),
        lease_sequence: 1,
        recorded_at_ms: 2,
        owned_process_exit_observed: true,
        physical_effects_stopped: false,
    });
    receipt.validate().unwrap();
    json!({"type":"workload_complete","schema_version":2,"id":d.grant_id,"receipt":receipt})
}
fn query(d: &Definition, sequence: u64) -> Query {
    Query {
        kind: "workload_lease_probe".into(),
        schema_version: 2,
        id: Uuid::new_v4().to_string(),
        contract: d.lease_contract().unwrap(),
        sequence,
    }
}
fn route(server: &NodeServer, d: &Definition, message: Value) {
    server
        .inner
        .workloads
        .route(
            &d.executor_node,
            Uuid::parse_str(&d.session_id).unwrap(),
            message,
        )
        .unwrap();
}

#[test]
fn workload_network_metadata_requires_exact_runtime_scope_and_no_restart_claim() {
    let runtime = Uuid::new_v4().to_string();
    let d = definition(&"a".repeat(64), &Uuid::new_v4().to_string());
    support(&runtime).validate().unwrap();
    Advertisement::unavailable().validate().unwrap();
    let offer = Offer {
        kind: "execute_workload".into(),
        schema_version: 2,
        id: d.grant_id.clone(),
        expected_runtime_id: runtime.clone(),
        definition: d.clone(),
    };
    offer
        .validate(&d.issuer_id, &d.session_id, "owner", &runtime)
        .unwrap();
    for (issuer, session, node, generation) in [
        (
            "b".repeat(64),
            d.session_id.clone(),
            "owner",
            runtime.clone(),
        ),
        (
            d.issuer_id.clone(),
            Uuid::new_v4().to_string(),
            "owner",
            runtime.clone(),
        ),
        (
            d.issuer_id.clone(),
            d.session_id.clone(),
            "other",
            runtime.clone(),
        ),
        (
            d.issuer_id.clone(),
            d.session_id.clone(),
            "owner",
            Uuid::new_v4().to_string(),
        ),
    ] {
        assert!(
            offer
                .validate(&issuer, &session, node, &generation)
                .is_err()
        );
    }
    let mut invalid = support(&runtime);
    invalid.restart_authorized = true;
    assert!(invalid.validate().is_err());
    let mut invalid = support(&runtime);
    invalid.policy.as_mut().unwrap().enabled = false;
    assert!(invalid.validate().is_err());
    let mut unknown = json!(offer);
    unknown["legacy_grant"] = json!({});
    assert!(serde_json::from_value::<Offer>(unknown).is_err());
}

#[tokio::test]
async fn workload_offer_refuses_missing_support_legacy_transport_and_changed_session() {
    for mode in 0..4 {
        let (server, mut d, mut output) = fixture().await;
        match mode {
            0 => {
                server
                    .inner
                    .connections
                    .write()
                    .await
                    .get_mut("owner")
                    .unwrap()
                    .credential_transport = false
            }
            1 => {
                server
                    .inner
                    .connections
                    .write()
                    .await
                    .get_mut("owner")
                    .unwrap()
                    .state
                    .as_mut()
                    .unwrap()
                    .registration
                    .as_object_mut()
                    .unwrap()
                    .remove("workload_execution");
            }
            2 => d.session_id = Uuid::new_v4().to_string(),
            3 => {
                server
                    .inner
                    .connections
                    .write()
                    .await
                    .get_mut("owner")
                    .unwrap()
                    .state
                    .as_mut()
                    .unwrap()
                    .registration["registration_receipt"]["runtime_id"] =
                    json!(Uuid::new_v4().to_string())
            }
            _ => unreachable!(),
        }
        assert!(server.open_workload(d, deadline()).await.is_err());
        assert!(output.try_recv().is_err());
        assert!(server.inner.workloads.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn workload_intake_requires_actual_pinned_peer_and_preserves_other_generations() {
    let (server, d, mut output) = fixture().await;
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    let offered: Offer = serde_json::from_value(output.recv().await.unwrap()).unwrap();
    offered
        .validate(&d.issuer_id, &d.session_id, "owner", link.peer().runtime())
        .unwrap();
    server
        .inner
        .workloads
        .route(
            "other",
            Uuid::parse_str(&d.session_id).unwrap(),
            acknowledgement(&d),
        )
        .unwrap();
    server
        .inner
        .workloads
        .route("owner", Uuid::new_v4(), acknowledgement(&d))
        .unwrap();
    assert!(
        server
            .inner
            .workloads
            .0
            .lock()
            .unwrap()
            .contains_key(&d.grant_id)
    );
    assert!(link.control.try_recv().is_err());
    route(&server, &d, acknowledgement(&d));
    let received = link.next().await.unwrap();
    assert_eq!(received.message()["type"], "workload_ack");
    assert_eq!(received.peer().issuer(), d.issuer_id);
    assert_eq!(received.peer().session(), d.session_id);
    assert_eq!(received.peer().node(), "owner");
}

#[tokio::test]
async fn active_event_processing_keeps_reviewed_pending_and_global_byte_ownership() {
    let (server, mut d, _output) = fixture().await;
    d.events.max_pending = 1;
    let bytes = server.inner.report_bytes.available_permits();
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    route(&server, &d, acknowledgement(&d));
    drop(link.next().await.unwrap());
    route(&server, &d, event(&d, 1));
    let active = link.next().await.unwrap();
    assert!(server.inner.report_bytes.available_permits() < bytes);
    assert_eq!(
        server.inner.workloads.0.lock().unwrap()[&d.grant_id]
            .event_slots
            .available_permits(),
        0
    );
    route(&server, &d, event(&d, 2));
    assert!(
        link.next().await.is_err(),
        "Dequeued but unprocessed event still owns max_pending"
    );
    assert!(server.inner.connections.read().await.contains_key("owner"));
    drop(active);
    drop(link);
    assert_eq!(server.inner.report_bytes.available_permits(), bytes);
}

#[tokio::test]
async fn lease_control_bypasses_events_and_terminal_receipt_drains_prior_prefix() {
    let (server, d, _output) = fixture().await;
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    route(&server, &d, acknowledgement(&d));
    drop(link.next().await.unwrap());
    route(&server, &d, event(&d, 1));
    route(&server, &d, event(&d, 2));
    let q = query(&d, 2);
    route(
        &server,
        &d,
        json!({"type":"workload_probe","schema_version":2,"id":d.grant_id,"query":q}),
    );
    route(&server, &d, complete(&d, 2));
    assert_eq!(
        link.next().await.unwrap().message()["type"],
        "workload_probe"
    );
    for sequence in 1..=2 {
        assert_eq!(
            link.next().await.unwrap().message()["event"]["sequence"],
            sequence
        );
    }
    assert_eq!(
        link.next().await.unwrap().message()["type"],
        "workload_complete"
    );
    assert!(link.next().await.is_err());
}

#[tokio::test]
async fn malformed_workload_scope_events_and_unreviewed_media_retire_only_the_owned_route() {
    for mode in 0..7 {
        let (server, d, _output) = fixture().await;
        let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
        if mode != 0 {
            route(&server, &d, acknowledgement(&d));
            drop(link.next().await.unwrap());
        }
        let message = match mode {
            0 => event(&d, 1),
            1 => {
                let mut bad = acknowledgement(&d);
                bad["receipt"]["restart_authorized"] = json!(true);
                bad
            }
            2 => {
                let mut bad = event(&d, 1);
                bad["event"]["raw_audio"] = json!("unreviewed");
                bad
            }
            3 => {
                route(&server, &d, event(&d, 1));
                event(&d, 1)
            }
            4 => {
                let mut bad = event(&d, 1);
                bad["event"]["source_frames"] = json!(1);
                bad
            }
            5 => complete(&d, 1),
            6 => {
                route(&server, &d, event(&d, 1));
                let mut bad = complete(&d, 1);
                bad["receipt"]["receipt"]["accepted_at_ms"] = json!(2);
                bad
            }
            _ => unreachable!(),
        };
        route(&server, &d, message);
        assert!(link.next().await.is_err());
        assert!(server.inner.connections.read().await.contains_key("owner"));
        drop(link);
        assert_eq!(
            server.inner.report_bytes.available_permits(),
            MAX_QUEUED_REPORT_BYTES
        );
    }
}

#[tokio::test]
async fn workload_rejection_bypasses_queued_observations_and_retires_further_lease_delivery() {
    let (server, d, _output) = fixture().await;
    let bytes = server.inner.report_bytes.available_permits();
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    route(&server, &d, acknowledgement(&d));
    drop(link.next().await.unwrap());
    route(&server, &d, event(&d, 1));
    route(&server, &d, event(&d, 2));
    route(
        &server,
        &d,
        json!({"type":"workload_rejected","schema_version":2,"id":d.grant_id,"code":"source_unconfirmed"}),
    );
    let stopped = link.next().await.unwrap();
    assert_eq!(stopped.message()["type"], "workload_rejected");
    assert!(link.next().await.is_err());
    let q = query(&d, 1);
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        d.lease_contract().unwrap(),
        &policy(),
        Instant::now() + Duration::from_secs(50),
    )
    .unwrap();
    let reply = issuer
        .reply(&q, &d.issuer_id, &d.session_id, &d.executor_node)
        .unwrap();
    assert!(link.reply_lease(&q, reply).is_err());
    assert!(server.inner.connections.read().await.contains_key("owner"));
    drop(stopped);
    drop(link);
    assert_eq!(server.inner.report_bytes.available_permits(), bytes);
}

#[tokio::test]
async fn control_and_events_share_bytes_through_active_processing_without_borrowing_count_slots() {
    let (mut server, d, _output) = fixture().await;
    let ack = acknowledgement(&d);
    let event = event(&d, 1);
    let budget =
        serde_json::to_vec(&ack).unwrap().len() + serde_json::to_vec(&event).unwrap().len();
    Arc::get_mut(&mut server.inner).unwrap().report_bytes =
        Arc::new(tokio::sync::Semaphore::new(budget));
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    route(&server, &d, ack);
    let active = link.next().await.unwrap();
    route(&server, &d, event);
    assert_eq!(server.inner.report_bytes.available_permits(), 0);
    let q = query(&d, 1);
    route(
        &server,
        &d,
        json!({"type":"workload_probe","schema_version":2,"id":d.grant_id,"query":q}),
    );
    assert!(link.next().await.is_err());
    drop(active);
    drop(link);
    assert_eq!(server.inner.report_bytes.available_permits(), budget);
    assert!(server.inner.connections.read().await.contains_key("owner"));
}

#[tokio::test]
async fn stale_workload_link_drop_cannot_remove_or_answer_a_replacement_session() {
    let (server, d, mut output) = fixture().await;
    let old = server.open_workload(d.clone(), deadline()).await.unwrap();
    output.recv().await.unwrap();
    server
        .inner
        .workloads
        .disconnect("owner", Uuid::parse_str(&d.session_id).unwrap());
    let mut next = d.clone();
    next.session_id = Uuid::new_v4().to_string();
    server
        .inner
        .connections
        .write()
        .await
        .get_mut("owner")
        .unwrap()
        .session_id = Uuid::parse_str(&next.session_id).unwrap();
    let mut replacement = server
        .open_workload(next.clone(), deadline())
        .await
        .unwrap();
    drop(old);
    assert!(
        server
            .inner
            .workloads
            .0
            .lock()
            .unwrap()
            .contains_key(&d.grant_id)
    );
    route(&server, &d, acknowledgement(&d));
    assert!(replacement.control.try_recv().is_err());
    route(&server, &next, acknowledgement(&next));
    assert_eq!(
        replacement.next().await.unwrap().peer().session(),
        next.session_id
    );
    let q = query(&d, 1);
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        q.contract.clone(),
        &policy(),
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    let reply = issuer
        .reply(&q, &d.issuer_id, &d.session_id, "owner")
        .unwrap();
    assert!(replacement.reply_lease(&q, reply).is_err());
}

#[tokio::test]
async fn offer_queue_failure_disconnect_and_gateway_stop_release_workload_routes() {
    for mode in 0..3 {
        let (server, d, _output) = fixture().await;
        if mode == 0 {
            let sender = server.inner.connections.read().await["owner"]
                .sender
                .clone();
            for _ in 0..64 {
                sender.try_send(json!({"type":"control"})).unwrap();
            }
            assert!(server.open_workload(d, deadline()).await.is_err());
        } else {
            let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
            if mode == 1 {
                server
                    .fail_pending_for("owner", Uuid::parse_str(&d.session_id).unwrap())
                    .await;
            } else {
                server.stop().await;
            }
            assert!(link.next().await.is_err());
        }
        assert!(server.inner.workloads.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn actual_pinned_tls_frames_round_trip_workload_challenge_and_bounded_terminal_prefix() {
    let (server, identity) = server();
    let mut registered = server.subscribe();
    server.start().await.unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
        .await
        .unwrap();
    crate::authentication::write_selector(&mut stream, "owner")
        .await
        .unwrap();
    let stream = crate::tls::connect(
        stream,
        &identity.certificate,
        crate::tls::client_config(&identity.certificate).unwrap(),
    )
    .await
    .unwrap();
    let (input, mut output) = tokio::io::split(stream);
    let mut input = BufReader::new(input);
    let crypto = NodeCrypto::new(&"a".repeat(64)).unwrap();
    write_message(
        &crypto,
        &mut output,
        &crate::protocol::build_auth(&crypto, "owner", crate::VERSION),
    )
    .await
    .unwrap();
    let authenticated = timeout(Duration::from_secs(3), read_message(&crypto, &mut input))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let runtime = Uuid::new_v4().to_string();
    write_message(&crypto, &mut output, &json!({"type":"register","node_id":"owner","capabilities":[CAPABILITY],
        "registration_receipt":crate::registration::Offer::new(&runtime).unwrap(), "workload_execution":support(&runtime)})).await.unwrap();
    assert!(matches!(
        timeout(Duration::from_secs(3), registered.recv())
            .await
            .unwrap()
            .unwrap(),
        NodeEvent::Registered(_)
    ));
    let registration = timeout(Duration::from_secs(3), read_message(&crypto, &mut input))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(registration["type"], "registration_accepted");
    let d = definition(
        &identity.fingerprint().unwrap(),
        authenticated["grant_session_id"].as_str().unwrap(),
    );
    let mut link = server.open_workload(d.clone(), deadline()).await.unwrap();
    let offer: Offer = serde_json::from_value(
        timeout(Duration::from_secs(3), read_message(&crypto, &mut input))
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    offer
        .validate(&d.issuer_id, &d.session_id, "owner", &runtime)
        .unwrap();
    let requests = Arc::new(crate::workload_leases::Requests::default());
    let contract = d.lease_contract().unwrap();
    let wire = d.grant_id.clone();
    let writer = &mut output;
    let secured = &crypto;
    let mut requesting = Box::pin(requests.request(&contract, 1, move |q| async move {
        write_message(
            secured,
            writer,
            &json!({"type":"workload_probe","schema_version":2,"id":wire,"query":q}),
        )
        .await
    }));
    let frame = tokio::select! {
        result = &mut requesting => panic!("Challenge returned before authenticated reply: {}", result.is_ok()),
        frame = link.next() => frame.unwrap(),
    };
    let q: Query = serde_json::from_value(frame.message()["query"].clone()).unwrap();
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        contract.clone(),
        &policy(),
        Instant::now() + Duration::from_secs(10),
    )
    .unwrap();
    let reply = issuer
        .reply(
            &q,
            frame.peer().issuer(),
            frame.peer().session(),
            frame.peer().node(),
        )
        .unwrap();
    link.reply_lease(&q, reply).unwrap();
    drop(frame);
    requests.complete(
        timeout(Duration::from_secs(3), read_message(&crypto, &mut input))
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    );
    let proof = requesting.await.unwrap();
    proof.initial_deadline(&contract, &policy()).unwrap();
    // These are simulated peer receipts, not real consumption/source evidence.
    for message in [acknowledgement(&d), event(&d, 1), complete(&d, 1)] {
        write_message(&crypto, &mut output, &message).await.unwrap();
    }
    for kind in ["workload_ack", "workload_event", "workload_complete"] {
        let frame = timeout(Duration::from_secs(3), link.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.message()["type"], kind);
        assert_eq!(frame.peer().session(), d.session_id);
        assert_eq!(frame.peer().issuer(), identity.fingerprint().unwrap());
    }
    assert!(link.next().await.is_err());
    drop(link);
    drop(output);
    drop(input);
    server.stop().await;
}
