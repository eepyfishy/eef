use super::*;
use crate::workload_leases::definition::{SourceKind, Template};
use serde_json::{Value, json};

fn policy() -> Policy {
    Policy {
        enabled: true,
        ..Default::default()
    }
}
fn camera() -> Template {
    let mut d = crate::workload_leases::definition::camera_tests::camera();
    d.executor_node = "node".into();
    if let crate::workload_leases::definition::Source::CameraMotion { resource_id, .. } =
        &mut d.source
    {
        *resource_id = "node::camera".into();
    }
    d.template()
}
fn microphone() -> Template {
    let mut definition = crate::grants::workload::tests::definition();
    definition.executor_node = "node".into();
    if let crate::workload_leases::definition::Source::MicrophoneEnergy { resource_id, .. } =
        &mut definition.source
    {
        *resource_id = "node::mic".into();
    }
    definition.template()
}
fn camera_report(template: &Template, runtime: &str) -> crate::camera_sources::Report {
    serde_json::from_value(json!({"schema_version":2,"report_type":"camera_workload_sources","success":true,
        "node_id":template.executor_node,"runtime_id":runtime,"source_kind":"camera_motion","adapter_sha256":template.source.adapter_sha256(),
        "resources":[{"resource_id":template.source.resource_id(),"name":"Camera","area":[],"available":true,"resource_sha256":template.source.resource_sha256()}],
        "local_bindings_current":true,"listening_enabled":true,"listening_revision":1,"listening_epoch":2,"listening_blocked":false,
        "saved_camera_permission":true,"applied_camera_permission":true,"python_configured":true,
        "capture_started":false,"execution_authorized":false,"execution_available":true})).unwrap()
}

#[test]
fn source_lists_are_explicit_while_microphone_only_advertisements_keep_the_exact_old_shape() {
    let runtime = uuid::Uuid::new_v4().to_string();
    let mic =
        Advertisement::for_sources(&runtime, policy(), vec![SourceKind::MicrophoneEnergy]).unwrap();
    assert_eq!(
        json!(mic),
        json!({"schema_version":2,"available":true,"runtime_id":runtime,"policy":policy(),
        "source_kind":"microphone_energy","event_delivery":"bounded_metadata","restart_authorized":false,"completion_payload_prefix":true})
    );
    mic.confirms_source(&microphone().source).unwrap();
    assert!(mic.confirms_source(&camera().source).is_err());
    let cam =
        Advertisement::for_sources(&runtime, policy(), vec![SourceKind::CameraMotion]).unwrap();
    assert_eq!(cam.schema_version, 3);
    assert_eq!(json!(cam)["source_kinds"], json!(["camera_motion"]));
    cam.confirms_source(&camera().source).unwrap();
    assert!(cam.confirms_source(&microphone().source).is_err());
    let both = Advertisement::for_sources(
        &runtime,
        policy(),
        vec![SourceKind::MicrophoneEnergy, SourceKind::CameraMotion],
    )
    .unwrap();
    both.confirms_source(&camera().source).unwrap();
    both.confirms_source(&microphone().source).unwrap();
    assert_eq!(
        json!(SourceKind::CameraMotion),
        json!(camera().source.kind())
    );
    assert!(
        json!(Advertisement::unavailable())
            .get("source_kinds")
            .is_none()
    );
}

#[test]
fn unsupported_duplicate_or_noncanonical_source_lists_and_downgrade_claims_fail_closed() {
    let runtime = uuid::Uuid::new_v4().to_string();
    for kinds in [
        vec![SourceKind::CameraMotion, SourceKind::MicrophoneEnergy],
        vec![SourceKind::CameraMotion, SourceKind::CameraMotion],
    ] {
        assert!(Advertisement::for_sources(&runtime, policy(), kinds).is_err());
    }
    let cam =
        Advertisement::for_sources(&runtime, policy(), vec![SourceKind::CameraMotion]).unwrap();
    for (key, value) in [
        ("schema_version", json!(2)),
        ("source_kind", json!("camera_motion")),
        ("available", json!(false)),
        ("source_kinds", json!([])),
        ("restart_authorized", json!(true)),
        ("schema_version", json!(4)),
    ] {
        let mut forged = json!(cam);
        forged[key] = value;
        let parsed: Advertisement = serde_json::from_value(forged).unwrap();
        assert!(parsed.validate().is_err(), "{key}");
    }
    for value in [json!(["unknown_source"]), Value::Null] {
        let mut forged = json!(cam);
        forged["source_kinds"] = value;
        assert!(serde_json::from_value::<Advertisement>(forged).is_err());
    }
    assert!(
        Advertisement::for_sources(&runtime, Policy::default(), vec![SourceKind::CameraMotion])
            .is_err()
    );
    assert!(
        Advertisement::for_sources("stale-runtime", policy(), vec![SourceKind::CameraMotion])
            .is_err()
    );
}

#[test]
fn executable_camera_reports_require_every_camera_readiness_field_and_the_original_runtime() {
    let template = camera();
    let runtime = uuid::Uuid::new_v4().to_string();
    let report = camera_report(&template, &runtime);
    let snapshot = crate::workload_sources::SourceSnapshot::Camera(report.clone());
    snapshot.confirms_execution(&template, &runtime).unwrap();
    assert!(
        snapshot
            .confirms_execution(&template, &uuid::Uuid::new_v4().to_string())
            .is_err()
    );
    for field in [
        "local_bindings_current",
        "listening_enabled",
        "saved_camera_permission",
        "applied_camera_permission",
        "python_configured",
    ] {
        let mut forged = json!(report);
        forged[field] = json!(false);
        let report = serde_json::from_value(forged).unwrap();
        assert!(
            crate::workload_sources::SourceSnapshot::Camera(report)
                .confirms_execution(&template, &runtime)
                .is_err(),
            "{field}"
        );
    }
    let mut blocked = report.clone();
    blocked.listening_blocked = true;
    assert!(
        crate::workload_sources::SourceSnapshot::Camera(blocked)
            .confirms_execution(&template, &runtime)
            .is_err()
    );
    let mut old = report;
    old.schema_version = 1;
    old.execution_available = false;
    old.confirms_template(&template).unwrap();
    assert!(
        crate::workload_sources::SourceSnapshot::Camera(old)
            .confirms_execution(&template, &runtime)
            .is_err()
    );
}

#[test]
fn normalized_source_snapshot_cannot_substitute_a_reviewed_resource_or_another_source_kind() {
    let template = camera();
    let runtime = uuid::Uuid::new_v4().to_string();
    let report = camera_report(&template, &runtime);
    let snapshot = crate::workload_sources::SourceSnapshot::Camera(report);
    for field in ["resource_sha256", "adapter_sha256"] {
        let mut changed = json!(template);
        changed["source"][field] = json!("f".repeat(64));
        assert!(
            snapshot
                .confirms_template(&serde_json::from_value(changed).unwrap())
                .is_err()
        );
    }
    assert!(snapshot.confirms_template(&microphone()).is_err());
}
