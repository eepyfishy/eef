// Dispatch units use real encrypted framing on an in-memory stream. They do
// not substitute for the final credential/TLS connection acceptance suite.
use super::*;

#[test]
fn local_failure_status_retains_typed_reason_without_private_error_contexts() {
    let typed = anyhow::Error::new(crate::python::BridgeFailure::Deadline)
        .context("PRIVATE_DEVICE and private interpreter output");
    let generic = anyhow::anyhow!("PRIVATE_SETTINGS_PATH and private source binding");
    let owner = anyhow::Error::new(crate::grants::GrantDenied("grant_owner_limits_exceeded"))
        .context("PRIVATE_JOURNAL_PATH and private source binding");
    let unknown = anyhow::Error::new(crate::grants::GrantDenied("PRIVATE_UNKNOWN_DENIAL"));
    let quota = anyhow::Error::new(crate::grants::GrantDenied("grant_receipt_quota_full"))
        .context("PRIVATE_JOURNAL_PATH and private receipt details");
    for (error, reason) in [
        (typed, "python_exchange_deadline"),
        (generic, "workload_unconfirmed"),
        (owner, "grant_owner_limits_exceeded"),
        (unknown, "workload_unconfirmed"),
        (quota, "grant_receipt_quota_full"),
    ] {
        let status = unconfirmed_status("grant", "source_unconfirmed", &error);
        assert_eq!(status["reason_code"], reason);
        assert_eq!(status["outcome_confirmed"], false);
        assert_eq!(status["state"], "unconfirmed");
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains("PRIVATE_") && !encoded.contains("private"));
    }
}
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

#[tokio::test]
async fn retired_source_cancels_pending_renewal_and_preserves_the_exact_final_save_waiter() {
    for save_succeeds in [true, false] {
        let mut f = Fixture::new(true);
        let d = f.definition.clone();
        let contract = d.lease_contract().unwrap();
        let requests = Arc::new(Requests::default());
        let output = Output {
            crypto: f.client.crypto.clone(),
            writer: f.writer.clone(),
            failed: f.failed.clone(),
        };
        let (sender, receiver) = crate::workload_leases::renewal_channel();
        let (retire, retirement) = tokio::sync::oneshot::channel();
        let (save, checkpoint) = tokio::sync::oneshot::channel();
        let (pump_done, pump_retired) = tokio::sync::oneshot::channel();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let source_calls = calls.clone();
        let source_finished = finished.clone();
        let source = async move {
            source_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            retirement.await.unwrap();
            // The real live supervisor owns this receiver. Its retirement
            // precedes the separately bounded historical persistence waiter.
            drop(receiver);
            checkpoint.await.unwrap();
            source_finished.store(true, std::sync::atomic::Ordering::SeqCst);
            if save_succeeds {
                Ok(17)
            } else {
                Err(anyhow::anyhow!("Synthetic final journal save refused"))
            }
        };
        let renewal = async {
            let outcome = renew(
                &d,
                &contract,
                requests.clone(),
                &output,
                sender,
                Instant::now() + Duration::from_secs(6),
                Instant::now() + Duration::from_secs(40),
            )
            .await;
            assert!(
                outcome.is_ok(),
                "Closed source receiver is not failed source execution"
            );
            pump_done.send(()).unwrap();
            outcome
        };
        let (result, ()) = tokio::join!(await_source(source, renewal), async {
            let frame = timeout(
                Duration::from_secs(5),
                read_message(&f.client.crypto, &mut f.peer),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(frame["type"], "workload_probe");
            let query: crate::workload_leases::Query =
                serde_json::from_value(frame["query"].clone()).unwrap();
            assert_eq!(query.sequence, 2);
            retire.send(()).unwrap();
            timeout(Duration::from_secs(3), pump_retired)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert!(!finished.load(std::sync::atomic::Ordering::SeqCst));
            // A late correlated reply cannot restart the retired pump or turn
            // its metadata into fresh source/reporting authority.
            requests.complete(json!(crate::workload_leases::Reply {
                kind: "workload_lease".into(),
                schema_version: 2,
                id: query.id,
                contract_sha256: query.contract.digest().unwrap(),
                sequence: query.sequence,
                lease_remaining_ms: 1000,
                session_remaining_ms: 30000,
            }));
            save.send(()).unwrap();
        });
        assert!(finished.load(std::sync::atomic::Ordering::SeqCst));
        if save_succeeds {
            assert_eq!(result.unwrap(), 17)
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("final journal save refused")
            );
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        f.no_consumption();
    }
}

#[tokio::test]
async fn already_retired_source_stops_renewal_without_a_probe_even_after_the_old_deadline() {
    use futures_util::FutureExt;
    let mut f = Fixture::new(true);
    let d = f.definition.clone();
    let output = Output {
        crypto: f.client.crypto.clone(),
        writer: f.writer.clone(),
        failed: f.failed.clone(),
    };
    let (sender, receiver) = crate::workload_leases::renewal_channel();
    drop(receiver);
    timeout(
        Duration::from_secs(1),
        renew(
            &d,
            &d.lease_contract().unwrap(),
            Arc::new(Requests::default()),
            &output,
            sender,
            Instant::now(),
            Instant::now(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        read_message(&f.client.crypto, &mut f.peer)
            .now_or_never()
            .is_none()
    );
    f.no_consumption();
}

#[tokio::test]
async fn live_source_renewal_expiry_still_cancels_source_instead_of_waiting_for_success() {
    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let f = Fixture::new(true);
    let d = f.definition.clone();
    let output = Output {
        crypto: f.client.crypto.clone(),
        writer: f.writer.clone(),
        failed: f.failed.clone(),
    };
    let (sender, receiver) = crate::workload_leases::renewal_channel();
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    let source = async move {
        let _live = (receiver, guard);
        std::future::pending::<Result<()>>().await
    };
    let error = await_source(
        source,
        renew(
            &d,
            &d.lease_contract().unwrap(),
            Arc::new(Requests::default()),
            &output,
            sender,
            Instant::now(),
            Instant::now() + Duration::from_secs(10),
        ),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Original workload renewal window expired")
    );
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    f.no_consumption();
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
