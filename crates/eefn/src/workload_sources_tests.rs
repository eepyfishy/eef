use super::*;
use std::sync::Arc;

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    runtime: String,
    resource: crate::context::Resource,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let resource: crate::context::Resource = serde_json::from_value(json!({"id":"mic","name":"Microphone","capability":"audio.capture","parameters":{"device":"PRIVATE_DEVICE_BINDING"},"properties":{"private":"PRIVATE_PROPERTY"}})).unwrap();
        let metadata = crate::context::NodeMetadata {
            resources: vec![resource.clone()],
            ..Default::default()
        };
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"node","psk":"PRIVATE_CREDENTIAL","metadata":metadata,"permissions":{"media":{"microphone":true}}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let mut policy = crate::NodePolicy::default();
        policy.media.microphone = true;
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_resources("node".into(), metadata)
                .unwrap(),
        );
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            runtime,
            resource,
            _engine: engine,
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
        serde_json::from_value(self.service.workload_sources(self.request()).unwrap()).unwrap()
    }
}

#[test]
fn snapshot_keeps_private_bindings_local_and_never_writes_or_claims_capture() {
    let f = Fixture::new();
    let path = f.dir.path().join("node.json");
    let before = std::fs::read(&path).unwrap();
    let report = f.inspect();
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains("PRIVATE_"));
    assert!(!encoded.contains(&f.dir.path().to_string_lossy().to_string()));
    assert_eq!(report.resources.len(), 1);
    assert_eq!(
        report.resources[0].resource_sha256,
        Some(microphone_resource_digest(&f.resource).unwrap())
    );
    assert!(report.local_bindings_current);
    assert!(report.saved_microphone_permission && report.applied_microphone_permission);
    assert!(!report.python_configured && !report.listening_enabled);
    assert!(!report.execution_available && !report.execution_authorized && !report.capture_started);
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn stale_runtime_and_malformed_claims_fail_without_a_fallback() {
    let f = Fixture::new();
    let mut request = f.request();
    request.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.workload_sources(request).is_err());
    let mut request = f.request();
    request.expected_node_id = "other".into();
    assert!(f.service.workload_sources(request).is_err());
    let report = f.inspect();
    let mut value = json!(report);
    value["device"] = json!("PRIVATE_INJECTION");
    assert!(serde_json::from_value::<Report>(value).is_err());
    for field in [
        "execution_available",
        "execution_authorized",
        "capture_started",
    ] {
        let mut value = json!(report);
        value[field] = json!(true);
        assert!(
            serde_json::from_value::<Report>(value)
                .unwrap()
                .validate(&f.request())
                .is_err()
        );
    }
    let mut duplicated = report.clone();
    duplicated.resources.push(report.resources[0].clone());
    assert!(duplicated.validate(&f.request()).is_err());
}
#[test]
fn pending_binding_edits_and_local_revocation_are_visible_without_exporting_new_private_values() {
    let f = Fixture::new();
    let original = f.inspect();
    let path = f.dir.path().join("node.json");
    let mut next: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    next["metadata"]["resources"][0]["parameters"]["device"] = json!("PRIVATE_NEW_DEVICE");
    next["permissions"]["media"]["microphone"] = json!(false);
    crate::setup::write_json(&path, &next).unwrap();
    f.service.listening_gate.deny();
    let current = f.inspect();
    assert!(!current.local_bindings_current);
    assert!(!current.saved_microphone_permission && current.applied_microphone_permission);
    assert!(current.listening_blocked && current.listening_epoch > original.listening_epoch);
    assert_eq!(
        current.resources[0].resource_sha256,
        original.resources[0].resource_sha256
    );
    assert!(
        !serde_json::to_string(&current)
            .unwrap()
            .contains("PRIVATE_")
    );
}
#[test]
fn private_binding_digest_rejects_unsupported_redirects_and_changes_with_device_selection() {
    let f = Fixture::new();
    let old = microphone_resource_digest(&f.resource).unwrap();
    let mut resource = f.resource.clone();
    resource.parameters.insert("device".into(), json!(1));
    assert_ne!(old, microphone_resource_digest(&resource).unwrap());
    resource
        .parameters
        .insert("path".into(), json!("PRIVATE_REDIRECT"));
    assert!(microphone_resource_digest(&resource).is_err());
    resource.parameters.remove("path");
    resource.parameters.insert("device".into(), json!(true));
    assert!(microphone_resource_digest(&resource).is_err());
}

#[test]
fn reviewed_template_requires_the_exact_current_source_without_inventing_authority() {
    let f = Fixture::new();
    let report = f.inspect();
    let template: crate::workload_leases::definition::Template = serde_json::from_value(json!({
        "schema_version":2,"executor_node":"node","priority":0,
        "source":{"kind":"microphone_energy","resource_id":"node::mic","resource_sha256":report.resources[0].resource_sha256,"adapter_sha256":report.adapter_sha256,
            "parameters":{"sample_rate":16000,"channels":1,"block_frames":80,"max_blocks":100,"activation_level":1000,"deactivation_level":500}},
        "limits":{"max_session_ms":60000,"max_lease_ms":10000,"max_job_memory_mib":128,"max_source_frames":8000},
        "events":{"mode":"activity_transitions","min_interval_ms":100,"max_events":100,"max_event_bytes":1024,"max_total_bytes":65536,"max_pending":4,"overflow":"stop"}
    })).unwrap();
    report.confirms_template(&template).unwrap();
    for field in ["resource_sha256", "adapter_sha256"] {
        let mut value = json!(template);
        value["source"][field] = json!("a".repeat(64));
        assert!(
            report
                .confirms_template(&serde_json::from_value(value).unwrap())
                .is_err()
        );
    }
    let mut stale = report.clone();
    stale.local_bindings_current = false;
    assert!(stale.confirms_template(&template).is_err());
    let mut unavailable = report.clone();
    unavailable.resources[0].available = false;
    assert!(unavailable.confirms_template(&template).is_err());
    assert!(
        !report.execution_authorized && !report.execution_available && !report.listening_enabled
    );
}
