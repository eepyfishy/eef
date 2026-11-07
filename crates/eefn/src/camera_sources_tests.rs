use super::*;
use std::sync::Arc;

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    runtime: String,
    engine: Arc<crate::NodeEngine>,
    guard: Option<crate::request_preview::PreviewGuard>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let metadata: crate::context::NodeMetadata = serde_json::from_value(json!({
            "extensions":{"private":"PRIVATE_METADATA"},"resources":[
                {"id":"camera","name":"Local camera","capability":"camera.capture","parameters":{"device":0}},
                {"id":"netcam","name":"Network camera","capability":"camera.capture","parameters":{"device":"rtsp://owner:PRIVATE_PASSWORD@camera.invalid:554/live"}},
                {"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_MIC"}}
            ]})).unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path, &json!({"node_id":"node","psk":"PRIVATE_CREDENTIAL",
            "metadata":metadata,"permissions":{"media":{"camera":true,"microphone":false}},"listening":{"enabled":true}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let mut policy = crate::NodePolicy::default();
        policy.media.camera = true;
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap()
                .with_service(service.clone()),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            runtime,
            engine,
            guard: Some(guard),
        }
    }
    fn request(&self) -> Request {
        Request {
            schema_version: 2,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
        }
    }
    fn inspect(&self) -> Report {
        serde_json::from_value(
            self.service
                .camera_workload_sources(self.request())
                .unwrap(),
        )
        .unwrap()
    }
    fn template(&self) -> Template {
        let mut d = crate::workload_leases::definition::camera_tests::camera();
        d.executor_node = "node".into();
        let Source::CameraMotion {
            resource_id,
            resource_sha256,
            ..
        } = &mut d.source
        else {
            unreachable!()
        };
        *resource_id = "node::camera".into();
        *resource_sha256 = camera_resource_digest(&self.engine.applied_resources()[0]).unwrap();
        d.template()
    }
}

#[test]
fn passive_camera_inspection_has_its_own_permissions_and_keeps_all_private_bindings_local() {
    let f = Fixture::new();
    let before = std::fs::read(&f.service.config_path).unwrap();
    let report = f.inspect();
    report.validate(&f.request()).unwrap();
    assert_eq!(report.resources.len(), 2);
    assert!(report.saved_camera_permission && report.applied_camera_permission);
    assert!(report.listening_enabled && !report.listening_blocked && report.local_bindings_current);
    assert!(
        !report.execution_available
            && !report.execution_authorized
            && !report.capture_started
            && !report.python_configured
    );
    let encoded = serde_json::to_string(&report).unwrap();
    for private in [
        "PRIVATE_",
        "rtsp://",
        "camera.invalid",
        "microphone_permission",
    ] {
        assert!(!encoded.contains(private));
    }
    assert!(!encoded.contains(&f.dir.path().to_string_lossy().to_string()));
    for (row, resource) in report.resources.iter().zip(f.engine.applied_resources()) {
        assert_eq!(
            row.resource_sha256,
            Some(camera_resource_digest(resource).unwrap())
        );
    }
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
    let text = render(&json!(report)).unwrap();
    assert!(text.contains("Camera permission (saved/applied): true/true"));
    assert!(text.contains("execution is not available"));
    assert!(!text.contains("PRIVATE_"));
}

#[test]
fn malformed_camera_claims_scopes_and_private_resource_fields_are_refused() {
    let f = Fixture::new();
    let report = f.inspect();
    for (field, replacement) in [
        ("schema_version", json!(2)),
        ("report_type", json!("workload_sources")),
        ("source_kind", json!("microphone_energy")),
        ("capture_started", json!(true)),
        ("execution_available", json!(true)),
        ("execution_authorized", json!(true)),
        ("adapter_sha256", json!("0".repeat(63))),
        ("node_id", json!("other")),
        ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
        ("listening_revision", json!(u64::MAX)),
    ] {
        let mut value = json!(report);
        value[field] = replacement;
        let claimed: Report = serde_json::from_value(value).unwrap();
        assert!(claimed.validate(&f.request()).is_err(), "{field}");
    }
    let mut duplicate = report.clone();
    duplicate.resources.push(duplicate.resources[0].clone());
    assert!(duplicate.validate(&f.request()).is_err());
    let mut foreign = report.clone();
    foreign.resources[0].resource_id = "other::camera".into();
    assert!(foreign.validate(&f.request()).is_err());
    let mut private = json!(report);
    private["resources"][0]["device"] = json!("PRIVATE_REDIRECT");
    assert!(serde_json::from_value::<Report>(private).is_err());
    let mut wrong = f.request();
    wrong.expected_node_id = "other".into();
    assert!(f.service.camera_workload_sources(wrong).is_err());
    let mut wrong = f.request();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.camera_workload_sources(wrong).is_err());
}

