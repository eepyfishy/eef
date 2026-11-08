use super::*;
use std::sync::Arc;

fn request() -> Request {
    Request {
        schema_version: 1,
        expected_node_id: "camera-check-node".into(),
        expected_runtime_id: uuid::Uuid::new_v4().to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        settings: Settings::default(),
    }
}
#[test]
fn camera_check_bounds_only_tighten_applied_python_limits() {
    let settings = Settings::default();
    let limits = settings.limits(&crate::python::BridgeLimits {
        timeout_seconds: 2,
        max_job_memory_mib: Some(64),
        ..Default::default()
    });
    assert_eq!(limits.timeout_seconds, 2);
    assert_eq!(limits.max_job_memory_mib, Some(64));
    assert_eq!(limits.max_stdout_bytes, 1024);
    assert_eq!(limits.max_stderr_bytes, 1024);
    assert!(limits.validate().is_ok());
    for (seconds, memory) in [(0, 128), (31, 128), (1, 63), (1, 4097)] {
        assert!(
            Settings {
                timeout_seconds: seconds,
                max_job_memory_mib: memory
            }
            .validate()
            .is_err()
        );
    }
    let mut value = json!(request());
    value["settings"]["module"] = json!("owner-input-must-not-select-code");
    assert!(serde_json::from_value::<Request>(value).is_err());
}
#[test]
fn camera_check_negative_reports_are_bound_to_exact_request_and_never_authorize_capture() {
    let request = request();
    let report = json!(Report::new(&request).unwrap());
    assert!(confirmed(&report, &request));
    for (key, value) in [
        ("success", json!(true)),
        ("capture_started", json!(true)),
        ("physical_ready", json!(true)),
        ("execution_authorized", json!(true)),
        ("restart_requested", json!(true)),
        ("automatically_retried", json!(true)),
        ("owned_exit_observed", json!(true)),
        ("status", json!("PRIVATE_runtime_output")),
        ("request_sha256", json!("b".repeat(64))),
        ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
    ] {
        let mut changed = report.clone();
        changed[key] = value;
        assert!(!confirmed(&changed, &request), "accepted forged {key}");
    }
    let mut ready = Report::new(&request).unwrap();
    ready.status = Status::Ready;
    ready.success = true;
    ready.owned_exit_observed = true;
    assert!(!confirmed(&json!(ready), &request));
    ready.effective_limits = Some(request.settings.limits(&Default::default()));
    assert!(confirmed(&json!(ready), &request));
    ready.effective_limits.as_mut().unwrap().max_job_memory_mib = Some(4096);
    assert!(!confirmed(&json!(ready), &request));
}
struct Fixture {
    _dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
    request: Request,
}
impl Fixture {
    fn new(python: Option<crate::python::PythonRuntime>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let request = request();
        let path = dir.path().join("node.json");
        crate::setup::write_json(
            &path,
            &json!({"node_id":request.expected_node_id,
            "permissions":{},"management":{"allow_remote":false},"custom":"PRIVATE_owner_data"}),
        )
        .unwrap();
        let service = crate::NodeService::new(path, request.expected_node_id.clone());
        let mut engine = crate::NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_service(service.clone())
            .with_resources(request.expected_node_id.clone(), Default::default())
            .unwrap();
        if let Some(python) = python {
            engine = engine.with_python_check_runtime(Arc::new(python));
        }
        let engine = Arc::new(engine);
        service.live.lock().unwrap()["runtime_id"] = json!(request.expected_runtime_id);
        let guard = service.attach_preview_runtime(&engine, request.expected_runtime_id.clone());
        Self {
            _dir: dir,
            service,
            _engine: engine,
            _guard: guard,
            request,
        }
    }
}
#[tokio::test]
async fn camera_check_fences_node_runtime_and_remote_approval_without_editing_owner_state() {
    let f = Fixture::new(None);
    let before = std::fs::read(&f.service.config_path).unwrap();
    let result = f
        .service
        .camera_check(f.request.clone(), false)
        .await
        .unwrap();
    assert!(confirmed(&result, &f.request));
    assert_eq!(result["status"], "runtime_unavailable");
    assert!(!result.to_string().contains("PRIVATE_"));
    assert!(
        f.service
            .camera_check(f.request.clone(), true)
            .await
            .is_err()
    );
    let mut wrong = f.request.clone();
    wrong.expected_node_id = "another-node".into();
    assert!(f.service.camera_check(wrong, false).await.is_err());
    let mut stale = f.request.clone();
    stale.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    let result = f.service.camera_check(stale.clone(), false).await.unwrap();
    assert!(confirmed(&result, &stale));
    assert_eq!(result["status"], "runtime_changed");
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
    assert!(f.service.camera_pool.reserve().is_ok());
}
#[tokio::test]
async fn camera_check_selection_does_not_register_capture_or_enable_python_execution() {
    let python =
        crate::python::PythonRuntime::discover(Some("must-not-execute-this-path".into())).unwrap();
    let f = Fixture::new(Some(python));
    assert!(!f._engine.python_configured());
    assert!(!f._engine.applied_permissions().media.camera);
    assert!(
        f._engine
            .camera_check_runtime(&f.request.expected_node_id, &f.service.camera_pool)
            .is_ok()
    );
    assert!(
        f._engine
            .camera_runtime(&f.request.expected_node_id, &f.service.camera_pool)
            .is_err()
    );
    assert!(
        f._engine
            .camera_check_runtime("another-node", &f.service.camera_pool)
            .is_err()
    );
    assert!(
        f._engine
            .camera_check_runtime(
                &f.request.expected_node_id,
                &crate::capture_ownership::Pool::camera()
            )
            .is_err()
    );
    #[cfg(not(windows))]
    {
        let result = f
            .service
            .camera_check(f.request.clone(), false)
            .await
            .unwrap();
        assert!(confirmed(&result, &f.request));
        assert_eq!(result["status"], "unsupported_platform");
    }
}

#[cfg(all(windows, feature = "native-test-fixtures"))]
#[tokio::test]
async fn camera_check_real_pinned_dependencies_without_permission_or_capture() {
    let python = std::env::var_os("EEF_TEST_CAMERA_PYTHON")
        .expect("Hosted native fixture requires the separately pinned OpenCV/NumPy interpreter");
    let f = Fixture::new(Some(
        crate::python::PythonRuntime::discover(Some(python.into())).unwrap(),
    ));
    let mut request = f.request.clone();
    request.settings.max_job_memory_mib = 512;
    request.settings.timeout_seconds = 20;
    let before = std::fs::read(&f.service.config_path).unwrap();
    let result = f
        .service
        .camera_check(request.clone(), false)
        .await
        .unwrap();
    assert!(confirmed(&result, &request));
    assert_eq!(result["status"], "ready", "{result}");
    assert_eq!(result["owned_exit_observed"], true);
    assert_eq!(result["capture_started"], false);
    assert!(!f._engine.python_configured());
    assert_eq!(std::fs::read(&f.service.config_path).unwrap(), before);
    let busy = f.service.camera_pool.reserve().unwrap();
    let result = f
        .service
        .camera_check(request.clone(), false)
        .await
        .unwrap();
    assert!(confirmed(&result, &request));
    assert_eq!(result["status"], "busy");
    drop(busy);
    assert!(f.service.camera_pool.reserve().is_ok());
}
