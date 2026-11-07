// Dispatch units use real encrypted framing on an in-memory stream. They do
// not substitute for the final credential/TLS connection acceptance suite.
use super::*;
use crate::grants::workload::tests::definition;

struct Fixture {
    _dir: tempfile::TempDir,
    client: NodeClient,
    definition: Definition,
    runtime: String,
    identity: crate::tls::CoordinatorIdentity,
    writer: Writer,
    peer: BufReader<tokio::io::DuplexStream>,
    failed: tokio::sync::watch::Sender<bool>,
    _service: Arc<crate::NodeService>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(credential: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
        let key = crate::authentication::generate_credential();
        let runtime = Uuid::new_v4().to_string();
        let mut d = definition();
        d.issuer_id = identity.fingerprint().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":"owner","psk":key,"permissions":{},"endpoints":[]}),
        )
        .unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone()),
        );
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        let config: NodeClientConfig = serde_json::from_value(json!({
            "node_id":"owner","psk":key,"auth_mode":if credential { "node_credential" } else { "legacy" },
            "coordinator_certificate":identity.certificate,"endpoints":["127.0.0.1"],"request_limits":{"work":1,"control":1}
        })).unwrap();
        let client = NodeClient::new(config, engine)
            .unwrap()
            .with_status(service.live.clone())
            .with_registration_runtime(&runtime)
            .unwrap()
            .with_grants(&dir.path().join("receipts.db"), Default::default());
        let (stream, peer) = tokio::io::duplex(128 * 1024);
        let stream: crate::tls::NodeStream = Box::new(stream);
        let (_input, output) = tokio::io::split(stream);
        Self {
            _dir: dir,
            client,
            definition: d,
            runtime,
            identity,
            writer: Arc::new(Mutex::new(output)),
            peer: BufReader::new(peer),
            failed: tokio::sync::watch::channel(false).0,
            _service: service,
            _guard: guard,
        }
    }
    fn offer(&self) -> Value {
        json!(crate::workload_network::Offer {
            kind: "execute_workload".into(),
            schema_version: 2,
            id: self.definition.grant_id.clone(),
            expected_runtime_id: self.runtime.clone(),
            definition: self.definition.clone()
        })
    }
    async fn reply(&mut self) -> Value {
        timeout(
            Duration::from_secs(3),
            read_message(&self.client.crypto, &mut self.peer),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap()
    }
    fn no_consumption(&self) {
        let ledger = self.client.grant_ledger.as_ref().unwrap();
        assert!(
            ledger
                .workload_receipt(&self.definition.issuer_id, &self.definition.grant_id)
                .is_err()
        );
        assert!(ledger.workload_reporting_ready());
    }
}

#[tokio::test]
async fn workload_dispatch_refuses_legacy_or_changed_scope_before_starting_a_task() {
    for mode in ["legacy", "issuer", "session", "runtime", "node", "schema"] {
        let mut f = Fixture::new(mode != "legacy");
        let mut offer = f.offer();
        match mode {
            "issuer" => offer["definition"]["issuer_id"] = json!("0".repeat(64)),
            "session" => offer["definition"]["session_id"] = json!(Uuid::new_v4().to_string()),
            "runtime" => offer["expected_runtime_id"] = json!(Uuid::new_v4().to_string()),
            "node" => offer["definition"]["executor_node"] = json!("other"),
            "schema" => offer["schema_version"] = json!(1),
            _ => {}
        }
        let mut tasks = tokio::task::JoinSet::new();
        f.client
            .dispatch_workload(
                offer,
                &f.writer,
                &f.failed,
                &mut tasks,
                Some(&f.identity.fingerprint().unwrap()),
                Some(&f.definition.session_id),
                Arc::new(Requests::default()),
            )
            .await
            .unwrap();
        let reply = f.reply().await;
        assert_eq!(reply["type"], "workload_rejected");
        assert_eq!(reply["code"], "policy_denied");
        assert!(tasks.is_empty());
        f.no_consumption();
    }
}

