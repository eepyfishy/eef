use super::*;
use crate::{
    NodeEngine,
    engine::NodePolicy,
    grants::{ReceiptState, definition_digest},
};
use std::time::Duration;

struct Fixture {
    transport: crate::peer::tests::Fixture,
    grants: [ExecutionGrant; 2],
    bytes: Vec<u8>,
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let mut transport = crate::peer::tests::Fixture::new();
        transport.scope.limits.max_frame_bytes = 64;
        let bytes = b"manifest-bound private synthetic transport payload ".repeat(9);
        let grants = [Role::Source, Role::Destination].map(|role| {
            let party = transport.scope.party(role);
            let remote = transport.scope.party(if role == Role::Source {
                Role::Destination
            } else {
                Role::Source
            });
            let mut g = crate::grants::tests::grant();
            g.grant_id = party.grant_id.clone();
            g.session_id = party.control_session_id.clone();
            g.executor_node = party.node_id.clone();
            g.issuer_id = transport.scope.issuer_id.clone();
            g.job_id = transport.scope.job_id.clone();
            g.plan_sha256 = transport.scope.plan_sha256.clone();
            g.request_context = transport.scope.request_context.clone();
            g.capability = "peer.transfer".into();
            g.action = if role == Role::Source {
                "send"
            } else {
                "receive"
            }
            .into();
            g.resource_id = Some(party.resource_id.clone());
            g.params = json!(Definition {
                schema_version: 1,
                resource_id: party.resource_id.clone(),
                peer_node: remote.node_id.clone(),
                peer_resource_id: remote.resource_id.clone(),
                payload_bytes: bytes.len() as u64,
                payload_sha256: Some(hex::encode(Sha256::digest(&bytes))),
                audio: None,
                limits: transport.scope.limits.clone()
            });
            g.issued_at_ms = transport.scope.issued_at_ms;
            g.expires_at_ms = transport.scope.expires_at_ms;
            g.limits.max_execution_ms = g.expires_at_ms - g.issued_at_ms;
            g.validate().unwrap();
            g
        });
        transport.scope.source.grant_sha256 = definition_digest(&json!(grants[0])).unwrap();
        transport.scope.destination.grant_sha256 = definition_digest(&json!(grants[1])).unwrap();
        transport.reseal();
        Self {
            transport,
            grants,
            bytes,
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn index(role: Role) -> usize {
        if role == Role::Source { 0 } else { 1 }
    }
    fn grant(&self, role: Role) -> &ExecutionGrant {
        &self.grants[Self::index(role)]
    }
    fn path(&self, role: Role) -> std::path::PathBuf {
        self.dir.path().join(if role == Role::Source {
            "source.db"
        } else {
            "destination.db"
        })
    }
    fn ledger(&self, role: Role) -> Arc<GrantLedger> {
        GrantLedger::open(
            &self.path(role),
            Default::default(),
            &self.grant(role).executor_node,
        )
        .unwrap()
    }
    fn metadata() -> crate::context::NodeMetadata {
        serde_json::from_value(
            json!({"resources":[{"id":"transport","capability":"peer.transfer"}]}),
        )
        .unwrap()
    }
    fn policy() -> NodePolicy {
        NodePolicy {
            peer_transfers: Policy {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn engine(&self, role: Role, enabled: bool) -> NodeEngine {
        let e = NodeEngine::new(false, vec![], self.dir.path().to_owned())
            .unwrap()
            .with_policy(Self::policy())
            .unwrap()
            .with_resources(self.grant(role).executor_node.clone(), Self::metadata())
            .unwrap();
        if enabled { e.with_peer_admission() } else { e }
    }
    async fn lease(g: &ExecutionGrant, remaining: u64) -> crate::grant_time::GrantLease {
        // Synthetic authenticated demultiplexer, NOT a physical issuer exchange.
        // No wall-clock constructor/fallback is added to production code.
        let manager = Arc::new(crate::grant_time::LeaseRequests::default());
        manager
            .request(g, &uuid::Uuid::new_v4().simple().to_string(), |value| {
                let manager = manager.clone();
                async move {
                    let query: crate::grant_time::LeaseQuery = serde_json::from_value(value)?;
                    manager.complete(json!(crate::grant_time::LeaseReply::new(&query, remaining)));
                    Ok(())
                }
            })
            .await
            .unwrap()
    }
    async fn prepare(
        &self,
        role: Role,
        engine: &NodeEngine,
        ledger: Arc<GrantLedger>,
        remaining: u64,
    ) -> Result<AdmittedPeer> {
        let g = self.grant(role);
        AdmittedPeer::prepare(
            g.clone(),
            ledger,
            engine,
            &self.transport.scope,
            if role == Role::Source {
                &self.transport.pair.source
            } else {
                &self.transport.pair.destination
            },
            self.transport.recipient(role),
            Self::lease(g, remaining).await,
        )
    }
}

#[tokio::test]
async fn finite_transport_uses_consumed_grants_and_retained_receipts_without_payload_storage() {
    let mut f = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.transport.scope.destination_address = listener.local_addr().unwrap();
    f.transport.reseal();
    let a = f.engine(Role::Source, true);
    let b = f.engine(Role::Destination, true);
    let la = f.ledger(Role::Source);
    let lb = f.ledger(Role::Destination);
    let source = f
        .prepare(Role::Source, &a, la.clone(), 20000)
        .await
        .unwrap();
    let destination = f
        .prepare(Role::Destination, &b, lb.clone(), 20000)
        .await
        .unwrap();
    for (side, ledger, admitted) in [
        (Role::Source, &la, &source),
        (Role::Destination, &lb, &destination),
    ] {
        let g = f.grant(side);
        assert!(admitted.acknowledgement().confirms(g));
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        assert!(
            f.prepare(
                side,
                if side == Role::Source { &a } else { &b },
                ledger.clone(),
                20000
            )
            .await
            .is_err()
        );
    }
    let sender = async {
        let tcp = tokio::net::TcpStream::connect(f.transport.scope.destination_address)
            .await
            .unwrap();
        source.run(&a, tcp, Some(f.bytes.clone())).await
    };
    let receiver = async {
        let (tcp, _) = listener.accept().await.unwrap();
        destination.run(&b, tcp, None).await
    };
    let (sent, received) = tokio::join!(sender, receiver);
    for (role, report, ledger) in [
        (Role::Source, sent, &la),
        (Role::Destination, received, &lb),
    ] {
        let g = f.grant(role);
        assert_eq!(report.state, ReceiptState::Completed);
        assert!(report.success && report.persisted && report.confirms(g));
        assert_eq!(report.data["payload_bytes"], f.bytes.len());
        assert_eq!(report.data["sink_effects_confirmed"], false);
        let saved = ledger
            .result(
                &g.issuer_id,
                &g.grant_id,
                &definition_digest(&json!(g)).unwrap(),
            )
            .unwrap();
        assert_eq!(saved["report"], json!(report));
        let disk = std::fs::read(f.path(role)).unwrap();
        assert!(!disk.windows(f.bytes.len()).any(|w| w == f.bytes));
        let mut forged = report;
        forged.data["payload_bytes"] = json!(f.bytes.len() + 1);
        assert!(!forged.confirms(g));
        assert!(
            f.prepare(
                role,
                if role == Role::Source { &a } else { &b },
                ledger.clone(),
                10000
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn ordinary_runtime_and_unsigned_or_substituted_delivery_do_not_consume_receipts() {
    let f = Fixture::new();
    let g = f.grant(Role::Source);
    let ledger = f.ledger(Role::Source);
    let disabled = f.engine(Role::Source, false);
    assert!(!disabled.capabilities().contains(&"peer.transfer".into()));
    let before = std::fs::read(f.path(Role::Source)).unwrap();
    assert!(
        f.prepare(Role::Source, &disabled, ledger.clone(), 10000)
            .await
            .is_err()
    );
    let engine = f.engine(Role::Source, true);
    assert!(
        AcceptedGrant::prepare_with_lease(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
            Fixture::lease(g, 10000).await
        )
        .is_err()
    );
    assert!(
        engine
            .execute("peer.transfer", "send", g.params.clone())
            .await
            .is_err()
    );
    for change in ["signature", "digest", "manifest", "lease", "role"] {
        let mut material = f.transport.pair.source.clone();
        let mut scope = f.transport.scope.clone();
        let mut grant = g.clone();
        let lease_grant;
        match change {
            "signature" => material.signature = String::new(),
            "digest" => scope.source.grant_sha256 = "c".repeat(64),
            "manifest" => grant.params["payload_bytes"] = json!(1),
            "role" => material.role = Role::Destination,
            "lease" => {}
            _ => unreachable!(),
        }
        lease_grant = if change == "lease" {
            f.grant(Role::Destination)
        } else {
            &grant
        };
        let result = AdmittedPeer::prepare(
            grant.clone(),
            ledger.clone(),
            &engine,
            &scope,
            &material,
            f.transport.recipient(Role::Source),
            Fixture::lease(lease_grant, 10000).await,
        );
        assert!(result.is_err(), "{change}");
    }
    assert_eq!(std::fs::read(f.path(Role::Source)).unwrap(), before);
}

#[tokio::test]
async fn dropped_admission_and_reopened_journal_never_readmit_an_attempt() {
    let f = Fixture::new();
    let engine = f.engine(Role::Source, true);
    let ledger = f.ledger(Role::Source);
    let admitted = f
        .prepare(Role::Source, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    assert!(engine.peer_slot().is_err());
    drop(admitted);
    assert!(engine.peer_slot().is_ok());
    let g = f.grant(Role::Source);
    assert_eq!(
        ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
        "unknown"
    );
    drop(ledger);
    let reopened = f.ledger(Role::Source);
    assert!(
        f.prepare(Role::Source, &engine, reopened, 10000)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn fresh_issuer_lease_not_scope_time_limits_a_stalled_peer_handshake() {
    let f = Fixture::new();
    let engine = f.engine(Role::Source, true);
    let ledger = f.ledger(Role::Source);
    let source = f
        .prepare(Role::Source, &engine, ledger.clone(), 250)
        .await
        .unwrap();
    let (a, _b) = tokio::io::duplex(1024);
    let report = tokio::time::timeout(
        Duration::from_secs(2),
        source.run(&engine, a, Some(f.bytes.clone())),
    )
    .await
    .unwrap();
    assert_eq!(report.state, ReceiptState::Unknown);
    assert!(!report.success && report.confirms(f.grant(Role::Source)));
    assert!(engine.peer_slot().is_ok());
    assert!(
        f.prepare(Role::Source, &engine, ledger, 10000)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn cancelled_transport_retains_uncertainty_and_releases_only_its_runtime_slot() {
    let f = Fixture::new();
    let engine = Arc::new(f.engine(Role::Source, true));
    let ledger = f.ledger(Role::Source);
    let source = f
        .prepare(Role::Source, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let (a, _b) = tokio::io::duplex(1024);
    let e = engine.clone();
    let bytes = f.bytes.clone();
    let task = tokio::spawn(async move { source.run(&e, a, Some(bytes)).await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let g = f.grant(Role::Source);
    assert_eq!(
        ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
        "unknown"
    );
    assert!(engine.peer_slot().is_ok());
    assert!(
        f.prepare(Role::Source, &engine, ledger, 10000)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn saved_permission_revocation_interrupts_pending_transport_without_a_dashboard() {
    let f = Fixture::new();
    let g = f.grant(Role::Source);
    let config = f.dir.path().join("node.json");
    let mut value = json!({"node_id":g.executor_node,"permissions":Fixture::policy(),"metadata":Fixture::metadata()});
    std::fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    let engine = f
        .engine(Role::Source, true)
        .with_service(crate::NodeService::new(
            config.clone(),
            g.executor_node.clone(),
        ));
    let ledger = f.ledger(Role::Source);
    let source = f
        .prepare(Role::Source, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let (a, _b) = tokio::io::duplex(1024);
    let operation = source.run(&engine, a, Some(f.bytes.clone()));
    let revoke = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        value["permissions"]["peer_transfers"]["enabled"] = json!(false);
        std::fs::write(&config, serde_json::to_vec(&value).unwrap()).unwrap();
    };
    let (report, _) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(operation, revoke)
    })
    .await
    .unwrap();
    assert_eq!(report.state, ReceiptState::Unknown);
    assert_eq!(report.error_code.as_deref(), Some("local_policy_changed"));
    assert!(report.confirms(g));
    assert!(
        f.prepare(Role::Source, &engine, ledger, 10000)
            .await
            .is_err()
    );
}

#[test]
fn strict_static_definitions_and_permissions_do_not_imply_feature_activation() {
    let f = Fixture::new();
    let g = f.grant(Role::Source);
    for key in [
        "destination_address",
        "token",
        "private_key",
        "execution_authorized",
    ] {
        let mut wrong = g.clone();
        wrong.params[key] = json!(true);
        assert!(wrong.validate().is_err());
    }
    let (role, d) = Definition::from_grant(g).unwrap();
    assert_eq!(role, Role::Source);
    assert!(Policy::default().permits(&d).is_err());
    assert!(
        !f.engine(Role::Source, false)
            .capabilities()
            .contains(&"peer.transfer".into())
    );
    let mut wrong = g.clone();
    wrong.resource_id = None;
    assert!(wrong.validate().is_err());
    let mut wrong = g.clone();
    wrong.action = "run".into();
    assert!(wrong.validate().is_err());
    assert!(
        !f.engine(Role::Source, true)
            .capabilities()
            .contains(&"peer.transfer".into()),
        "experimental admission must not advertise a public operation"
    );
}

#[tokio::test]
async fn node_permission_limits_resources_and_full_capacity_deny_before_consumption() {
    let f = Fixture::new();
    let ledger = f.ledger(Role::Source);
    let before = std::fs::read(f.path(Role::Source)).unwrap();
    for fault in ["disabled", "bytes", "wire", "rate", "resource"] {
        let mut policy = Fixture::policy();
        match fault {
            "disabled" => policy.peer_transfers.enabled = false,
            "bytes" => policy.peer_transfers.max_payload_bytes = 1,
            "wire" => policy.peer_transfers.max_wire_bytes = 1,
            "rate" => policy.peer_transfers.max_bytes_per_second = 1,
            _ => {}
        }
        let mut engine = f.engine(Role::Source, true).with_policy(policy).unwrap();
        if fault == "resource" {
            engine = engine
                .with_resources(
                    f.grant(Role::Source).executor_node.clone(),
                    Default::default(),
                )
                .unwrap();
        }
        assert!(
            f.prepare(Role::Source, &engine, ledger.clone(), 10000)
                .await
                .is_err(),
            "{fault}"
        );
    }
    assert_eq!(std::fs::read(f.path(Role::Source)).unwrap(), before);
    let engine = f.engine(Role::Source, true);
    let admitted = f
        .prepare(Role::Source, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let other = Fixture::new();
    let other_ledger = other.ledger(Role::Source);
    let before = std::fs::read(other.path(Role::Source)).unwrap();
    assert!(
        other
            .prepare(Role::Source, &engine, other_ledger.clone(), 10000)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(other.path(Role::Source)).unwrap(), before);
    drop(admitted);
    assert!(
        other
            .prepare(Role::Source, &engine, other_ledger, 10000)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn full_existing_receipt_journal_refuses_admission_without_eviction() {
    let f = Fixture::new();
    let g = f.grant(Role::Source);
    let policy = crate::grants::GrantPolicy {
        max_receipts: 1,
        ..Default::default()
    };
    let ledger = GrantLedger::open(&f.path(Role::Source), policy, &g.executor_node).unwrap();
    let mut previous = g.clone();
    previous.task_id = uuid::Uuid::new_v4().simple().to_string();
    previous.grant_id = uuid::Uuid::new_v4().to_string();
    previous.capability = "system.info".into();
    previous.action = "run".into();
    previous.resource_id = None;
    previous.params = json!({});
    ledger.claim(&previous).unwrap();
    let before = std::fs::read(f.path(Role::Source)).unwrap();
    assert!(
        f.prepare(
            Role::Source,
            &f.engine(Role::Source, true),
            ledger.clone(),
            10000
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(f.path(Role::Source)).unwrap(), before);
    assert_eq!(
        ledger
            .receipt(&previous.issuer_id, &previous.grant_id)
            .unwrap()["state"],
        "accepted"
    );
}

#[tokio::test]
async fn substituted_payloads_fail_without_emitting_a_successful_transport_receipt() {
    let f = Fixture::new();
    let engine = f.engine(Role::Source, true);
    let source = f
        .prepare(Role::Source, &engine, f.ledger(Role::Source), 10000)
        .await
        .unwrap();
    let (a, mut b) = tokio::io::duplex(1024);
    let report = source.run(&engine, a, Some(vec![1])).await;
    assert_eq!(report.state, ReceiptState::Unknown);
    let mut buf = [0; 16];
    assert_eq!(
        tokio::io::AsyncReadExt::read(&mut b, &mut buf)
            .await
            .unwrap(),
        0,
        "source manifest failure must precede TLS IO"
    );
    let f = Fixture::new();
    let engine = f.engine(Role::Destination, true);
    let destination = f
        .prepare(
            Role::Destination,
            &engine,
            f.ledger(Role::Destination),
            10000,
        )
        .await
        .unwrap();
    let (a, b) = tokio::io::duplex(1024);
    let malicious = async {
        let mut source = f.transport.endpoint(Role::Source).connect(a).await.unwrap();
        let mut bytes = f.bytes.clone();
        *bytes.last_mut().unwrap() ^= 1;
        let frames = bytes
            .chunks(f.transport.scope.limits.max_frame_bytes as usize)
            .collect::<Vec<_>>();
        for (i, frame) in frames.iter().enumerate() {
            source.send_data(frame).await.unwrap();
            let acknowledged = source.receive_acknowledgement().await;
            if i + 1 == frames.len() {
                assert!(acknowledged.is_err());
            } else {
                acknowledged.unwrap();
            }
        }
    };
    let (_, received) = tokio::join!(malicious, destination.run(&engine, b, None));
    assert_eq!(received.state, ReceiptState::Unknown);
    assert!(
        !received.success && received.persisted && received.confirms(f.grant(Role::Destination))
    );
    assert!(received.data.is_null());
}

#[tokio::test]
async fn native_socket_readiness_owns_listener_and_accept_wait_expires_under_original_lease() {
    let mut f = Fixture::new();
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    f.transport.scope.destination_address = address;
    f.transport.reseal();
    let engine = f.engine(Role::Destination, true);
    let ledger = f.ledger(Role::Destination);
    let admitted = f
        .prepare(Role::Destination, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    // Keep native socket ownership, but separate the exact deadline transition
    // from host scheduling and SQLite latency. This advances the original
    // admitted lease; it neither resets nor lengthens execution authority.
    let deadline = admitted.accepted.execution_deadline();
    let clock = crate::grants::ControlledClock::at(&admitted.accepted);
    let expiry = clock.clone();
    let started = std::time::Instant::now();
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = ready.clone();
    let report = admitted
        .run_tcp_with_clock(
            &engine,
            None,
            |ack| async move {
                assert_eq!(ack.state, ReceiptState::Accepted);
                assert!(
                    tokio::net::TcpListener::bind(address).await.is_err(),
                    "ACK preceded native socket ownership"
                );
                observed.store(true, std::sync::atomic::Ordering::SeqCst);
                expiry.advance_to(deadline);
                Ok(())
            },
            clock,
        )
        .await;
    assert!(
        ready.load(std::sync::atomic::Ordering::SeqCst),
        "Ready was not observed: {:?}",
        report.error_code
    );
    assert_eq!(report.state, ReceiptState::Unknown);
    assert!(report.persisted && !report.success);
    assert!(matches!(
        report.error_code.as_deref(),
        Some("grant_deadline_exceeded" | "grant_deadline_or_clock_changed")
    ));
    assert!(started.elapsed() < Duration::from_secs(3));
    let _released = tokio::net::TcpListener::bind(address).await.unwrap();
    assert_eq!(
        ledger
            .receipt(
                &f.grant(Role::Destination).issuer_id,
                &f.grant(Role::Destination).grant_id
            )
            .unwrap()["state"],
        "unknown"
    );
    assert!(engine.peer_slot().is_ok());
}

#[tokio::test]
async fn native_expired_original_lease_never_binds_or_calls_readiness() {
    let mut f = Fixture::new();
    // Keep this socket owned. An incorrectly polled operation would return a
    // bind failure, not the required deadline outcome before source effects.
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = occupied.local_addr().unwrap();
    f.transport.scope.destination_address = address;
    f.transport.reseal();
    let engine = f.engine(Role::Destination, true);
    let ledger = f.ledger(Role::Destination);
    let admitted = f
        .prepare(Role::Destination, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let clock = crate::grants::ControlledClock::at(&admitted.accepted);
    clock.advance_to(admitted.accepted.execution_deadline());
    let report = admitted
        .run_tcp_with_clock(
            &engine,
            None,
            |_| async {
                panic!("Expired authority cannot reach readiness");
                #[allow(unreachable_code)]
                Ok(())
            },
            clock,
        )
        .await;
    assert_eq!(report.state, ReceiptState::Unknown);
    assert_eq!(
        report.error_code.as_deref(),
        Some("grant_deadline_or_clock_changed")
    );
    assert!(report.persisted && !report.success && report.data.is_null());
    assert_eq!(
        ledger
            .receipt(
                &f.grant(Role::Destination).issuer_id,
                &f.grant(Role::Destination).grant_id
            )
            .unwrap()["state"],
        "unknown"
    );
    assert!(engine.peer_slot().is_ok());
    assert!(
        f.prepare(Role::Destination, &engine, ledger, 10000)
            .await
            .is_err(),
        "Consumed authority must never replay"
    );
    assert_eq!(occupied.local_addr().unwrap(), address);
}

#[tokio::test]
async fn blocked_native_readiness_output_cannot_extend_the_original_lease() {
    let mut f = Fixture::new();
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    f.transport.scope.destination_address = address;
    f.transport.reseal();
    let engine = f.engine(Role::Destination, true);
    let ledger = f.ledger(Role::Destination);
    let admitted = f
        .prepare(Role::Destination, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let deadline = admitted.accepted.execution_deadline();
    let clock = crate::grants::ControlledClock::at(&admitted.accepted);
    let expiry = clock.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready = observed.clone();
    let report = tokio::time::timeout(
        Duration::from_secs(3),
        admitted.run_tcp_with_clock(
            &engine,
            None,
            |_| async move {
                assert!(tokio::net::TcpListener::bind(address).await.is_err());
                ready.store(true, std::sync::atomic::Ordering::SeqCst);
                expiry.advance_to(deadline);
                std::future::pending::<Result<()>>().await
            },
            clock,
        ),
    )
    .await
    .unwrap();
    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "Readiness not polled: {:?}",
        report.error_code
    );
    assert_eq!(report.state, ReceiptState::Unknown);
    assert!(matches!(
        report.error_code.as_deref(),
        Some("grant_deadline_exceeded" | "grant_deadline_or_clock_changed")
    ));
    assert!(report.persisted && !report.success);
    let _released = tokio::net::TcpListener::bind(address).await.unwrap();
    assert!(engine.peer_slot().is_ok());
    assert_eq!(
        ledger
            .receipt(
                &f.grant(Role::Destination).issuer_id,
                &f.grant(Role::Destination).grant_id
            )
            .unwrap()["state"],
        "unknown"
    );
}

#[tokio::test]
async fn cancelling_native_ready_wait_releases_socket_and_fences_consumed_grant() {
    let mut f = Fixture::new();
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    f.transport.scope.destination_address = address;
    f.transport.reseal();
    let engine = Arc::new(f.engine(Role::Destination, true));
    let ledger = f.ledger(Role::Destination);
    let admitted = f
        .prepare(Role::Destination, &engine, ledger.clone(), 10000)
        .await
        .unwrap();
    let (started, seen) = tokio::sync::oneshot::channel();
    let running_engine = engine.clone();
    let work = tokio::spawn(async move {
        admitted
            .run_tcp(&running_engine, None, |_| async move {
                started.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpListener::bind(address).await.is_err());
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let _released = tokio::net::TcpListener::bind(address).await.unwrap();
    assert!(engine.peer_slot().is_ok());
    let g = f.grant(Role::Destination);
    assert_eq!(
        ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
        "unknown"
    );
    assert!(
        f.prepare(Role::Destination, &engine, ledger, 10000)
            .await
            .is_err()
    );
}

#[test]
fn tcp_system_clock_wrapper_does_not_duplicate_large_readiness_future() {
    async fn large_ready(_: GrantReport) -> Result<()> {
        let bytes = [7_u8; 64 * 1024];
        std::future::pending::<()>().await;
        std::hint::black_box(&bytes);
        Ok(())
    }
    fn size<F, R>(_: F) -> usize
    where
        F: FnOnce(AdmittedPeer, &'static NodeEngine) -> R,
    {
        std::mem::size_of::<R>()
    }
    let direct = size(|admitted, engine| {
        admitted.run_tcp_with_clock(engine, None, large_ready, crate::grants::SystemClock)
    });
    let normal = size(|admitted, engine| admitted.run_tcp(engine, None, large_ready));
    assert_eq!(
        normal, direct,
        "TCP wrapper must not store a duplicate large async frame"
    );
}
