//! Real authenticated CONTROL sockets with emulated executor receipts. This
//! tests coordinator ordering, NOT peer payload IO or physical two-PC acceptance.
use super::*;
use eefn::protocol::{read_message, write_message};
use tokio::io::BufReader;

struct Wire {
    io: BufReader<eefn::tls::NodeStream>,
    crypto: eefn::NodeCrypto,
}
impl Wire {
    async fn connect(server: &eefn::NodeServer, node: &str) -> Self {
        let policy = server.authentication_policy();
        let certificate = &policy.tls.as_ref().unwrap().certificate;
        let secret = policy.credentials[node].as_deref().unwrap();
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", server.port()))
            .await
            .unwrap();
        eefn::authentication::write_selector(&mut socket, node)
            .await
            .unwrap();
        let secured = eefn::tls::connect(
            socket,
            certificate,
            eefn::tls::client_config(certificate).unwrap(),
        )
        .await
        .unwrap();
        let mut wire = Self {
            io: BufReader::new(secured),
            crypto: eefn::NodeCrypto::new(secret).unwrap(),
        };
        wire.send(eefn::protocol::build_auth(
            &wire.crypto,
            node,
            eefn::VERSION,
        ))
        .await;
        assert_eq!(wire.read().await["type"], "ok");
        let offer = eefn::registration::Offer::new(&uuid::Uuid::new_v4().to_string()).unwrap();
        let mut registration = eefn::protocol::build_register(
            node,
            node,
            eefn::VERSION,
            &["peer.transfer".into()],
            &json!({}),
            &[],
        );
        registration["metadata"] =
            json!({"resources":[{"id":"transport","capability":"peer.transfer","available":true}]});
        registration["execution_grants"] = json!({"schema_version":1,"available":true,"limits":eefn::grants::GrantPolicy::default(),
            "deadline_guarantee":"rpc_future_only","timing_mode":eefn::grant_time::MODE,"retained_results":true});
        registration["registration_receipt"] = json!(offer);
        registration["peer_transfer"] = json!({"schema_version":1,"finite_transport":true,"runtime_generation":offer.runtime_id,
            "destination_address":if node=="destination" {Some("127.0.0.1:12345")} else {None}});
        wire.send(registration).await;
        assert_eq!(wire.read().await["type"], "registration_accepted");
        wire
    }
    async fn send(&mut self, value: Value) {
        write_message(&self.crypto, &mut self.io, &value)
            .await
            .unwrap();
    }
    async fn read(&mut self) -> Value {
        read_message(&self.crypto, &mut self.io)
            .await
            .unwrap()
            .unwrap()
    }
    async fn accept(&mut self, request: &Value, g: &ExecutionGrant) {
        let query = eefn::grant_time::LeaseQuery {
            kind: "grant_lease_probe".into(),
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            request_id: request["id"].as_str().unwrap().into(),
            node_id: g.executor_node.clone(),
            session_id: g.session_id.clone(),
            grant_id: g.grant_id.clone(),
            grant_sha256: definition_digest(&json!(g)).unwrap(),
        };
        self.send(json!(query)).await;
        let lease = self.read().await;
        assert_eq!(lease["type"], "grant_lease");
        assert!(lease["remaining_ms"].as_u64().unwrap() > 0);
        self.send(json!({"type":"grant_ack","id":request["id"],"grant":GrantReport::new(g,ReceiptState::Accepted,Value::Null,None)})).await;
    }
    fn receipt(request: &Value, g: &ExecutionGrant) -> GrantReport {
        let scope: Scope = serde_json::from_value(request["peer"]["scope"].clone()).unwrap();
        let (role, d) = eefn::peer::admission::Definition::from_grant(g).unwrap();
        GrantReport::new(
            g,
            ReceiptState::Completed,
            json!(eefn::peer::admission::TransferReceipt {
                schema_version: 1,
                pair_session_id: scope.session_id.clone(),
                scope_sha256: scope.digest().unwrap(),
                role,
                payload_bytes: d.payload_bytes,
                payload_sha256: d.payload_sha256,
                data_frames: d
                    .payload_bytes
                    .div_ceil(u64::from(d.limits.max_frame_bytes))
                    as u32,
                payload_retained: false,
                sink_effects_confirmed: false
            }),
            None,
        )
    }
}

