use super::*;
use crate::workload_leases::{IssuerWindow, Policy, Query, Requests, definition::*};

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<NodeService>,
    runtime: String,
    definition: Definition,
    engine: Arc<crate::NodeEngine>,
    guard: Option<crate::request_preview::PreviewGuard>,
}
impl Fixture {
    fn new(local_memory: Option<u64>) -> Self {
        Self::with_python(local_memory, None)
    }
    fn with_python(local_memory: Option<u64>, python: Option<PythonRuntime>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let resource: crate::context::Resource = serde_json::from_value(json!({"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_DEVICE"}})).unwrap();
        let metadata = crate::context::NodeMetadata {
            resources: vec![resource.clone()],
            ..Default::default()
        };
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"node","psk":"PRIVATE_CREDENTIAL","metadata":metadata,"permissions":{"media":{"microphone":true}},"listening":{"enabled":true}})).unwrap();
        let service = NodeService::new(path, "node".into());
        let mut policy = crate::NodePolicy::default();
        policy.media.microphone = true;
        // Default refusal/preparation fixtures use a deliberately missing
        // interpreter. Explicit native tests supply their pinned test runtime.
        let mut limits = crate::python::BridgeLimits::default();
        limits.max_job_memory_mib = local_memory;
        let python = python
            .unwrap_or_else(|| {
                PythonRuntime::discover(Some(dir.path().join("not-executed-python.exe"))).unwrap()
            })
            .with_limits(limits)
            .unwrap();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap()
                .with_service(service.clone())
                .with_python_runtime(Arc::new(python)),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let definition = Definition {
            schema_version: 2,
            grant_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            session_id: uuid::Uuid::new_v4().to_string(),
            executor_node: "node".into(),
            job_id: "b".repeat(32),
            task_id: "c".repeat(32),
            attempt: 1,
            plan_sha256: "d".repeat(64),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                "origin".into(),
                vec!["home".into()],
            )
            .unwrap(),
            priority: 0,
            source: Source::MicrophoneEnergy {
                resource_id: "node::mic".into(),
                resource_sha256: microphone_resource_digest(&resource).unwrap(),
                adapter_sha256: crate::workload_sources::microphone_adapter_digest(),
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
        };
        Self {
            dir,
            service,
            runtime,
            definition,
            engine,
            guard: Some(guard),
        }
    }
    fn prepare(&self) -> Result<PreparedMicrophone> {
        self.service
            .prepare_microphone(&self.runtime, self.definition.clone())
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.service.config_path).unwrap()).unwrap()
    }
    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let mut config = self.config();
        change(&mut config);
        crate::setup::write_json(&self.service.config_path, &config).unwrap();
    }
    fn listening(&self, enabled: bool) {
        let shown = self
            .service
            .listening_command(
                crate::listening::Request {
                    schema_version: 1,
                    expected_node_id: "node".into(),
                    expected_runtime_id: self.runtime.clone(),
                    command: crate::listening::Command::Show {},
                },
                false,
            )
            .unwrap();
        self.service
            .listening_command(
                crate::listening::Request {
                    schema_version: 1,
                    expected_node_id: "node".into(),
                    expected_runtime_id: self.runtime.clone(),
                    command: crate::listening::Command::Set {
                        enabled,
                        expected_revision: shown["revision"].as_u64().unwrap(),
                        expected_epoch: shown["runtime_epoch"].as_u64().unwrap(),
                    },
                },
                false,
            )
            .unwrap();
    }
}

// These opt-in tests use the same private preparation helpers and real host/job
// path. Default/local tests require no Python, capture hardware or dependency.
#[cfg(feature = "native-test-fixtures")]
include!("workload_runtime_native_tests.rs");
async fn consume(
    f: &Fixture,
    d: Definition,
) -> (
    Arc<crate::grants::GrantLedger>,
    crate::grants::workload::Consumption,
) {
    let ledger = crate::grants::GrantLedger::open(
        &f.dir.path().join("receipts.db"),
        Default::default(),
        "node",
    )
    .unwrap();
    let policy = Policy {
        enabled: true,
        ..Default::default()
    };
    let contract = d.lease_contract().unwrap();
    let mut issuer = IssuerWindow::new(
        contract.clone(),
        &policy,
        std::time::Instant::now() + std::time::Duration::from_secs(50),
    )
    .unwrap();
    let requests = Arc::new(Requests::default());
    let manager = requests.clone();
    let proof = requests
        .request(&contract, 1, |value| {
            let query: Query = serde_json::from_value(value).unwrap();
            let reply = issuer
                .reply(&query, &d.issuer_id, &d.session_id, &d.executor_node)
                .unwrap();
            manager.complete(json!(reply));
            async { Ok(()) }
        })
        .await
        .unwrap();
    let token = ledger
        .consume_workload(
            d.clone(),
            &policy,
            proof,
            &d.issuer_id,
            &d.session_id,
            &d.executor_node,
        )
        .unwrap();
    (ledger, token)
}