#[test]
fn camera_review_requires_exact_binding_and_never_accepts_a_microphone_template() {
    let f = Fixture::new();
    let report = f.inspect();
    let template = f.template();
    report.confirms_template(&template).unwrap();
    for field in ["resource_id", "resource_sha256", "adapter_sha256"] {
        let mut changed = json!(template);
        changed["source"][field] = if field == "resource_id" {
            json!("node::netcam")
        } else {
            json!("f".repeat(64))
        };
        let template: Template = serde_json::from_value(changed).unwrap();
        assert!(report.confirms_template(&template).is_err());
    }
    let mut stale = report.clone();
    stale.local_bindings_current = false;
    assert!(stale.confirms_template(&template).is_err());
    let mut unavailable = report.clone();
    unavailable.resources[0].available = false;
    assert!(unavailable.confirms_template(&template).is_err());
    let mut microphone = crate::grants::workload::tests::definition();
    microphone.executor_node = "node".into();
    if let Source::MicrophoneEnergy { resource_id, .. } = &mut microphone.source {
        *resource_id = "node::mic".into();
    }
    assert!(report.confirms_template(&microphone.template()).is_err());
    assert!(!report.execution_available && !report.execution_authorized);
}

#[test]
fn pending_camera_binding_edits_and_revocation_are_visible_without_substituting_applied_source() {
    let f = Fixture::new();
    let original = f.inspect();
    let mut config = f.service.read_config().unwrap();
    config["metadata"]["resources"][0]["parameters"]["device"] = json!(1);
    config["permissions"]["media"]["camera"] = json!(false);
    f.service.save_configuration(config).unwrap();
    let current = f.inspect();
    assert!(
        !current.local_bindings_current
            && !current.saved_camera_permission
            && current.applied_camera_permission
    );
    assert!(current.listening_blocked && current.listening_epoch > original.listening_epoch);
    assert_eq!(
        current.resources[0].resource_sha256,
        original.resources[0].resource_sha256
    );
    assert!(current.confirms_template(&f.template()).is_err());
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
}

#[tokio::test]
async fn same_uuid_replacement_during_blocked_camera_inspection_refuses_the_old_snapshot() {
    let mut f = Fixture::new();
    let service = f.service.clone();
    let (release, wait) = std::sync::mpsc::channel();
    let (locked, ready) = tokio::sync::oneshot::channel();
    let blocker = std::thread::spawn(move || {
        service
            .inspect_grant_configuration(|_| {
                locked.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                Ok(())
            })
            .unwrap()
    });
    ready.await.unwrap();
    let service = f.service.clone();
    let request = f.request();
    let (reply, result) = tokio::sync::oneshot::channel();
    let inspector = std::thread::spawn(move || {
        let _ = reply.send(service.camera_workload_sources(request));
    });
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
    let before = std::time::Instant::now();
    let replacement = f
        .service
        .attach_preview_runtime(&f.engine, f.runtime.clone());
    drop(f.guard.take());
    assert!(
        before.elapsed() < std::time::Duration::from_secs(1),
        "Runtime replacement cannot join blocked inspection"
    );
    release.send(()).unwrap();
    blocker.join().unwrap();
    inspector.join().unwrap();
    assert!(
        result
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("changed during inspection")
    );
    assert!(f.inspect().local_bindings_current);
    drop(replacement);
    assert!(f.service.camera_workload_sources(f.request()).is_err());
}

#[tokio::test]
async fn authenticated_camera_snapshot_uses_current_pinned_registration_without_execution_support()
{
    let f = Fixture::new();
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    let key = crate::authentication::generate_credential();
    let server = crate::NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            tls: Some(identity.clone()),
            credentials: std::collections::BTreeMap::from([("node".into(), Some(key.clone()))]),
        },
    )
    .unwrap();
    server.start().await.unwrap();
    let config = serde_json::from_value(json!({"node_id":"node","psk":key,"auth_mode":"node_credential",
        "coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",server.port())],"heartbeat_seconds":0.2})).unwrap();
    let client = crate::NodeClient::new(config, f.engine.clone())
        .unwrap()
        .with_grants(&f.dir.path().join("receipts.json"), Default::default())
        .with_registration_runtime(&f.runtime)
        .unwrap()
        .with_status(f.service.live.clone());
    let task = tokio::spawn(async move { client.connect_once().await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if server.grant_session("node").await.is_ok()
                && server.node_state("node").await.is_some_and(|s| {
                    s.registration["registration_receipt"]["runtime_id"] == f.runtime
                })
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let before = std::fs::read(&f.service.config_path).unwrap();
    let remote = server.camera_workload_sources("node", 5).await.unwrap();
    assert_eq!(json!(remote), json!(f.inspect()));
    assert!(!remote.capture_started && !remote.execution_authorized && !remote.execution_available);
    assert!(!serde_json::to_string(&remote).unwrap().contains("PRIVATE_"));
    assert!(server.workload_session(&f.template(), 5).await.is_err());
    assert!(server.camera_workload_sources("node", 0).await.is_err());
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
    server.stop().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(server.camera_workload_sources("node", 5).await.is_err());
}
