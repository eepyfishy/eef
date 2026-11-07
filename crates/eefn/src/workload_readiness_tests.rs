use super::*;
use crate::{NodeEngine, NodePolicy};
use serde_json::json;

struct Fixture {
    _dir: tempfile::TempDir,
    service: Arc<NodeService>,
    _engine: Arc<NodeEngine>,
    runtime: String,
    guard: Option<crate::request_preview::PreviewGuard>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let metadata: crate::context::NodeMetadata = serde_json::from_value(
            json!({"resources":[{"id":"mic","capability":"audio.capture"}]}),
        )
        .unwrap();
        crate::setup::write_json(&path,&json!({"node_id":"node","metadata":metadata,"permissions":{"media":{"microphone":true}},
            "listening":{"enabled":true},"workload_policy":{"enabled":true}})).unwrap();
        let service = NodeService::new(path, "node".into());
        let mut policy = NodePolicy::default();
        policy.media.microphone = true;
        // Inspection only: a deliberately missing configured interpreter cannot
        // launch anything. Actual source execution has opt-in native tests.
        let python =
            crate::python::PythonRuntime::discover(Some(dir.path().join("not-run-python.exe")))
                .unwrap();
        let engine = Arc::new(
            NodeEngine::new(false, vec![], dir.path().into())
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
        Self {
            _dir: dir,
            service,
            _engine: engine,
            runtime,
            guard: Some(guard),
        }
    }
}
#[tokio::test]
async fn passive_source_readiness_reflects_platform_and_rejects_revoked_or_retired_versions() {
    let mut f = Fixture::new();
    let snapshot = f
        .service
        .clone()
        .inspect_source_readiness(f.runtime.clone())
        .await
        .unwrap();
    let request = crate::listening::Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: f.runtime.clone(),
        command: crate::listening::Command::Show {},
    };
    let consent = f.service.listening_command(request.clone(), false).unwrap();
    assert!(crate::listening::confirmed(&consent, &request));
    assert_eq!(consent["capture_integration_available"], cfg!(windows));
    let mut forged = consent.clone();
    forged["schema_version"] = json!(1);
    forged["capture_integration_available"] = json!(true);
    assert!(!crate::listening::confirmed(&forged, &request));
    assert!(snapshot.current(&f.service, &f.runtime));
    assert_eq!(snapshot.report.execution_available, cfg!(windows));
    assert!(!snapshot.report.capture_started && !snapshot.report.execution_authorized);
    f.service.listening_gate.deny();
    assert!(!snapshot.current(&f.service, &f.runtime));
    let revoked = f
        .service
        .clone()
        .inspect_source_readiness(f.runtime.clone())
        .await
        .unwrap();
    assert!(!revoked.report.execution_available);
    drop(f.guard.take());
    assert!(!revoked.current(&f.service, &f.runtime));
    assert!(
        f.service
            .clone()
            .inspect_source_readiness(f.runtime.clone())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn cancelled_passive_inspection_keeps_native_capacity_until_actual_storage_exits() {
    let mut f = Fixture::new();
    let service = f.service.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let (locked, ready) = tokio::sync::oneshot::channel();
    let blocker = std::thread::spawn(move || {
        service
            .inspect_grant_configuration(|_| {
                let _ = locked.send(());
                assert!(
                    wait.recv_timeout(std::time::Duration::from_secs(10))
                        .is_ok()
                );
                Ok(())
            })
            .unwrap()
    });
    ready.await.unwrap();
    let mut pending = Box::pin(
        f.service
            .clone()
            .inspect_source_readiness(f.runtime.clone()),
    );
    tokio::select! {biased;
        result=&mut pending=>panic!("Blocked passive read returned: {}",result.is_ok()),
        _=tokio::time::sleep(std::time::Duration::from_millis(20))=>{}
    }
    assert_eq!(f.service.source_readiness.capacity.available_permits(), 0);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f
            .service
            .source_readiness
            .inspecting
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(pending);
    assert_eq!(f.service.source_readiness.capacity.available_permits(), 0);
    let retired = std::time::Instant::now();
    drop(f.guard.take());
    assert!(
        retired.elapsed() < std::time::Duration::from_secs(1),
        "Runtime retirement must not join passive storage"
    );
    release.send(()).unwrap();
    blocker.join().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f.service.source_readiness.capacity.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.guard = Some(
        f.service
            .attach_preview_runtime(&f._engine, f.runtime.clone()),
    );
    assert!(
        f.service
            .clone()
            .inspect_source_readiness(f.runtime.clone())
            .await
            .unwrap()
            .current(&f.service, &f.runtime)
    );
}
#[test]
fn source_config_invalidation_tracks_source_inputs_without_following_unrelated_settings() {
    let old = json!({"permissions":{"media":{"microphone":true},"http":{"enabled":false}},"name":"node","workload_policy":{"enabled":true}});
    let mut unrelated = old.clone();
    unrelated["name"] = json!("Friendly node");
    unrelated["permissions"]["http"]["enabled"] = json!(true);
    assert!(!source_config_changed(&old, &unrelated));
    unrelated["permissions"]["media"]["microphone"] = json!(false);
    assert!(source_config_changed(&old, &unrelated));
    let mut limits = old.clone();
    limits["workload_policy"]["max_lease_ms"] = json!(1000);
    assert!(source_config_changed(&old, &limits));
}

#[tokio::test]
async fn camera_permission_changes_and_camera_epoch_retire_passive_readiness_without_starting_work()
{
    let f = Fixture::new();
    let original = f
        .service
        .clone()
        .inspect_source_readiness(f.runtime.clone())
        .await
        .unwrap();
    let changes = f.service.source_readiness.subscribe();
    let mut unrelated = f.service.read_config().unwrap();
    unrelated["name"] = json!("Friendly node");
    f.service.save_configuration(unrelated).unwrap();
    assert!(!original.current(&f.service, &f.runtime));
    assert!(
        changes.has_changed().unwrap(),
        "Initial camera denial must wake readiness, including unrelated saves"
    );
    let refreshed = f
        .service
        .clone()
        .inspect_source_readiness(f.runtime.clone())
        .await
        .unwrap();
    assert!(refreshed.current(&f.service, &f.runtime));
    assert_eq!(
        refreshed.report.execution_available,
        cfg!(windows),
        "Camera denial cannot remove microphone readiness after refresh"
    );
    let mut config = f.service.read_config().unwrap();
    config["permissions"]["media"]["camera"] = json!(true);
    f.service.save_configuration(config).unwrap();
    assert!(!refreshed.current(&f.service, &f.runtime));
    let current = f
        .service
        .clone()
        .inspect_source_readiness(f.runtime.clone())
        .await
        .unwrap();
    assert!(current.current(&f.service, &f.runtime));
    f.service.camera_listening_gate.retire_runtime();
    assert!(!current.current(&f.service, &f.runtime));
    assert_eq!(f.service.diagnostics()["success"], true);
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
}
