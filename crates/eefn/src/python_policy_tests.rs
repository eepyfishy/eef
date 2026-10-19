use super::*;
struct Fixture {
    dir: tempfile::TempDir,
    service: std::sync::Arc<crate::NodeService>,
    runtime: String,
    _engine: std::sync::Arc<crate::NodeEngine>,
    guard: Option<crate::request_preview::PreviewGuard>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path, &json!({"node_id":"node","psk":"private-secret","endpoints":[],"future":{"keep":true},"management":{"allow_remote":false}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let engine =
            std::sync::Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        {
            let mut live = service.live.lock().unwrap();
            live["runtime_id"] = json!(runtime);
            live["python_bridge"] = json!(BridgeLimits::default());
        }
        Self {
            dir,
            service,
            runtime,
            _engine: engine,
            guard: Some(guard),
        }
    }
    fn request(&self, command: Command) -> Request {
        Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command,
        }
    }
    fn show(&self) -> Value {
        self.service
            .python_policy(self.request(Command::Show), false)
            .unwrap()
    }
    fn configure(&self, limits: BridgeLimits) -> Request {
        self.request(Command::Configure {
            limits,
            expected_digest: self.show()["saved_digest"].as_str().unwrap().into(),
        })
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("node.json")).unwrap()
    }
}
#[test]
fn python_policy_atomic_save_preserves_settings_and_noop_does_not_write() {
    let f = Fixture::new();
    let limits = BridgeLimits {
        timeout_seconds: 7,
        max_job_memory_mib: cfg!(windows).then_some(128),
        ..Default::default()
    };
    let request = f.configure(limits.clone());
    let report = f.service.python_policy(request.clone(), false).unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["saved"], json!(limits));
    assert_eq!(report["applied"], json!(BridgeLimits::default()));
    assert_eq!(report["changed"], true);
    assert_eq!(report["restart_required"], true);
    let config: Value = serde_json::from_slice(&f.bytes()).unwrap();
    assert_eq!(config["psk"], "private-secret");
    assert_eq!(config["future"]["keep"], true);
    assert_eq!(config["management"]["allow_remote"], false);
    let before = f.bytes();
    assert!(
        f.service.python_policy(request, false).is_err(),
        "stale pre-save digest is not replay authority"
    );
    let no_op = f.service.python_policy(f.configure(limits), false).unwrap();
    assert_eq!(no_op["changed"], false);
    assert_eq!(before, f.bytes());
}
#[tokio::test]
async fn python_policy_rechecks_remote_consent_runtime_and_reserved_status() {
    let f = Fixture::new();
    let request = f.configure(BridgeLimits {
        timeout_seconds: 8,
        ..Default::default()
    });
    let before = f.bytes();
    assert!(
        f.service
            .remote("python_policy", json!(request))
            .await
            .is_err()
    );
    assert!(
        f.service
            .remote("python_policy_status", json!(request))
            .await
            .is_err()
    );
    assert!(
        f.service
            .remote("python_policy_status", json!(f.request(Command::Show)))
            .await
            .is_ok()
    );
    let mut wrong = f.request(Command::Show);
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.python_policy(wrong, false).is_err());
    let mut invalid = request.clone();
    if let Command::Configure { limits, .. } = &mut invalid.command {
        limits.max_job_memory_mib = Some(63);
    }
    assert!(f.service.python_policy(invalid, false).is_err());
    assert_eq!(before, f.bytes());
    let mut config: Value = serde_json::from_slice(&before).unwrap();
    config["management"]["allow_remote"] = json!(true);
    crate::setup::write_json(&f.dir.path().join("node.json"), &config).unwrap();
    assert!(
        f.service
            .remote("python_policy", json!(request))
            .await
            .is_ok()
    );
    config = serde_json::from_slice(&f.bytes()).unwrap();
    config["management"]["allow_remote"] = json!(false);
    crate::setup::write_json(&f.dir.path().join("node.json"), &config).unwrap();
    let current = f.configure(BridgeLimits::default());
    assert!(
        f.service
            .remote("python_policy", json!(current))
            .await
            .is_err()
    );
}
#[test]
fn python_policy_confirmation_rejects_substitution_forged_effects_and_bad_digests() {
    let f = Fixture::new();
    let request = f.request(Command::Show);
    let report = f.show();
    let mut missing = report.clone();
    missing.as_object_mut().unwrap().remove("applied");
    assert!(!confirmed(&missing, &request));
    for (key, value) in [
        ("execution_authorized", json!(true)),
        ("restart_requested", json!(true)),
        ("changed", json!(true)),
        ("saved_digest", json!("a".repeat(64))),
        ("note", json!("PRIVATE_UNCONFIRMED")),
        ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
    ] {
        let mut bad = report.clone();
        bad[key] = value;
        assert!(!confirmed(&bad, &request));
    }
    let mut bad = report.clone();
    bad["saved"]["timeout_seconds"] = json!(9);
    assert!(!confirmed(&bad, &request));
    let mut bad = report.clone();
    bad["applied"]["max_job_memory_mib"] = json!(63);
    assert!(!confirmed(&bad, &request));
    assert!(render(&bad).is_err());
    assert!(serde_json::from_value::<Command>(json!({"operation":"configure","limits":{},"expected_digest":"a".repeat(64),"restart":true})).is_err());
}
#[test]
fn python_policy_patch_retains_other_budgets_and_memory_default_is_explicit() {
    let mut settings = Settings {
        timeout_seconds: None,
        max_request_bytes: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
        memory_mib: None,
        default_memory: false,
    };
    assert!(settings.merge(BridgeLimits::default()).is_err());
    settings.timeout_seconds = Some(3);
    let original = BridgeLimits {
        max_job_memory_mib: Some(128),
        ..Default::default()
    };
    let merged = settings.merge(original.clone()).unwrap();
    assert_eq!(merged.timeout_seconds, 3);
    assert_eq!(merged.max_job_memory_mib, Some(128));
    assert_eq!(merged.max_stdout_bytes, original.max_stdout_bytes);
    settings.default_memory = true;
    assert_eq!(settings.merge(original).unwrap().max_job_memory_mib, None);
    settings.memory_mib = Some(128);
    assert!(settings.merge(BridgeLimits::default()).is_err());
}

#[test]
fn python_policy_runtime_retirement_cannot_save_using_a_stale_live_snapshot() {
    let mut f = Fixture::new();
    let request = f.configure(BridgeLimits {
        timeout_seconds: 6,
        ..Default::default()
    });
    let before = f.bytes();
    drop(f.guard.take());
    assert!(f.service.python_policy(request, false).is_err());
    assert_eq!(f.bytes(), before);
}
