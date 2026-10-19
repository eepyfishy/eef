use super::*;
use std::sync::Arc;

struct Fixture {
    service: Arc<crate::NodeService>,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
    runtime: String,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new(extra: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let mut config = json!({"node_id":"node","psk":"private-secret","future":{"preserved":true},"permissions":{},"metadata":{"extensions":{"keep":true}}});
        for (key, value) in extra.as_object().unwrap() {
            config[key] = value.clone();
        }
        crate::setup::write_json(&path, &config).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let metadata =
            serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({}))).unwrap();
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap(),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        service.live.lock().unwrap()["connection"] =
            json!({"state":"connected","transport":"pinned_tls13"});
        service.live.lock().unwrap()["control_local_ip"] = json!("127.0.0.1");
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        Self {
            service,
            _engine: engine,
            _guard: guard,
            runtime,
            _dir: dir,
        }
    }
    fn request(&self, command: RelayCommand) -> RelayRequest {
        RelayRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command,
        }
    }
    fn show(&self) -> Value {
        self.service
            .relay_command(self.request(RelayCommand::Show), false)
            .unwrap()
    }
    fn configure(&self, settings: Configuration, remote: bool) -> Result<Value> {
        let expected_digest = self.show()["saved_digest"].as_str().unwrap().to_string();
        self.service.relay_command(
            self.request(RelayCommand::Configure {
                settings,
                expected_digest,
            }),
            remote,
        )
    }
}
#[tokio::test]
async fn sender_only_noop_does_not_save_and_private_binding_drift_requires_restart() {
    let f = Fixture::new(json!({}));
    let before = f.service.read_config().unwrap();
    let no_change = f
        .configure(
            Configuration {
                send_only: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(no_change["changed"], false);
    assert_eq!(f.service.read_config().unwrap(), before);
    assert!(render(&no_change).unwrap().contains("First choose a role"));
    let f = Fixture::new(
        json!({"metadata":{"resources":[{"id":"mic","capability":"audio.capture","parameters":{"device":0}}]}}),
    );
    assert_eq!(f.show()["restart_required"], false);
    let mut saved = f.service.read_config().unwrap();
    saved["metadata"]["resources"][0]["parameters"]["device"] = json!(1);
    f.service.save_config(saved).unwrap();
    let report = f.show();
    assert_eq!(report["saved_resources"], report["applied_resources"]);
    assert_eq!(
        report["restart_required"], true,
        "private device binding drift is not hidden by an unchanged public resource view"
    );
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("\"device\"")
    );
}
#[tokio::test]
async fn setup_is_atomic_permission_neutral_stable_and_never_listens() {
    let f = Fixture::new(json!({}));
    let before = f.service.read_config().unwrap();
    let settings = Configuration {
        microphone: true,
        receive: true,
        ..Default::default()
    };
    let report = f.configure(settings.clone(), false).unwrap();
    assert_eq!(report["changed"], true);
    assert_eq!(report["restart_required"], true);
    assert_eq!(report["peer_permission_saved"], false);
    assert_eq!(report["saved_resources"].as_array().unwrap().len(), 3);
    let after = f.service.read_config().unwrap();
    for key in ["node_id", "psk", "future", "permissions"] {
        assert_eq!(after[key], before[key]);
    }
    assert_eq!(
        after["metadata"]["extensions"],
        before["metadata"]["extensions"]
    );
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("private-secret")
    );
    let address: SocketAddr =
        serde_json::from_value(report["saved_receive_address"].clone()).unwrap();
    assert!(address.ip().is_loopback());
    assert_ne!(address.port(), 0);
    assert!(
        tokio::net::TcpStream::connect(address).await.is_err(),
        "setup did not leave a listener"
    );
    let repeated = f.configure(settings, false).unwrap();
    assert_eq!(repeated["changed"], false);
    assert_eq!(f.service.read_config().unwrap(), after);
    assert!(render(&report).unwrap().contains("loopback-only"));
    let mut request = f.request(RelayCommand::Show);
    request.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.relay_command(request, false).is_err());
    let cleared = f
        .configure(
            Configuration {
                send_only: true,
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert!(cleared["saved_receive_address"].is_null());
    assert_eq!(cleared["saved_resources"], report["saved_resources"]);
}
#[tokio::test]
async fn stale_or_remote_unapproved_changes_preserve_saved_state() {
    let f = Fixture::new(json!({}));
    let before = f.service.read_config().unwrap();
    let settings = Configuration {
        microphone: true,
        ..Default::default()
    };
    assert!(f.configure(settings.clone(), true).is_err());
    assert_eq!(f.service.read_config().unwrap(), before);
    let request = f.request(RelayCommand::Configure {
        settings: settings.clone(),
        expected_digest: f.show()["saved_digest"].as_str().unwrap().into(),
    });
    f.configure(settings, false).unwrap();
    let after = f.service.read_config().unwrap();
    assert!(f.service.relay_command(request, false).is_err());
    assert_eq!(f.service.read_config().unwrap(), after);
    f.service.live.lock().unwrap()["connection"] = json!({"state":"disconnected"});
    let saved = f.service.read_config().unwrap();
    assert!(
        f.configure(
            Configuration {
                receive: true,
                ..Default::default()
            },
            false
        )
        .is_err()
    );
    assert_eq!(
        f.service.read_config().unwrap(),
        saved,
        "failed route discovery must not save partial resources"
    );
}
#[tokio::test]
async fn ambiguous_disabled_or_incompatible_resources_are_not_silently_replaced() {
    for resources in [
        json!([{"id":"a","capability":"audio.capture"},{"id":"b","capability":"audio.capture"}]),
        json!([{"id":"a","capability":"audio.capture","available":false}]),
        json!([{"id":"a","capability":"audio.capture","parameters":{"program":"never"}}]),
    ] {
        let f = Fixture::new(json!({"metadata":{"resources":resources}}));
        let before = f.service.read_config().unwrap();
        assert!(
            f.configure(
                Configuration {
                    microphone: true,
                    ..Default::default()
                },
                false
            )
            .is_err()
        );
        assert_eq!(f.service.read_config().unwrap(), before);
    }
    let f = Fixture::new(
        json!({"metadata":{"resources":[{"id":"a","capability":"audio.capture"},{"id":"b","capability":"audio.capture"}]}}),
    );
    f.configure(
        Configuration {
            microphone: true,
            microphone_resource: Some("node::b".into()),
            microphone_device: Some(3),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    let config = f.service.read_config().unwrap();
    assert_eq!(
        config["metadata"]["resources"][1]["parameters"]["device"],
        3
    );
    assert!(config["metadata"]["resources"][0]["parameters"].is_null());
}
#[test]
fn configuration_and_confirmation_reject_scope_or_authority_substitution() {
    for settings in [
        Configuration::default(),
        Configuration {
            receive: true,
            send_only: true,
            ..Default::default()
        },
        Configuration {
            microphone: true,
            address: Some("127.0.0.1".parse().unwrap()),
            ..Default::default()
        },
        Configuration {
            receive: true,
            address: Some("0.0.0.0".parse().unwrap()),
            ..Default::default()
        },
        Configuration {
            receive: true,
            address: Some("255.255.255.255".parse().unwrap()),
            ..Default::default()
        },
        Configuration {
            receive: true,
            port: Some(0),
            ..Default::default()
        },
        Configuration {
            microphone: true,
            microphone_resource: Some("other::bad/path".into()),
            ..Default::default()
        },
    ] {
        assert!(settings.validate().is_err());
    }
    let f = Fixture::new(json!({}));
    let foreign = f.request(RelayCommand::Configure {
        settings: Configuration {
            microphone: true,
            microphone_resource: Some("other::mic".into()),
            ..Default::default()
        },
        expected_digest: f.show()["saved_digest"].as_str().unwrap().into(),
    });
    assert!(foreign.validate().is_err());
    assert!(f.service.relay_command(foreign, false).is_err());
    let request = f.request(RelayCommand::Show);
    let report = f.show();
    assert!(confirmed(&report, &request));
    for (field, value) in [
        ("execution_authorized", json!(true)),
        ("restart_requested", json!(true)),
        ("automatically_retried", json!(true)),
        ("node_id", json!("other")),
        ("changed", json!(true)),
        ("saved_digest", json!("bad")),
    ] {
        let mut altered = report.clone();
        altered[field] = value;
        assert!(!confirmed(&altered, &request));
    }
}