#[tokio::test]
async fn real_control_dispatch_waits_for_durable_destination_ack_and_both_outcomes() {
    let f = Fixture::new(2 * 1024 * 1024);
    f.server.start().await.unwrap();
    let mut source = Wire::connect(&f.server, "source").await;
    let mut destination = Wire::connect(&f.server, "destination").await;
    let registry = crate::capability::CapabilityRegistry::default();
    for node in ["source", "destination"] {
        let mut p = crate::capability::CapabilityProvider::local(
            "peer.transfer",
            "*",
            json!({"resources":[{
            "id":"transport","capability":"peer.transfer","available":true,"area":[]}]}),
        );
        p.node_id = node.into();
        registry.register(p);
    }
    let driver = crate::request_execution::Driver::peer_fixture(
        f.engine.clone(),
        f.server.clone(),
        registry,
    );
    let (ready_received, ready_seen) = tokio::sync::oneshot::channel();
    let (allow_ready, ready_gate) = tokio::sync::oneshot::channel();
    let (allow_finish, finish_gate) = tokio::sync::oneshot::channel();
    let (source_received, mut source_seen) = tokio::sync::oneshot::channel();
    let (done, closing) = tokio::sync::watch::channel(false);
    let store = f.engine.store.clone();
    let mut closing_dest = closing.clone();
    let dest = tokio::spawn(async move {
        let req = destination.read().await;
        assert_eq!(req["type"], "execute_peer_grant");
        let g: ExecutionGrant = serde_json::from_value(req["grant"].clone()).unwrap();
        let saved = store.get(&g.job_id).unwrap();
        let e = saved.execution.unwrap();
        assert_eq!(e.attempts.len(), 2);
        assert!(e.attempts.iter().all(|a| a.state == AttemptState::Issued));
        assert_eq!(json!(e.peer.unwrap().scope), req["peer"]["scope"]);
        ready_received.send(()).unwrap();
        ready_gate.await.unwrap();
        destination.accept(&req, &g).await;
        finish_gate.await.unwrap();
        destination.send(json!({"type":"response","id":req["id"],"success":true,"grant":Wire::receipt(&req,&g)})).await;
        closing_dest.changed().await.unwrap();
    });
    let mut closing_source = closing;
    let src = tokio::spawn(async move {
        let req = source.read().await;
        assert_eq!(req["type"], "execute_peer_grant");
        source_received.send(()).unwrap();
        let g: ExecutionGrant = serde_json::from_value(req["grant"].clone()).unwrap();
        source.accept(&req, &g).await;
        source.send(json!({"type":"response","id":req["id"],"success":true,"grant":Wire::receipt(&req,&g)})).await;
        closing_source.changed().await.unwrap();
    });
    let id = f.plan.plan_id.clone();
    let job = id.clone();
    let execution = tokio::spawn(async move { driver.drive_peer(&job).await });
    tokio::time::timeout(Duration::from_secs(5), ready_seen)
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        matches!(
            source_seen.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "source sent before durable destination ACK"
    );
    allow_ready.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), source_seen)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = f.engine.store.get(&id).unwrap();
            if p.tasks[0].status == TaskStatus::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let waiting = f.engine.store.get(&id).unwrap();
    assert_eq!(waiting.status, PlanStatus::Running);
    assert_eq!(
        waiting.execution.as_ref().unwrap().attempts[1].state,
        AttemptState::Accepted
    );
    allow_finish.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), execution)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        f.engine.store.get(&id).unwrap().status,
        PlanStatus::Completed
    );
    done.send(true).unwrap();
    src.await.unwrap();
    dest.await.unwrap();
    f.server.stop().await;
}
