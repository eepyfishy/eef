use super::*;
use crate::workload_leases::{IssuerWindow, Policy, Query, Requests, definition::*};

pub(crate) struct Fixture {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) service: Arc<NodeService>,
    pub(crate) runtime: String,
    pub(crate) definition: Definition,
    pub(crate) engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(local_memory: Option<u64>) -> Self {
        Self::with_python(local_memory, None, true)
    }
    fn with_python(local_memory: Option<u64>, python: Option<PythonRuntime>, camera: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let resource = crate::workload_leases::definition::camera_tests::resource();
        let metadata = crate::context::NodeMetadata {
            resources: vec![resource.clone()],
            ..Default::default()
        };
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path, &json!({"node_id":"node","psk":"PRIVATE_CREDENTIAL",
            "metadata":metadata,"permissions":{"media":{"camera":true,"microphone":false}},"listening":{"enabled":true}})).unwrap();
        let service = NodeService::new(path, "node".into());
        let mut policy = crate::NodePolicy::default();
        policy.media.camera = camera;
        let mut limits = crate::python::BridgeLimits::default();
        limits.max_job_memory_mib = local_memory;
        let python = python
            .unwrap_or_else(|| {
                PythonRuntime::discover(Some(dir.path().join("never-started-python.exe"))).unwrap()
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
        let mut definition = crate::workload_leases::definition::camera_tests::camera();
        definition.executor_node = "node".into();
        if let Source::CameraMotion {
            resource_id,
            parameters,
            ..
        } = &mut definition.source
        {
            *resource_id = "node::camera".into();
            parameters.max_frames = 5;
        }
        definition.limits = Limits {
            max_session_ms: 60000,
            max_lease_ms: 10000,
            max_job_memory_mib: 128,
            max_source_frames: 5,
        };
        definition.events = Events {
            mode: EventMode::ActivityTransitions,
            min_interval_ms: 100,
            max_events: 100,
            max_event_bytes: 1024,
            max_total_bytes: 65536,
            max_pending: 4,
            overflow: Overflow::Stop,
        };
        definition.validate().unwrap();
        Self {
            dir,
            service,
            runtime,
            definition,
            engine,
            _guard: guard,
        }
    }
    fn prepare(&self) -> Result<PreparedSource> {
        self.service
            .prepare_camera(&self.runtime, self.definition.clone())
    }
    fn config(&self) -> Value {
        self.service.read_config().unwrap()
    }
    fn save(&self, change: impl FnOnce(&mut Value)) {
        let mut config = self.config();
        change(&mut config);
        self.service.save_configuration(config).unwrap();
    }
    fn listening(&self, enabled: bool) {
        let request = crate::listening::Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command: crate::listening::Command::Show {},
        };
        let current = self
            .service
            .listening_command(request.clone(), false)
            .unwrap();
        self.service
            .listening_command(
                crate::listening::Request {
                    command: crate::listening::Command::Set {
                        enabled,
                        expected_revision: current["revision"].as_u64().unwrap(),
                        expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                        expected_camera_epoch: current
                            .pointer("/camera/runtime_epoch")
                            .and_then(Value::as_u64),
                    },
                    ..request
                },
                false,
            )
            .unwrap();
    }
}
async fn consume(
    f: &Fixture,
    definition: Definition,
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
    let contract = definition.lease_contract().unwrap();
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
                .reply(
                    &query,
                    &definition.issuer_id,
                    &definition.session_id,
                    &definition.executor_node,
                )
                .unwrap();
            manager.complete(json!(reply));
            async { Ok(()) }
        })
        .await
        .unwrap();
    let token = ledger
        .consume_workload(
            definition.clone(),
            &policy,
            proof,
            &definition.issuer_id,
            &definition.session_id,
            &definition.executor_node,
            tokio::time::Instant::now() + std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();
    (ledger, token)
}
async fn unknown(f: &Fixture, ledger: &crate::grants::GrantLedger, definition: &Definition) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if ledger.workload_reporting_ready()
                && ledger
                    .workload_receipt(&definition.issuer_id, &definition.grant_id)
                    .unwrap()
                    .receipt
                    .state
                    == "unknown"
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let retained = ledger
        .workload_receipt(&definition.issuer_id, &definition.grant_id)
        .unwrap();
    assert_eq!(retained.definition, *definition);
    assert!(retained.completion.is_none());
    assert!(!retained.execution_authorized && !retained.restart_authorized);
    assert!(f.service.microphone_pool.reserve().is_ok());
}

#[test]
fn camera_preparation_pins_source_and_private_binding_with_only_its_own_reservation() {
    let f = Fixture::new(None);
    let before = f.config();
    let source = f.prepare().unwrap();
    assert_eq!(
        source.source.digest(),
        crate::workload_sources::camera_adapter_digest()
    );
    assert_eq!(
        source.params["device"],
        "rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live"
    );
    assert_eq!(
        (
            source.bounds.max_observations,
            source.bounds.max_source_frames
        ),
        (5, 5)
    );
    assert!(f.service.camera_pool.reserve().is_err());
    assert!(f.service.microphone_pool.reserve().is_ok());
    assert_eq!(f.config(), before);
    assert!(!f.dir.path().join("pids.json").exists());
    drop(source);
    assert!(f.service.camera_pool.reserve().is_ok());
}

