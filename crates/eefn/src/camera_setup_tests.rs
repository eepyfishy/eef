use super::*;
use std::sync::Arc;

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    runtime: String,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(resources: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let config = json!({"node_id":"node","psk":"PRIVATE_SECRET","future":{"keep":true},"permissions":{},
            "metadata":{"extensions":{"keep":true},"unknown_metadata":"preserved","resources":resources}});
        crate::setup::write_json(&path, &config).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let metadata = serde_json::from_value(config["metadata"].clone()).unwrap();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap(),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        Self {
            dir,
            service,
            runtime,
            _engine: engine,
            _guard: guard,
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
            .camera_setup(self.request(Command::Show), false)
            .unwrap()
    }
    fn configure_request(&self, settings: Settings) -> Request {
        self.request(Command::Configure {
            settings,
            expected_digest: self.show()["saved_digest"].as_str().unwrap().into(),
        })
    }
    fn configure(&self, settings: Settings) -> Value {
        self.service
            .camera_setup(self.configure_request(settings), false)
            .unwrap()
    }
    fn config(&self) -> Value {
        self.service.read_config().unwrap()
    }
}

#[test]
fn default_camera_setup_preserves_owner_data_permissions_and_consent_without_starting_work() {
    let f = Fixture::new(json!([]));
    let before = f.config();
    let result = f.configure(Settings::default());
    assert_eq!(result["changed"], true);
    assert_eq!(result["restart_required"], true);
    let after = f.config();
    for key in ["psk", "future", "permissions"] {
        assert_eq!(after[key], before[key]);
    }
    for key in ["extensions", "unknown_metadata"] {
        assert_eq!(after["metadata"][key], before["metadata"][key]);
    }
    assert!(after.get("listening").is_none() && after.get("workload_policy").is_none());
    assert_eq!(after["metadata"]["resources"].as_array().unwrap().len(), 1);
    assert_eq!(result["saved_resources"][0]["available"], true);
    assert!(result["applied_resources"].as_array().unwrap().is_empty());
    let unchanged = f.configure(Settings::default());
    assert_eq!(unchanged["changed"], false);
    assert_eq!(f.config(), after);
    assert!(f.service.camera_pool.reserve().is_ok());
    for key in [
        "capture_started",
        "execution_authorized",
        "restart_requested",
        "automatically_retried",
    ] {
        assert_eq!(result[key], false);
    }
    assert!(f.service.restart.notified().now_or_never().is_none());
}

#[test]
fn rtsp_binding_stays_private_and_explicit_webcam_selection_replaces_it_without_erasing_metadata() {
    let f = Fixture::new(
        json!([{"id":"camera","capability":"camera.capture","parameters":{"device":0},"unknown_resource":"preserved","properties":{"PRIVATE_PROPERTY":true}}]),
    );
    assert_eq!(f.show()["restart_required"], false);
    let url = "rtsps://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:7441/private";
    let report = f.configure(Settings {
        rtsp: Some(url.into()),
        ..Default::default()
    });
    assert_eq!(
        f.config()["metadata"]["resources"][0]["parameters"]["device"],
        url
    );
    assert_eq!(
        f.config()["metadata"]["resources"][0]["unknown_resource"],
        "preserved"
    );
    assert_eq!(report["saved_resources"][0]["id"], "node::camera");
    assert_ne!(report["saved_resources"], report["applied_resources"]);
    for output in [
        report.to_string(),
        render(&report).unwrap(),
        f.show().to_string(),
    ] {
        assert!(
            !output.contains("PRIVATE_")
                && !output.contains("camera.invalid")
                && !output.contains("rtsps://")
        );
    }
    f.configure(Settings {
        resource: Some("node::camera".into()),
        device: Some(2),
        ..Default::default()
    });
    assert_eq!(
        f.config()["metadata"]["resources"][0]["parameters"]["device"],
        2
    );
}

#[test]
fn ambiguous_or_incompatible_cameras_refuse_without_saving_or_selecting_another_resource() {
    for resources in [
        json!([{"id":"one","capability":"camera.capture"},{"id":"two","capability":"camera.capture"}]),
        json!([{"id":"camera","capability":"camera.capture","available":false}]),
        json!([{"id":"camera","capability":"camera.capture","parameters":{"plugin":"private.py"}}]),
    ] {
        let f = Fixture::new(resources);
        let before = f.config();
        assert!(
            f.service
                .camera_setup(f.configure_request(Settings::default()), false)
                .is_err()
        );
        assert_eq!(f.config(), before);
    }
    let f = Fixture::new(json!([{"id":"mic","capability":"audio.capture"}]));
    let before = f.config();
    for id in ["mic", "other::camera"] {
        assert!(
            f.service
                .camera_setup(
                    f.configure_request(Settings {
                        resource: Some(id.into()),
                        ..Default::default()
                    }),
                    false
                )
                .is_err()
        );
        assert_eq!(f.config(), before);
    }
}