#[tokio::test]
async fn full_workload_capacity_refuses_without_borrowing_reserved_control_or_consuming() {
    let mut f = Fixture::new(true);
    let held = f
        .client
        .request_budget
        .admit("workload.session", "execute")
        .unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    f.client
        .dispatch_workload(
            f.offer(),
            &f.writer,
            &f.failed,
            &mut tasks,
            Some(&f.identity.fingerprint().unwrap()),
            Some(&f.definition.session_id),
            Arc::new(Requests::default()),
        )
        .await
        .unwrap();
    assert_eq!(f.reply().await["code"], "workload_busy");
    assert!(tasks.is_empty());
    assert!(
        f.client
            .request_budget
            .admit("system.ping", "ping")
            .is_some()
    );
    assert!(
        f.client
            .request_budget
            .admit("node.configure", "workload_policy")
            .is_some(),
        "Remote policy revocation cannot depend on free source execution capacity"
    );
    f.no_consumption();
    drop(held);
}

#[tokio::test]
async fn default_disabled_owner_policy_refuses_before_a_lease_probe_or_consumption() {
    let mut f = Fixture::new(true);
    let mut tasks = tokio::task::JoinSet::new();
    f.client
        .dispatch_workload(
            f.offer(),
            &f.writer,
            &f.failed,
            &mut tasks,
            Some(&f.identity.fingerprint().unwrap()),
            Some(&f.definition.session_id),
            Arc::new(Requests::default()),
        )
        .await
        .unwrap();
    timeout(Duration::from_secs(3), tasks.join_next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let reply = f.reply().await;
    assert_eq!(
        reply["type"], "workload_rejected",
        "No initial lease probe was sent"
    );
    assert_eq!(reply["code"], "policy_denied");
    assert_eq!(f._service.read_config().unwrap()["listening"], Value::Null);
    assert_eq!(
        f.client.status.lock().unwrap()["last_workload"]["outcome_confirmed"],
        false
    );
    f.no_consumption();
}

#[cfg(all(windows, feature = "native-test-fixtures"))]
include!("client_workload_native_tests.rs");

#[cfg(all(windows, feature = "native-test-fixtures"))]
include!("client_camera_workload_native_tests.rs");

#[test]
fn cancelled_preparation_retains_actual_capacity_without_holding_runtime_shutdown() {
    let f = Fixture::new(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let service = f._service.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let (locked, ready) = tokio::sync::oneshot::channel();
    let owner = std::thread::spawn(move || {
        service
            .serialize_configuration(|| {
                let _ = locked.send(());
                assert!(wait.recv_timeout(Duration::from_secs(8)).is_ok());
                Ok(())
            })
            .unwrap();
    });
    runtime.block_on(ready).unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    runtime.block_on(async {
        f.client
            .dispatch_workload(
                f.offer(),
                &f.writer,
                &f.failed,
                &mut tasks,
                Some(&f.identity.fingerprint().unwrap()),
                Some(&f.definition.session_id),
                Arc::new(Requests::default()),
            )
            .await
            .unwrap();
        timeout(Duration::from_secs(2), async {
            while f
                .client
                .source_prepare_started
                .load(std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    });
    let start = Instant::now();
    drop(runtime);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(
        f.client
            .request_budget
            .admit("workload.session", "execute")
            .is_none(),
        "Actual native preparation retains execution after its async waiter/runtime are gone"
    );
    assert!(
        f.client
            .request_budget
            .admit("node.configure", "workload_policy_status")
            .is_some()
    );
    release.send(()).unwrap();
    owner.join().unwrap();
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    observer.block_on(async {
        timeout(Duration::from_secs(3), async {
            loop {
                if f.client
                    .request_budget
                    .admit("workload.session", "execute")
                    .is_some()
                {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    });
    f.no_consumption();
}