#[test]
fn preparation_owns_compiled_bytes_and_private_binding_without_opening_a_source() {
    let f = Fixture::new(None);
    let before = f.config();
    let source = f.prepare().unwrap();
    assert_eq!(
        source.source.digest(),
        crate::workload_sources::microphone_adapter_digest()
    );
    assert_eq!(source.params["device"], "PRIVATE_DEVICE");
    assert_eq!(source.bounds.max_source_frames, 8000);
    assert!(f.service.microphone_pool.reserve().is_err());
    assert!(
        f.engine
            .microphone_runtime("node", &f.service.microphone_pool)
            .is_ok()
    );
    assert_eq!(before, f.config());
    drop(source);
    assert!(f.service.microphone_pool.reserve().is_ok());
}
#[test]
fn preparation_rejects_changed_source_or_binding_without_reserving_it() {
    let f = Fixture::new(None);
    for field in ["adapter_sha256", "resource_sha256"] {
        let mut value = json!(f.definition);
        value["source"][field] = json!("0".repeat(64));
        let changed = serde_json::from_value(value).unwrap();
        assert!(f.service.prepare_microphone(&f.runtime, changed).is_err());
        assert!(f.service.microphone_pool.reserve().is_ok());
    }
}
#[test]
fn preparation_requires_both_consent_and_microphone_permission() {
    let f = Fixture::new(None);
    f.listening(false);
    assert!(f.prepare().is_err());
    f.listening(true);
    f.edit(|c| c["permissions"]["media"]["microphone"] = json!(false));
    assert!(f.prepare().is_err());
    assert!(f.service.microphone_pool.reserve().is_ok());
}
#[test]
fn preparation_refuses_pending_resource_edits() {
    let f = Fixture::new(None);
    f.edit(|c| c["metadata"]["resources"][0]["parameters"]["device"] = json!("changed"));
    assert!(f.prepare().is_err());
    assert!(f.service.microphone_pool.reserve().is_ok());
}
#[test]
fn retiring_runtime_invalidates_old_preparation_generation() {
    let mut f = Fixture::new(None);
    let before = f.service.listening_gate.snapshot();
    drop(f.guard.take());
    assert!(f.service.listening_gate.snapshot().0 > before.0);
    assert!(f.prepare().is_err());
}
#[test]
fn prepared_job_memory_tightens_local_and_reviewed_bounds() {
    let f = Fixture::new(Some(64));
    let source = f.prepare().unwrap();
    assert_eq!(source.python.limits().max_job_memory_mib, Some(64));
    assert_eq!(source.definition.limits.max_job_memory_mib, 128);
    assert_eq!(
        source.python.limits().timeout_seconds,
        30,
        "ordinary duration ceiling remains"
    );
}
#[tokio::test]
async fn prepared_source_cannot_accept_a_different_consumed_definition() {
    let f = Fixture::new(None);
    let source = f.prepare().unwrap();
    let mut changed = f.definition.clone();
    changed.plan_sha256 = "e".repeat(64);
    let (ledger, consumed) = consume(&f, changed.clone()).await;
    let (_tx, renewals) = crate::workload_leases::renewal_channel();
    let error = source
        .run(consumed, renewals, |_| async { Ok(()) })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("differs"));
    assert_eq!(
        ledger
            .workload_receipt(&changed.issuer_id, &changed.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(f.service.microphone_pool.reserve().is_ok());
}
#[tokio::test]
async fn listening_off_on_cannot_launch_an_old_prepared_source() {
    let f = Fixture::new(None);
    let source = f.prepare().unwrap();
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    f.listening(false);
    f.listening(true);
    let (_tx, renewals) = crate::workload_leases::renewal_channel();
    let error = source
        .run(consumed, renewals, |_| async { Ok(()) })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("epoch"));
    assert_eq!(
        ledger
            .workload_receipt(&f.definition.issuer_id, &f.definition.grant_id)
            .unwrap()
            .receipt
            .state,
        "unknown"
    );
    assert!(f.service.microphone_pool.reserve().is_ok());
}