#[test]
fn network_preparation_selects_camera_only_after_the_existing_workload_policy_permit() {
    let f = Fixture::new(None);
    assert!(
        f.engine
            .prepare_workload(&f.runtime, f.definition.clone())
            .is_err()
    );
    assert!(f.service.camera_pool.reserve().is_ok());
    let request = crate::workload_policy::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: f.runtime.clone(),
        command: crate::workload_policy::Command::Show {},
    };
    let current = f
        .service
        .workload_policy_command(request.clone(), false)
        .unwrap();
    f.service
        .workload_policy_command(
            crate::workload_policy::Request {
                command: crate::workload_policy::Command::Set {
                    policy: Policy {
                        enabled: true,
                        ..Default::default()
                    },
                    expected_revision: current["revision"].as_u64().unwrap(),
                    expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                },
                ..request
            },
            false,
        )
        .unwrap();
    let (_policy, _permit, source) = f
        .engine
        .prepare_workload(&f.runtime, f.definition.clone())
        .unwrap();
    assert_eq!(
        source.source.digest(),
        crate::workload_sources::camera_adapter_digest()
    );
    assert!(f.service.camera_pool.reserve().is_err());
    assert!(f.service.microphone_pool.reserve().is_ok());
    drop(source);
    assert!(f.service.camera_pool.reserve().is_ok());
}
#[test]
fn camera_preparation_refuses_microphone_kind_and_changed_reviewed_source_before_reservation() {
    let f = Fixture::new(None);
    let microphone = crate::grants::workload::tests::definition();
    assert!(
        f.service
            .prepare_camera(&f.runtime, microphone)
            .err()
            .unwrap()
            .to_string()
            .contains("Microphone source execution")
    );
    for field in ["resource_sha256", "adapter_sha256"] {
        let mut changed = json!(f.definition);
        changed["source"][field] = json!("f".repeat(64));
        assert!(
            f.service
                .prepare_camera(&f.runtime, serde_json::from_value(changed).unwrap())
                .is_err()
        );
        assert!(f.service.camera_pool.reserve().is_ok());
    }
    assert!(
        f.service
            .prepare_microphone(&f.runtime, f.definition.clone())
            .is_err()
    );
}
#[test]
fn camera_preparation_requires_saved_applied_permission_consent_and_current_binding() {
    let f = Fixture::with_python(None, None, false);
    assert!(f.prepare().is_err());
    for fault in ["permission", "consent", "binding", "runtime"] {
        let f = Fixture::new(None);
        match fault {
            "permission" => f.save(|c| c["permissions"]["media"]["camera"] = json!(false)),
            "consent" => f.listening(false),
            "binding" => {
                f.save(|c| c["metadata"]["resources"][0]["parameters"]["device"] = json!(1))
            }
            _ => {
                f.service.live.lock().unwrap()["runtime_id"] =
                    json!(uuid::Uuid::new_v4().to_string())
            }
        }
        assert!(f.prepare().is_err(), "{fault}");
        assert!(f.service.camera_pool.reserve().is_ok());
    }
}
#[test]
fn camera_preparation_intersects_memory_without_extending_time_or_frame_limits() {
    let f = Fixture::new(Some(96));
    let source = f.prepare().unwrap();
    assert_eq!(source.python.limits().max_job_memory_mib, Some(96));
    assert_eq!(source.definition.limits.max_job_memory_mib, 128);
    assert_eq!(source.python.limits().timeout_seconds, 30);
    assert_eq!(source.params["max_duration_ms"], 10000);
    assert_eq!(source.bounds.max_source_frames, 5);
}
#[tokio::test]
async fn old_camera_preparation_cannot_launch_after_epoch_replacement_or_changed_consumption() {
    for fault in ["consent", "runtime", "definition"] {
        let f = Fixture::new(None);
        let source = f.prepare().unwrap();
        let mut definition = f.definition.clone();
        if fault == "definition" {
            definition.plan_sha256 = "e".repeat(64);
        }
        let (ledger, consumed) = consume(&f, definition.clone()).await;
        let mut replacement = None;
        if fault == "consent" {
            f.listening(false);
            f.listening(true);
        }
        if fault == "runtime" {
            replacement = Some(
                f.service
                    .attach_preview_runtime(&f.engine, f.runtime.clone()),
            );
        }
        let (_sender, renewals) = crate::workload_leases::renewal_channel();
        let error = source
            .run(consumed, renewals, |_| async {
                panic!("Unstarted camera cannot emit events");
                #[allow(unreachable_code)]
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(if fault == "definition" {
                "differs"
            } else {
                "epoch"
            }),
            "{fault}: {error:#}"
        );
        assert!(f.service.camera_pool.reserve().is_ok());
        unknown(&f, &ledger, &definition).await;
        drop(replacement);
    }
}
#[tokio::test]
async fn unpolled_camera_runner_is_bounded_and_drop_retains_uncertainty_without_capture() {
    let f = Fixture::new(None);
    let source = f.prepare().unwrap();
    let (ledger, consumed) = consume(&f, f.definition.clone()).await;
    let (_sender, renewals) = crate::workload_leases::renewal_channel();
    let run = source.run(consumed, renewals, |_| std::future::ready(Ok(())));
    assert!(
        std::mem::size_of_val(&run) <= 64 * 1024,
        "Prepared source embeds oversized transport/control state"
    );
    drop(run);
    assert!(f.service.camera_pool.reserve().is_ok());
    unknown(&f, &ledger, &f.definition).await;
}

#[cfg(feature = "native-test-fixtures")]
include!("workload_camera_runtime_native_tests.rs");
