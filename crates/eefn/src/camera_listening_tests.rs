use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    runtime: String,
    engine: Arc<crate::NodeEngine>,
    guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(saved_camera: bool, applied_camera: bool, microphone: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":"node","psk":"private","endpoints":[],
                "permissions":{"media":{"camera":saved_camera,"microphone":microphone}}}),
        )
        .unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let mut policy = crate::NodePolicy::default();
        policy.media.camera = applied_camera;
        policy.media.microphone = microphone;
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap(),
        );
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            runtime,
            engine,
            guard,
        }
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.dir.path().join("node.json")).unwrap()).unwrap()
    }
    fn change(&self, enabled: bool) -> Value {
        self.service
            .listening_command(self.set(enabled), false)
            .unwrap()
    }
    fn set(&self, enabled: bool) -> Request {
        let show = Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command: Command::Show {},
        };
        let current = self.service.listening_command(show.clone(), false).unwrap();
        Request {
            command: Command::Set {
                enabled,
                expected_revision: current["revision"].as_u64().unwrap(),
                expected_epoch: current["runtime_epoch"].as_u64().unwrap(),
                expected_camera_epoch: current
                    .pointer("/camera/runtime_epoch")
                    .and_then(Value::as_u64),
            },
            ..show
        }
    }
    async fn camera(&self, epoch: Option<u64>) -> Result<()> {
        self.service
            .supervise_camera_listening_epoch(&self.runtime, deadline(), epoch, async { Ok(()) })
            .await
    }
}
fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(3)
}
struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
async fn pending_owned(
    started: tokio::sync::oneshot::Sender<()>,
    dropped: Arc<AtomicBool>,
) -> Result<()> {
    let _owned = Dropped(dropped);
    started.send(()).unwrap();
    std::future::pending().await
}

#[tokio::test]
async fn camera_consent_uses_camera_permission_without_requiring_microphone_or_granting_work() {
    let f = Fixture::new(true, true, false);
    let report = f.change(true);
    assert_eq!(report["capture_started"], false);
    assert_eq!(report["execution_authorized"], false);
    assert_eq!(report["restart_requested"], false);
    f.camera(None).await.unwrap();
    assert!(
        f.service
            .supervise_listening(&f.runtime, deadline(), async { Ok(()) })
            .await
            .is_err()
    );
    assert!(
        !crate::workload_policy::from_config(&f.config())
            .unwrap()
            .enabled
    );
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
}