#[test]
fn explicit_resource_creation_and_updates_preserve_other_resources_and_private_bindings() {
    let f = Fixture::new(
        json!([{"id":"one","capability":"camera.capture","parameters":{"device":1}}, {"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_MIC"}}]),
    );
    let original = f.config()["metadata"]["resources"].clone();
    let added = f.configure(Settings {
        resource: Some("two".into()),
        device: Some(2),
        ..Default::default()
    });
    assert_eq!(added["saved_resources"].as_array().unwrap().len(), 2);
    assert_eq!(
        &f.config()["metadata"]["resources"].as_array().unwrap()[..2],
        original.as_array().unwrap()
    );
    f.configure(Settings {
        resource: Some("node::one".into()),
        device: Some(3),
        ..Default::default()
    });
    assert_eq!(
        f.config()["metadata"]["resources"][0]["parameters"]["device"],
        3
    );
    assert_eq!(
        f.config()["metadata"]["resources"][2]["parameters"]["device"],
        2
    );
}

#[test]
fn stale_digest_runtime_identity_and_unapproved_remote_changes_are_refused_before_save() {
    let f = Fixture::new(json!([]));
    let stale = f.configure_request(Settings::default());
    let mut next = f.config();
    next["metadata"]["extensions"]["later"] = json!(true);
    f.service.save_config(next.clone()).unwrap();
    assert!(f.service.camera_setup(stale, false).is_err());
    let request = f.configure_request(Settings::default());
    assert!(f.service.camera_setup(request.clone(), true).is_err());
    let mut wrong = request.clone();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.camera_setup(wrong, false).is_err());
    let mut wrong = request;
    wrong.expected_node_id = "other".into();
    assert!(f.service.camera_setup(wrong, false).is_err());
    assert_eq!(f.config(), next);
    let mut allowed = next;
    allowed["management"] = json!({"allow_remote":true});
    f.service.save_config(allowed).unwrap();
    let request = f.configure_request(Settings::default());
    assert!(f.service.camera_setup(request, true).is_ok());
}

#[test]
fn failed_storage_does_not_claim_a_save_or_request_restart() {
    let f = Fixture::new(json!([]));
    let before = f.config();
    let request = f.configure_request(Settings::default());
    std::fs::create_dir(f.dir.path().join("node.json.bak")).unwrap();
    assert!(f.service.camera_setup(request, false).is_err());
    assert_eq!(f.config(), before);
    assert_ne!(f.service.live.lock().unwrap()["pending_restart"], true);
    assert!(f.service.restart.notified().now_or_never().is_none());
}

#[test]
fn confirmation_rejects_substituted_requests_malformed_resources_and_effect_claims() {
    let f = Fixture::new(json!([]));
    let request = f.request(Command::Show);
    let report = f.show();
    assert!(confirmed(&report, &request));
    for (key, value) in [
        ("schema_version", json!(2)),
        ("capture_started", json!(true)),
        ("execution_authorized", json!(true)),
        ("automatically_retried", json!(true)),
        ("restart_requested", json!(true)),
        ("changed", json!(true)),
        ("request_sha256", json!("a".repeat(64))),
        (
            "saved_resources",
            json!([{"id":"other::camera","available":true,"binding_sha256":null}]),
        ),
    ] {
        let mut forged = report.clone();
        forged[key] = value;
        assert!(!confirmed(&forged, &request), "{key}");
    }
    let other = f.configure_request(Settings::default());
    assert!(!confirmed(&report, &other));
    let mut invalid = report.clone();
    invalid["node_id"] = json!("private\nnode");
    assert!(render(&invalid).is_err());
    let mut invalid = report;
    invalid["saved_resources"] =
        json!([{"id":"other::camera","available":true,"binding_sha256":null}]);
    assert!(render(&invalid).is_err());
}

#[test]
fn invalid_selectors_urls_and_authority_fields_fail_closed_without_opening_a_source() {
    for value in [
        json!({"device":1024}),
        json!({"device":1,"rtsp":"rtsp://camera.invalid/live"}),
        json!({"rtsp":"https://camera.invalid"}),
        json!({"rtsp":"rtsp://camera.invalid:0/live"}),
        json!({"rtsp":"rtsp://camera.invalid/live#private"}),
        json!({"rtsp":"rtsp://camera.invalid/\nprivate"}),
        json!({"resource":"../camera"}),
    ] {
        assert!(
            serde_json::from_value::<Settings>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    for key in [
        "capture",
        "permission",
        "listening",
        "restart",
        "grant",
        "plugin",
    ] {
        assert!(serde_json::from_value::<Settings>(json!({key:true})).is_err());
    }
}

use futures_util::FutureExt;
