use super::*;
use crate::workload_leases::definition::SourceKind;
use serde_json::json;

struct Fixture {
    _dir: tempfile::TempDir,
    service: Arc<NodeService>,
    runtime: String,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let metadata:crate::context::NodeMetadata=serde_json::from_value(json!({"resources":[
            {"id":"mic","capability":"audio.capture"},{"id":"camera","capability":"camera.capture","parameters":{"device":0}}]})).unwrap();
        crate::setup::write_json(&path,&json!({"node_id":"node","metadata":metadata,
            "permissions":{"media":{"microphone":true,"camera":true}},"listening":{"enabled":true},"workload_policy":{"enabled":true}})).unwrap();
        let service = NodeService::new(path, "node".into());
        let mut policy = crate::NodePolicy::default();
        policy.media.microphone = true;
        policy.media.camera = true;
        let python =
            crate::python::PythonRuntime::discover(Some(dir.path().join("never-run-python.exe")))
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
        Self {
            _dir: dir,
            service,
            runtime,
            _engine: engine,
            _guard: guard,
        }
    }
    async fn inspect(&self) -> Snapshot {
        self.service
            .clone()
            .inspect_source_readiness(self.runtime.clone())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn coalesced_snapshot_reports_each_source_and_camera_survives_microphone_permission_withdrawal()
 {
    let f = Fixture::new();
    let original = f.inspect().await;
    assert_eq!(original.report.execution_available, cfg!(windows));
    assert_eq!(original.camera_report.execution_available, cfg!(windows));
    let expected = if cfg!(windows) {
        vec![SourceKind::MicrophoneEnergy, SourceKind::CameraMotion]
    } else {
        vec![]
    };
    assert_eq!(original.supported_sources(&f.service, &f.runtime), expected);
    let mut config = f.service.read_config().unwrap();
    config["permissions"]["media"]["microphone"] = json!(false);
    f.service.save_configuration(config).unwrap();
    assert!(
        original
            .supported_sources(&f.service, &f.runtime)
            .is_empty()
    );
    let camera = f.inspect().await;
    assert!(!camera.report.execution_available);
    assert_eq!(camera.camera_report.execution_available, cfg!(windows));
    assert_eq!(
        camera.supported_sources(&f.service, &f.runtime),
        if cfg!(windows) {
            vec![SourceKind::CameraMotion]
        } else {
            vec![]
        }
    );
    let mut config = f.service.read_config().unwrap();
    config["permissions"]["media"]["camera"] = json!(false);
    f.service.save_configuration(config).unwrap();
    assert!(camera.supported_sources(&f.service, &f.runtime).is_empty());
    assert!(
        f.inspect()
            .await
            .supported_sources(&f.service, &f.runtime)
            .is_empty()
    );
}

#[tokio::test]
async fn readiness_rejects_retired_camera_epochs_and_mixed_runtime_reports_before_advertising_any_kind()
 {
    let f = Fixture::new();
    let mut snapshot = f.inspect().await;
    snapshot.camera_report.runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(
        snapshot
            .supported_sources(&f.service, &f.runtime)
            .is_empty()
    );
    let snapshot = f.inspect().await;
    f.service.camera_listening_gate.retire_runtime();
    assert!(
        snapshot
            .supported_sources(&f.service, &f.runtime)
            .is_empty()
    );
    let refreshed = f.inspect().await;
    assert!(!refreshed.camera_report.execution_available);
    assert_eq!(refreshed.report.execution_available, cfg!(windows));
}