#[tokio::test]
async fn missing_camera_permission_consent_identity_or_deadline_refuses_before_polling() {
    // Saved and applied permissions are independent, and consent defaults off.
    for (saved, applied, enable, expired, stale_runtime) in [
        (false, true, true, false, false),
        (true, false, true, false, false),
        (true, true, false, false, false),
        (true, true, true, true, false),
        (true, true, true, false, true),
    ] {
        let f = Fixture::new(saved, applied, true);
        if enable {
            f.change(true);
        }
        let polled = AtomicBool::new(false);
        let runtime = if stale_runtime {
            uuid::Uuid::new_v4().to_string()
        } else {
            f.runtime.clone()
        };
        assert!(
            f.service
                .supervise_camera_listening_epoch(
                    &runtime,
                    if expired {
                        tokio::time::Instant::now()
                    } else {
                        deadline()
                    },
                    None,
                    async {
                        polled.store(true, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
                .is_err()
        );
        assert!(!polled.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn microphone_revocation_leaves_camera_running_but_camera_revocation_drops_its_owner() {
    let f = Fixture::new(true, true, true);
    f.change(true);
    let original_epoch = f.service.camera_listening_gate.snapshot().0;
    let dropped = Arc::new(AtomicBool::new(false));
    let (started, ready) = tokio::sync::oneshot::channel();
    let running = f.service.supervise_camera_listening_epoch(
        &f.runtime,
        deadline(),
        Some(original_epoch),
        pending_owned(started, dropped.clone()),
    );
    tokio::pin!(running);
    tokio::select! { result=ready=>result.unwrap(), _=&mut running=>panic!("Expected pending camera owner") };
    let mut config = f.config();
    config["permissions"]["media"]["microphone"] = json!(false);
    f.service.save_configuration(config).unwrap();
    assert!(f.service.listening_gate.snapshot().1);
    assert_eq!(
        f.service.camera_listening_gate.snapshot(),
        (original_epoch, false)
    );
    tokio::select! { biased; _=&mut running=>panic!("Microphone denial cannot stop camera consent"), _=std::future::ready(())=>{} };
    assert!(!dropped.load(Ordering::SeqCst));
    let mut config = f.config();
    config["permissions"]["media"]["camera"] = json!(false);
    f.service.save_configuration(config).unwrap();
    assert!(running.await.is_err());
    assert!(
        dropped.load(Ordering::SeqCst),
        "Owned operation drops before caller finalization"
    );
    let mut config = f.config();
    config["permissions"]["media"]["camera"] = json!(true);
    f.service.save_configuration(config).unwrap();
    assert!(
        f.camera(None).await.is_err(),
        "Generic permission restore cannot clear a denial"
    );
    f.change(true);
    assert!(
        f.camera(Some(original_epoch)).await.is_err(),
        "Owner enable cannot revive a prepared epoch"
    );
    f.camera(None).await.unwrap();
    assert_eq!(f.service.diagnostics()["success"], true);
}

#[tokio::test]
async fn failed_camera_permission_save_revokes_before_storage_and_never_replays_old_epoch() {
    let f = Fixture::new(true, true, true);
    f.change(true);
    let before = f.config();
    let stale_enable = f.set(true);
    let microphone_before = f.service.listening_gate.snapshot();
    let original_epoch = f.service.camera_listening_gate.snapshot().0;
    let dropped = Arc::new(AtomicBool::new(false));
    let (started, ready) = tokio::sync::oneshot::channel();
    let running = f.service.supervise_camera_listening_epoch(
        &f.runtime,
        deadline(),
        Some(original_epoch),
        pending_owned(started, dropped.clone()),
    );
    tokio::pin!(running);
    tokio::select! { result=ready=>result.unwrap(), _=&mut running=>panic!("Expected pending camera owner") };
    // Only the fixture backup is replaced, injecting failure after revocation.
    std::fs::remove_file(f.dir.path().join("node.json.bak")).unwrap();
    std::fs::create_dir(f.dir.path().join("node.json.bak")).unwrap();
    let mut next = before.clone();
    next["permissions"]["media"]["camera"] = json!(false);
    assert!(f.service.save_configuration(next).is_err());
    assert_eq!(f.config(), before);
    assert_eq!(f.service.listening_gate.snapshot(), microphone_before);
    assert!(
        f.service
            .listening_command(stale_enable.clone(), false)
            .is_err()
    );
    let mut legacy_enable = stale_enable;
    if let Command::Set {
        expected_camera_epoch,
        ..
    } = &mut legacy_enable.command
    {
        *expected_camera_epoch = None;
    }
    assert!(f.service.listening_command(legacy_enable, false).is_err());
    assert!(f.service.camera_listening_gate.snapshot().1);
    assert!(running.await.is_err());
    assert!(dropped.load(Ordering::SeqCst));
    assert!(
        !f.service.listening_gate.snapshot().1,
        "Camera denial does not revoke microphone consent"
    );
    assert!(f.camera(None).await.is_err());
    assert_eq!(f.change(true)["changed"], false);
    assert!(f.camera(Some(original_epoch)).await.is_err());
    f.camera(None).await.unwrap();
}

#[test]
fn camera_consent_confirmation_requires_its_reported_epoch_and_closed_status() {
    let f = Fixture::new(true, true, false);
    let request = f.set(true);
    let report = f.service.listening_command(request.clone(), false).unwrap();
    assert_eq!(report["schema_version"], 3);
    assert!(confirmed(&report, &request));
    for pointer in [
        "/camera/runtime_blocked",
        "/camera/runtime_epoch",
        "/camera",
    ] {
        let mut forged = report.clone();
        *forged.pointer_mut(pointer).unwrap() = match pointer {
            "/camera/runtime_blocked" => json!(true),
            "/camera/runtime_epoch" => {
                json!(report["camera"]["runtime_epoch"].as_u64().unwrap() + 2)
            }
            _ => Value::Null,
        };
        assert!(!confirmed(&forged, &request), "{pointer}");
    }
    let mut forged = report;
    forged["camera"]["execution_authorized"] = json!(true);
    assert!(!confirmed(&forged, &request));
}

#[tokio::test]
async fn local_listening_off_drops_both_input_owners_and_on_does_not_revive_either_epoch() {
    let f = Fixture::new(true, true, true);
    f.change(true);
    let camera_epoch = f.service.camera_listening_gate.snapshot().0;
    let microphone_epoch = f.service.listening_gate.snapshot().0;
    let camera_dropped = Arc::new(AtomicBool::new(false));
    let microphone_dropped = Arc::new(AtomicBool::new(false));
    let (camera_started, camera_ready) = tokio::sync::oneshot::channel();
    let (microphone_started, microphone_ready) = tokio::sync::oneshot::channel();
    let camera = f.service.supervise_camera_listening_epoch(
        &f.runtime,
        deadline(),
        Some(camera_epoch),
        pending_owned(camera_started, camera_dropped.clone()),
    );
    let microphone = f.service.supervise_listening_epoch(
        &f.runtime,
        deadline(),
        Some(microphone_epoch),
        pending_owned(microphone_started, microphone_dropped.clone()),
    );
    tokio::pin!(camera, microphone);
    tokio::select! {
        results=async { tokio::join!(camera_ready, microphone_ready) }=> { results.0.unwrap(); results.1.unwrap(); },
        _=&mut camera=>panic!("Expected pending camera owner"),
        _=&mut microphone=>panic!("Expected pending microphone owner"),
    }
    f.change(false);
    assert!(camera.await.is_err());
    assert!(microphone.await.is_err());
    assert!(camera_dropped.load(Ordering::SeqCst) && microphone_dropped.load(Ordering::SeqCst));
    f.change(true);
    assert!(f.camera(Some(camera_epoch)).await.is_err());
    assert!(
        f.service
            .supervise_listening_epoch(&f.runtime, deadline(), Some(microphone_epoch), async {
                Ok(())
            })
            .await
            .is_err()
    );
    f.camera(None).await.unwrap();
    f.service
        .supervise_listening(&f.runtime, deadline(), async { Ok(()) })
        .await
        .unwrap();
}

#[tokio::test]
async fn same_uuid_runtime_replacement_retires_camera_and_stale_guard_cannot_retire_replacement() {
    let f = Fixture::new(true, true, true);
    f.change(true);
    let original_epoch = f.service.camera_listening_gate.snapshot().0;
    let dropped = Arc::new(AtomicBool::new(false));
    let (started, ready) = tokio::sync::oneshot::channel();
    let running = f.service.supervise_camera_listening_epoch(
        &f.runtime,
        deadline(),
        Some(original_epoch),
        pending_owned(started, dropped.clone()),
    );
    tokio::pin!(running);
    tokio::select! { result=ready=>result.unwrap(), _=&mut running=>panic!("Expected pending camera owner") };
    let replacement = f
        .service
        .attach_preview_runtime(&f.engine, f.runtime.clone());
    assert!(running.await.is_err());
    assert!(dropped.load(Ordering::SeqCst));
    let current = f.service.camera_listening_gate.snapshot();
    assert!(current.0 > original_epoch && !current.1);
    drop(f.guard);
    assert_eq!(f.service.camera_listening_gate.snapshot(), current);
    // Check the actual binding, not only the epoch counter after stale Drop.
    f.service
        .supervise_camera_listening_epoch(&f.runtime, deadline(), Some(current.0), async { Ok(()) })
        .await
        .unwrap();
    drop(replacement);
    assert!(f.service.camera_listening_gate.snapshot().1);
    assert!(
        f.service
            .supervise_camera_listening_epoch(&f.runtime, deadline(), None, async { Ok(()) })
            .await
            .is_err()
    );
}
