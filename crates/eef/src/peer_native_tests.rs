//! Real NodeClients, consumed node journals, coordinator driver and peer TCP/TLS.
//! In-memory finite producers and hash/discard receivers, not physical devices.
use super::*;
use sha2::{Digest, Sha256};

struct NativeNodes {
    clients: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
    status: Vec<Arc<std::sync::Mutex<Value>>>,
    ledgers: Vec<std::path::PathBuf>,
}
impl NativeNodes {
    async fn start(f: &Fixture, address: SocketAddr, bytes: &[u8]) -> Self {
        let mut nodes = Self {
            clients: vec![],
            status: vec![],
            ledgers: vec![],
        };
        for node in ["source", "destination"] {
            let generation = uuid::Uuid::new_v4().to_string();
            let metadata: eefn::context::NodeMetadata =
                serde_json::from_value(json!({"resources":[
                {"id":"transport","capability":"peer.transfer"},
                {"id":"unbound","capability":"peer.transfer"}]}))
                .unwrap();
            let engine = Arc::new(
                eefn::NodeEngine::new(false, vec![], f.dir.path().into())
                    .unwrap()
                    .with_policy(eefn::NodePolicy {
                        peer_transfers: eefn::peer::admission::Policy {
                            enabled: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .unwrap()
                    .with_resources(node.into(), metadata.clone())
                    .unwrap()
                    .with_peer_admission(),
            );
            let config: eefn::NodeClientConfig = serde_json::from_value(json!({
                "node_id":node,"metadata":metadata,"auth_mode":"node_credential",
                "psk":f.server.authentication_policy().credentials[node],
                "coordinator_certificate":f.server.authentication_policy().tls.as_ref().unwrap().certificate,
                "endpoints":[format!("127.0.0.1:{}",f.server.port())],"heartbeat_seconds":0.2})).unwrap();
            let runtime = if node == "source" {
                eefn::peer::runtime::FiniteRuntime::new(
                    node,
                    BTreeMap::from([("transport".into(), bytes.to_vec())]),
                    Default::default(),
                    None,
                )
            } else {
                eefn::peer::runtime::FiniteRuntime::new(
                    node,
                    Default::default(),
                    ["transport".into()].into(),
                    Some(address),
                )
            }
            .unwrap();
            let live = Arc::new(std::sync::Mutex::new(json!({"runtime_id":generation})));
            let ledger = f.dir.path().join(format!("{node}-grants.db"));
            let client = eefn::NodeClient::new(config, engine)
                .unwrap()
                .with_registration_runtime(&generation)
                .unwrap()
                .with_peer_runtime(runtime)
                .unwrap()
                .with_grants(&ledger, Default::default())
                .with_status(live.clone());
            nodes
                .clients
                .push(tokio::spawn(async move { client.connect_once().await }));
            nodes.status.push(live);
            nodes.ledgers.push(ledger);
        }
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if f.server.peer_grant_session("source").await.is_ok()
                    && f.server.peer_grant_session("destination").await.is_ok()
                    && nodes.status.iter().all(|s| {
                        s.lock().unwrap()["registration_observation"]["receipt"].is_object()
                    })
                {
                    break;
                }
                assert!(
                    nodes.clients.iter().all(|c| !c.is_finished()),
                    "native client ended before registration"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("native registrations");
        nodes
    }
    async fn stop(self, server: &eefn::NodeServer) {
        server.stop().await;
        for task in self.clients {
            let ended = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            assert!(
                ended.is_err(),
                "closed coordinator connection should end connect_once"
            );
        }
    }
}
fn driver(f: &Fixture) -> crate::request_execution::Driver {
    let registry = crate::capability::CapabilityRegistry::default();
    for node in ["source", "destination"] {
        let mut p = crate::capability::CapabilityProvider::local(
            "peer.transfer",
            "*",
            json!({"resources":[{"id":"transport","capability":"peer.transfer","available":true,"area":[]}]}),
        );
        p.node_id = node.into();
        registry.register(p);
    }
    crate::request_execution::Driver::peer_fixture(f.engine.clone(), f.server.clone(), registry)
}
struct Ignore;
impl eefn::grants::GrantObserver for Ignore {
    fn record(&self, _: &GrantReport) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn native_manifest_refusal_and_control_disconnect_do_not_leave_a_receiver_or_replay_work() {
    let approved = b"approved finite producer bytes";
    let different = b"changed finite producer bytes";
    let f = Fixture::with_manifest(
        2 * 1024 * 1024,
        approved.len() as u64,
        format!("{:x}", Sha256::digest(approved)),
    );
    f.server.start().await.unwrap();
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let nodes = NativeNodes::start(&f, address, different).await;
    let driver = driver(&f);
    let id = f.plan.plan_id.clone();
    let execution = tokio::spawn(async move { driver.drive_peer(&id).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = f.engine.store.get(&f.plan.plan_id).unwrap();
            if p.execution.as_ref().is_some_and(|e| {
                e.attempts
                    .first()
                    .is_some_and(|a| a.state == AttemptState::Rejected)
            }) {
                break;
            }
            assert!(
                !execution.is_finished(),
                "driver ended before manifest refusal"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let p = f.engine.store.get(&f.plan.plan_id).unwrap();
    let e = p.execution.as_ref().unwrap();
    assert_eq!(p.status, PlanStatus::Running);
    assert_eq!(e.attempts[1].state, AttemptState::Accepted);
    assert!(nodes.status[0].lock().unwrap()["last_workload"].is_null());
    assert!(
        tokio::net::TcpListener::bind(address).await.is_err(),
        "destination was not actually waiting on its native listener"
    );
    let ledgers = nodes.ledgers.clone();
    nodes.stop(&f.server).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(3), execution)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    // JoinSet drop requests cancellation; observe the owned listener actually
    // released before checking retained evidence. No claim of physical stop.
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(listener) = tokio::net::TcpListener::bind(address).await {
                drop(listener);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for (index, path) in ledgers.iter().enumerate() {
        // Do not reopen GrantLedger here: opening it reconciles unfinished
        // admissions and could manufacture the very Unknown being tested.
        use rusqlite::OptionalExtension;
        let journal =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let g = &e.attempts[index].grant;
        let state: Option<String> = journal
            .query_row(
                "SELECT state FROM execution_receipts WHERE issuer_id=?1 AND grant_id=?2",
                rusqlite::params![g.issuer_id, g.grant_id],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        if index == 0 {
            assert!(state.is_none(), "changed producer consumed a source grant");
        } else {
            assert_eq!(state.as_deref(), Some("unknown"));
        }
    }
    f.engine.store.interrupt(&f.plan.plan_id).unwrap();
    crate::jobs::validate_retained_review(&f.engine.store.get(&f.plan.plan_id).unwrap()).unwrap();
}

#[tokio::test]
async fn native_clients_complete_exact_pair_over_peer_tcp_and_refuse_consumed_replay() {
    let bytes = b"PRIVATE_NATIVE_PEER_PAYLOAD_".repeat(4);
    let f = Fixture::with_manifest(
        2 * 1024 * 1024,
        bytes.len() as u64,
        format!("{:x}", Sha256::digest(&bytes)),
    );
    f.server.start().await.unwrap();
    // Select an OS-assigned local test port, then release it. The executor must
    // bind it itself after consumption; retaining a fixture listener would hide
    // an absent native socket implementation.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    let nodes = NativeNodes::start(&f, address, &bytes).await;
    for node in ["source", "destination"] {
        let state = f.server.node_state(node).await.unwrap();
        assert_eq!(
            state.registration["metadata"]["resources"][1]["available"],
            false
        );
        assert!(
            !serde_json::to_string(&state.registration)
                .unwrap()
                .contains("PRIVATE_NATIVE_PEER_PAYLOAD_")
        );
    }
    tokio::time::timeout(
        Duration::from_secs(8),
        driver(&f).drive_peer(&f.plan.plan_id),
    )
    .await
    .unwrap()
    .unwrap();
    let p = f.engine.store.get(&f.plan.plan_id).unwrap();
    assert_eq!(p.status, PlanStatus::Completed);
    crate::jobs::validate_retained_review(&p).unwrap();
    let execution = p.execution.as_ref().unwrap();
    let issuance = execution.peer.as_ref().unwrap();
    assert_eq!(issuance.scope.coordinator_runtime_id, f.server.runtime_id());
    for (index, attempt) in execution.attempts.iter().enumerate() {
        assert_eq!(attempt.state, AttemptState::Completed);
        assert!(attempt.acknowledgement.is_some());
        let report = attempt.report.as_ref().unwrap();
        assert!(report.success && report.persisted && issuance.confirms(&attempt.grant, report));
        assert_eq!(report.data["payload_retained"], false);
        assert_eq!(report.data["sink_effects_confirmed"], false);
        let ledger = eefn::grants::GrantLedger::open(
            &nodes.ledgers[index],
            Default::default(),
            &attempt.grant.executor_node,
        )
        .unwrap();
        assert_eq!(
            ledger
                .receipt(&attempt.grant.issuer_id, &attempt.grant.grant_id)
                .unwrap()["state"],
            "completed"
        );
    }
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("PRIVATE_NATIVE_PEER_PAYLOAD_")
    );
    let g = &execution.attempts[0].grant;
    let replay = f
        .server
        .invoke_peer_grant_observed(
            g.clone(),
            issuance.scope.clone(),
            issuance.material.source.clone(),
            Duration::from_secs(3),
            Arc::new(Ignore),
            Instant::now() + Duration::from_secs(3),
        )
        .await
        .unwrap();
    let rejected: GrantReport = serde_json::from_value(replay["grant"].clone()).unwrap();
    assert_eq!(rejected.state, ReceiptState::Rejected);
    assert!(rejected.confirms(g));
    assert!(
        tokio::net::TcpStream::connect(address).await.is_err(),
        "receiver listener remained open after finite completion"
    );
    nodes.stop(&f.server).await;
}

#[tokio::test]
async fn native_destination_bind_failure_is_consumed_unknown_and_never_dispatches_source() {
    let bytes = b"approved finite bytes".to_vec();
    let f = Fixture::with_manifest(
        2 * 1024 * 1024,
        bytes.len() as u64,
        format!("{:x}", Sha256::digest(&bytes)),
    );
    f.server.start().await.unwrap();
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let nodes = NativeNodes::start(&f, occupied.local_addr().unwrap(), &bytes).await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        driver(&f).drive_peer(&f.plan.plan_id),
    )
    .await
    .unwrap();
    assert!(result.is_err());
    let p = f.engine.store.get(&f.plan.plan_id).unwrap();
    let e = p.execution.as_ref().unwrap();
    assert_eq!(e.attempts[0].state, AttemptState::Issued);
    assert_eq!(e.attempts[1].state, AttemptState::Unknown);
    assert!(e.attempts.iter().all(|a| a.acknowledgement.is_none()));
    assert!(e.attempts[1].report.as_ref().unwrap().persisted);
    assert!(nodes.status[0].lock().unwrap()["last_workload"].is_null());
    assert!(nodes.status[1].lock().unwrap()["last_workload"].is_null());
    let source = &e.attempts[0].grant;
    let ledger =
        eefn::grants::GrantLedger::open(&nodes.ledgers[0], Default::default(), "source").unwrap();
    assert!(
        ledger.receipt(&source.issuer_id, &source.grant_id).is_err(),
        "source grant was consumed without destination readiness"
    );
    // Same guard used by the real Runtime when a driver returns early.
    f.engine.store.interrupt(&f.plan.plan_id).unwrap();
    let saved = f.engine.store.get(&f.plan.plan_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    crate::jobs::validate_retained_review(&saved).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), occupied.accept())
            .await
            .is_err()
    );
    nodes.stop(&f.server).await;
}
