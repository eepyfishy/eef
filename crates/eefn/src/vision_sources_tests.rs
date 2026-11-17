use super::*;
use std::sync::Arc;

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    engine: Arc<crate::NodeEngine>,
    runtime: String,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(url: &str, modality: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let metadata: crate::context::NodeMetadata = serde_json::from_value(json!({"resources":[
            {"id":"camera","name":"Room camera","capability":"camera.capture",
            "parameters":{"device":"rtsp://owner:PRIVATE_PASSWORD@camera.invalid/live"}},
            {"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_MIC"}}
        ]}))
        .unwrap();
        let selected: crate::SelectedModel =
            serde_json::from_value(json!({"model_id":"selected","modality":modality})).unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"node","psk":"PRIVATE_CREDENTIAL",
            "metadata":metadata,"permissions":{"media":{"camera":true}},"listening":{"enabled":true},
            "models":{"provider":"ollama","ollama":{"base_url":url,"selected":[selected]}}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let mut permissions = crate::NodePolicy::default();
        permissions.media.camera = true;
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(permissions)
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap()
                .with_service(service.clone())
                .with_ollama(url.into(), [selected])
                .unwrap(),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            engine,
            runtime,
            _guard: guard,
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
                .vision_workload_sources(self.request())
                .unwrap(),
        )
        .unwrap()
    }
    fn template(&self) -> Template {
        let mut d = crate::workload_leases::definition::vision::tests::vision();
        d.executor_node = "node".into();
        if let Source::CameraVision {
            resource_id,
            resource_sha256,
            ..
        } = &mut d.source
        {
            *resource_id = "node::camera".into();
            *resource_sha256 = crate::workload_leases::definition::camera_resource_digest(
                &self.engine.applied_resources()[0],
            )
            .unwrap();
        }
        d.template()
    }
}

#[tokio::test]
async fn passive_vision_inspection_never_contacts_model_captures_or_exposes_private_bindings() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let f = Fixture::new(&format!("http://{}", listener.local_addr().unwrap()), "vlm");
    let before = std::fs::read(&f.service.config_path).unwrap();
    let report = f.inspect();
    report.validate(&f.request()).unwrap();
    assert_eq!(report.models.len(), 1);
    assert!(report.models[0].local_endpoint_configured);
    assert!(report.model_bindings_current && report.camera.local_bindings_current);
    assert!(
        !report.capture_started
            && !report.model_contacted
            && !report.execution_available
            && !report.execution_authorized
            && !report.configuration_ready
            && !report.vision_enabled
    );
    assert!(f.service.camera_pool.reserve().is_ok());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    let value = json!(report);
    let raw = value.to_string() + &render(&value).unwrap();
    for private in [
        "PRIVATE_",
        "rtsp://",
        "camera.invalid",
        "http://",
        &f.dir.path().to_string_lossy(),
    ] {
        assert!(!raw.contains(private));
    }
    assert!(raw.contains("Execution requires a separately reviewed job and current node support"));
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
}

#[test]
fn vision_snapshot_rejects_forged_scopes_execution_claims_and_unknown_fields() {
    let f = Fixture::new("http://127.0.0.1:1", "vlm");
    let report = f.inspect();
    for (field, replacement) in [
        ("schema_version", json!(2)),
        ("node_id", json!("other")),
        ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
        ("source_kind", json!("camera_motion")),
        ("report_type", json!("camera_workload_sources")),
        ("capture_started", json!(true)),
        ("model_contacted", json!(true)),
        ("execution_available", json!(true)),
        ("execution_authorized", json!(true)),
        ("configuration_ready", json!(true)),
        ("policy_revision", json!(u64::MAX)),
        ("vision_enabled", json!(true)),
    ] {
        let mut value = json!(report);
        value[field] = replacement;
        let claimed: Report = serde_json::from_value(value).unwrap();
        assert!(claimed.validate(&f.request()).is_err(), "{field}");
    }
    let mut duplicate = report.clone();
    duplicate.models.push(duplicate.models[0].clone());
    assert!(duplicate.validate(&f.request()).is_err());
    let mut private = json!(report);
    private["models"][0]["url"] = json!("PRIVATE_URL");
    assert!(serde_json::from_value::<Report>(private).is_err());
    let mut wrong = f.request();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.vision_workload_sources(wrong).is_err());
    let mut wrong = f.request();
    wrong.expected_node_id = "other".into();
    assert!(f.service.vision_workload_sources(wrong).is_err());
}

