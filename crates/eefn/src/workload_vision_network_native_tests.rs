// Real authenticated framing, node admission, leases, journal and owned Python;
// camera/JPEG/model are explicit fixtures. No hardware/model-quality claim.
#[tokio::test]
async fn authenticated_vision_execution_negotiates_then_retains_bounded_results_after_owner_revocation()
 {
    let (listener, url) = listener().await;
    let mut f = Fixture::new(url, false);
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
    let config = serde_json::from_value(json!({"node_id":"node","auth_mode":"node_credential",
        "psk":credential,"coordinator_certificate":identity.certificate,
        "endpoints":[format!("127.0.0.1:{}",server.port())],"ollama_url":"http://127.0.0.1:9",
        "heartbeat_seconds":0.2}))
    .unwrap();
    let client = crate::NodeClient::new(config, f.engine.clone())
        .unwrap()
        .with_registration_runtime(&f.runtime)
        .unwrap()
        .with_grants(&f.dir.path().join("receipts.db"), Default::default())
        .with_status(f.service.live.clone());
    let mut connection = tokio::spawn(async move { client.connect_once().await });
    let session = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if let Ok(session) = server.workload_session(&f.definition.template(), 5).await {
                break session;
            }
            if connection.is_finished() {
                panic!(
                    "Vision node exited before negotiation: {:?}",
                    (&mut connection).await
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(session.runtime, f.runtime);
    assert!(session.policy.vision_analysis);
    let state = server.node_state("node").await.unwrap();
    assert_eq!(
        state.registration["workload_vision_execution"]["offer_schema_version"],
        3
    );
    assert!(
        state.registration["workload_execution"]["policy"]
            .get("vision_analysis")
            .is_none(),
        "Metadata protocol remains unchanged"
    );
    let source = server
        .workload_source(&f.definition.template(), 5)
        .await
        .unwrap();
    source
        .confirms_execution(&f.definition.template(), &session.runtime)
        .unwrap();
    f.definition.issuer_id = identity.fingerprint().unwrap();
    f.definition.session_id = session.session.clone();
    let d = f.definition.clone();
    let budget = std::time::Duration::from_millis(d.limits.max_session_ms.min(50_000));
    let mut issuer = crate::workload_leases::IssuerWindow::new(
        d.lease_contract().unwrap(),
        &session.policy,
        Instant::now() + budget,
    )
    .unwrap();
    assert!(!f.root().join("trace").exists());
    let mut link = server
        .open_workload(d.clone(), tokio::time::Instant::now() + budget)
        .await
        .unwrap();
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    let (started, ready) = tokio::sync::oneshot::channel();
    let root = f.dir.path().to_path_buf();
    let waiting = hold.clone();
    let provider = tokio::spawn(async move {
        provider(
            listener,
            vec![tags(), answer(), tags()],
            root,
            Some(waiting),
            Some(started),
        )
        .await
    });
    let mut ready = Some(ready);
    let mut events = Vec::new();
    let mut processes = None;
    let mut acknowledged = false;
    let receipt = loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(15), link.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.peer().session(), d.session_id);
        assert_eq!(frame.peer().runtime(), f.runtime);
        let raw = frame.message().to_string();
        for forbidden in [
            "PRIVATE_CAMERA_PASSWORD",
            "rtsp://",
            "\"image_base64\"",
            "/9gBAgME/9k=",
        ] {
            assert!(
                !raw.contains(forbidden),
                "Private camera data entered coordinator framing"
            );
        }
        match frame.message()["type"].as_str().unwrap() {
            "workload_probe" => {
                let query: crate::workload_leases::Query =
                    serde_json::from_value(frame.message()["query"].clone()).unwrap();
                let reply = issuer
                    .reply(
                        &query,
                        frame.peer().issuer(),
                        frame.peer().session(),
                        frame.peer().node(),
                    )
                    .unwrap();
                link.reply_lease(&query, reply).unwrap();
            }
            "workload_ack" => {
                let ack: crate::grants::workload::Receipt =
                    serde_json::from_value(frame.message()["receipt"].clone()).unwrap();
                ack.validate().unwrap();
                assert_eq!(ack.definition, d);
                assert_eq!(ack.receipt.state, "accepted");
                issuer.acknowledge_consumed(&d.scope().unwrap()).unwrap();
                acknowledged = true;
            }
            "workload_event" => {
                assert!(acknowledged);
                let event: crate::workload_network::Event =
                    serde_json::from_value(frame.message()["event"].clone()).unwrap();
                if processes.is_none() {
                    processes =
                        Some(crate::process_lifetime::tests::Processes::observe(&f.root()).await);
                }
                if event.sequence >= 2 {
                    if let Some(ready) = ready.take() {
                        tokio::time::timeout(std::time::Duration::from_secs(5), ready)
                            .await
                            .unwrap()
                            .unwrap();
                        processes.as_ref().unwrap().alive();
                        assert!(f.service.camera_pool.reserve().is_err());
                        hold.add_permits(1);
                    }
                }
                events.push(event);
            }
            "workload_complete" => {
                let receipt: crate::grants::workload::Receipt =
                    serde_json::from_value(frame.message()["receipt"].clone()).unwrap();
                receipt.validate().unwrap();
                assert!(acknowledged && ready.is_none());
                assert_eq!(receipt.definition, d);
                assert_eq!(receipt.receipt.state, "completed");
                break receipt;
            }
            _ => panic!(
                "Unexpected vision frame kind={} code={} reason={}",
                frame.message()["type"],
                frame.message()["code"],
                f.service
                    .live
                    .lock()
                    .unwrap()
                    .pointer("/last_workload/reason_code")
                    .cloned()
                    .unwrap_or(Value::Null)
            ),
        }
    };
    let completion = receipt.completion.as_ref().unwrap();
    assert_eq!(completion.schema_version, 3);
    issuer
        .close_after_report(&d.scope().unwrap(), completion.lease_sequence)
        .unwrap();
    let summary = crate::workload_network::summarize_events(&d, &events).unwrap();
    assert_eq!(
        completion.event_payload_prefix_sha256.as_deref(),
        Some(summary.prefix_sha256.as_str())
    );
    let history = server
        .completed_workload_analyses(
            &d,
            tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(history.receipt, receipt.receipt);
    assert_eq!(json!(history.completion), json!(completion));
    assert_eq!(history.analyses.len(), 1);
    assert_eq!(history.analyses[0].text(), Some("movement detected"));
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), provider)
            .await
            .unwrap()
            .unwrap(),
        [
            "GET /api/tags HTTP/1.1",
            "POST /api/chat HTTP/1.1",
            "GET /api/tags HTTP/1.1"
        ]
    );
    reusable(&f, processes.as_ref().unwrap()).await;
    f.vision_policy(false);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if server
                .node_state("node")
                .await
                .is_some_and(|s| s.registration["workload_vision_execution"]["available"] == false)
            {
                break;
            }
            assert!(
                !connection.is_finished(),
                "Vision policy refresh retired the connection"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(server.workload_session(&d.template(), 5).await.is_err());
    let historical = server
        .completed_workload_analyses(
            &d,
            tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert_eq!(
        json!(historical),
        json!(history),
        "Historical reads never restore withdrawn execution"
    );
    assert_eq!(
        server.grant_session("node").await.unwrap()["session_id"],
        session.session
    );
    server.stop().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), connection)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let ledger = crate::grants::GrantLedger::open(
        &f.dir.path().join("receipts.db"),
        Default::default(),
        "node",
    )
    .unwrap();
    assert_eq!(
        json!(ledger.workload_receipt(&d.issuer_id, &d.grant_id).unwrap()),
        json!(receipt)
    );
}
