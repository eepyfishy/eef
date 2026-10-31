// Included in the real journal test module to reuse explicit synthetic lease
// fixtures. Network authentication below is real loopback pinned TLS; no source
// factory, microphone, model or physical device is started.
#[tokio::test]
async fn workload_inspection_reads_exact_history_without_writes_or_version_fallback() {
    use crate::workload_inspection::{Advertisement, WireRequest};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inspect.db");
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let d = definition();
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let raw = json!({"type":"workload_receipt","id":uuid::Uuid::new_v4().simple().to_string(),"scope":d.scope().unwrap()});
    let query: WireRequest = serde_json::from_value(raw.clone()).unwrap();
    let before = std::fs::read(&path).unwrap();
    let receipt = query.read(&ledger, &d.issuer_id, &d.executor_node).unwrap();
    assert_eq!(receipt.definition, d);
    assert_eq!(receipt.receipt.state, "accepted");
    assert!(!receipt.execution_authorized && !receipt.restart_authorized);
    for (field, value) in [
        ("session_id", json!(uuid::Uuid::new_v4().to_string())),
        ("grant_sha256", json!("a".repeat(64))),
        ("grant_id", json!(uuid::Uuid::new_v4().to_string())),
        ("job_id", json!(uuid::Uuid::new_v4().simple().to_string())),
    ] {
        let mut bad = raw.clone();
        bad["scope"][field] = value;
        let query: WireRequest = serde_json::from_value(bad).unwrap();
        assert!(
            query.read(&ledger, &d.issuer_id, &d.executor_node).is_err(),
            "{field}"
        );
    }
    assert!(
        query
            .read(&ledger, &"f".repeat(64), &d.executor_node)
            .is_err()
    );
    assert!(query.read(&ledger, &d.issuer_id, "different-node").is_err());
    let mut extra = raw;
    extra["execute"] = json!(true);
    assert!(serde_json::from_value::<WireRequest>(extra).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    for (field, value) in [
        ("schema_version", json!(1)),
        ("receipt_versions", json!([1])),
        ("read_only", json!(false)),
    ] {
        let mut bad = json!(Advertisement::new(true));
        bad[field] = value;
        assert!(
            serde_json::from_value::<Advertisement>(bad)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    consumed.retire().await.unwrap();
    let legacy_path = dir.path().join("ordinary.db");
    let legacy = GrantLedger::open(&legacy_path, Default::default(), "owner").unwrap();
    let before = std::fs::read(&legacy_path).unwrap();
    assert!(query.read(&legacy, &d.issuer_id, &d.executor_node).is_err());
    assert_eq!(version(&legacy_path), 2);
    assert_eq!(std::fs::read(&legacy_path).unwrap(), before);
}

#[tokio::test]
async fn pinned_workload_inspection_survives_reconnect_without_recreating_a_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("network.db");
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    let key = crate::authentication::generate_credential();
    let server = crate::NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([("owner".into(), Some(key.clone()))]),
            tls: Some(identity.clone()),
        },
    )
    .unwrap();
    server.start().await.unwrap();
    let config: crate::NodeClientConfig=serde_json::from_value(json!({"node_id":"owner","psk":key,"auth_mode":"node_credential",
        "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",server.port())],"heartbeat_seconds":0.2,"ollama_url":"http://127.0.0.1:1"})).unwrap();
    let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let start = |with_journal: bool| {
        let client = crate::NodeClient::new(config.clone(), engine.clone()).unwrap();
        let client = if with_journal {
            client.with_grants(&path, Default::default())
        } else {
            client
        };
        tokio::spawn(async move { client.connect_once().await })
    };
    let wait_ready = || async {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(session) = server.grant_session("owner").await {
                    break session;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    };
    let task = start(true);
    let first = wait_ready().await;
    let ledger = GrantLedger::open(&path, Default::default(), "owner").unwrap();
    let mut d = definition();
    d.issuer_id = first["issuer_id"].as_str().unwrap().into();
    assert_ne!(
        d.session_id,
        first["session_id"].as_str().unwrap(),
        "Fixture is historical, not current authority"
    );
    let consumed = consume(&ledger, d.clone()).await.unwrap();
    let before = std::fs::read(&path).unwrap();
    let receipt = server.inspect_workload_receipt(&d).await.unwrap();
    assert_eq!(receipt.definition, d);
    assert_eq!(receipt.receipt.state, "accepted");
    assert!(!receipt.execution_authorized && !receipt.restart_authorized);
    let mut changed = d.clone();
    changed.priority += 1;
    assert!(server.inspect_workload_receipt(&changed).await.is_err());
    changed = d.clone();
    changed.issuer_id = "f".repeat(64);
    assert!(server.inspect_workload_receipt(&changed).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    consumed.retire().await.unwrap();
    let unknown = server.inspect_workload_receipt(&d).await.unwrap();
    assert_eq!(unknown.receipt.state, "unknown");
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !server.connected_nodes().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let task = start(true);
    let second = wait_ready().await;
    assert_ne!(first["session_id"], second["session_id"]);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        json!(server.inspect_workload_receipt(&d).await.unwrap()),
        json!(unknown)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !server.connected_nodes().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let task = start(false);
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.node_state("owner").await.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        server.inspect_workload_receipt(&d).await.is_err(),
        "Unavailable read advertisement cannot fall back to an ordinary grant"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    task.abort();
    let _ = task.await;
    server.stop().await;
}

#[tokio::test]
async fn workload_receipts_never_use_legacy_shared_key_transport() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-network.db");
    let server = crate::NodeServer::new("isolated-workload-read-secret", "127.0.0.1", 0).unwrap();
    server.start().await.unwrap();
    let config: crate::NodeClientConfig = serde_json::from_value(
        json!({"node_id":"owner","psk":"isolated-workload-read-secret",
        "endpoints":[format!("127.0.0.1:{}",server.port())],"ollama_url":"http://127.0.0.1:1"}),
    )
    .unwrap();
    let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let client = crate::NodeClient::new(config, engine)
        .unwrap()
        .with_grants(&path, Default::default());
    let task = tokio::spawn(async move { client.connect_once().await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.node_state("owner").await.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(
        server
            .inspect_workload_receipt(&definition())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(version(&path), 2);
    task.abort();
    let _ = task.await;
    server.stop().await;
}