#[test]
fn vision_review_requires_its_exact_camera_adapter_and_applied_local_model() {
    let f = Fixture::new("http://127.0.0.1:1", "vlm");
    let report = f.inspect();
    let template = f.template();
    report.confirms_template(&template).unwrap();
    for (pointer, value) in [
        ("/source/resource_id", json!("node::other")),
        ("/source/resource_sha256", json!("b".repeat(64))),
        ("/source/adapter_sha256", json!("c".repeat(64))),
        ("/source/parameters/model/model_id", json!("other")),
    ] {
        let mut changed = json!(template);
        *changed.pointer_mut(pointer).unwrap() = value;
        let changed: Template = serde_json::from_value(changed).unwrap();
        assert!(report.confirms_template(&changed).is_err(), "{pointer}");
    }
    let mut camera = crate::workload_leases::definition::camera_tests::camera();
    camera.executor_node = "node".into();
    if let Source::CameraMotion { resource_id, .. } = &mut camera.source {
        *resource_id = "node::camera".into();
    }
    assert!(report.confirms_template(&camera.template()).is_err());
    let mut unavailable = report.clone();
    unavailable.camera.resources[0].available = false;
    assert!(unavailable.confirms_template(&template).is_err());
}

#[test]
fn pending_model_selection_changes_require_restart_and_cannot_claim_current_vision_bindings() {
    let f = Fixture::new("http://127.0.0.1:1", "vlm");
    let before = f.inspect();
    let mut next = f.service.read_config().unwrap();
    next["models"]["ollama"]["selected"][0]["model_id"] = json!("changed");
    f.service.save_configuration(next).unwrap();
    let after = f.inspect();
    assert!(!after.model_bindings_current && !after.configuration_ready);
    assert_eq!(json!(after.models), json!(before.models));
    assert!(after.confirms_template(&f.template()).is_err());
    assert!(!after.model_contacted && !after.execution_authorized);
}

#[test]
fn remote_endpoints_and_text_only_models_never_qualify_as_local_vision_configuration() {
    for url in [
        "http://example.invalid",
        "http://localhost:11434",
        "http://owner:secret@127.0.0.1:1",
        "http://127.0.0.1:1/path",
    ] {
        let f = Fixture::new(url, "vlm");
        let report = f.inspect();
        assert_eq!(report.models.len(), 1);
        assert!(!report.models[0].local_endpoint_configured);
        assert!(!report.configuration_ready);
        assert!(report.confirms_template(&f.template()).is_err());
    }
    let f = Fixture::new("http://127.0.0.1:1", "text");
    let report = f.inspect();
    assert!(report.models.is_empty() && !report.configuration_ready);
    assert!(report.confirms_template(&f.template()).is_err());
}

#[tokio::test]
async fn authenticated_vision_snapshot_uses_pinned_node_session_without_enabling_execution() {
    let f = Fixture::new("http://127.0.0.1:1", "vlm");
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
    let config=serde_json::from_value(json!({"node_id":"node","psk":key,"auth_mode":"node_credential",
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
    let report = server.vision_workload_sources("node", 5).await.unwrap();
    assert_eq!(json!(report), json!(f.inspect()));
    assert!(!report.capture_started && !report.model_contacted && !report.execution_available);
    let snapshot = server.workload_source(&f.template(), 5).await.unwrap();
    assert!(matches!(
        &snapshot,
        crate::workload_sources::SourceSnapshot::Vision(_)
    ));
    assert_eq!(snapshot.runtime_id(), f.runtime);
    snapshot.confirms_template(&f.template()).unwrap();
    assert!(
        snapshot
            .confirms_execution(&f.template(), &f.runtime)
            .is_err()
    );
    let mut changed = f.template();
    if let Source::CameraVision { parameters, .. } = &mut changed.source {
        parameters.model.model_id = "unselected".into();
    }
    assert!(server.workload_source(&changed, 5).await.is_err());
    assert!(server.workload_session(&f.template(), 5).await.is_err());
    assert!(server.vision_workload_sources("node", 0).await.is_err());
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
    server.stop().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(server.vision_workload_sources("node", 5).await.is_err());
}
