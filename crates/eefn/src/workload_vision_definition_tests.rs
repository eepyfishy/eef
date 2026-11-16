use super::*;
use crate::model_selection::Backend;
use crate::workload_leases::definition::{Definition, Source, SourceKind, Template};
use serde_json::json;

pub(crate) fn vision() -> Definition {
    let mut d = crate::workload_leases::definition::camera_tests::camera();
    let Source::CameraMotion {
        resource_id,
        resource_sha256,
        parameters,
        ..
    } = d.source
    else {
        panic!("Camera fixture required")
    };
    d.schema_version = 3;
    d.events.max_total_bytes = 32 * 1024;
    d.source = Source::CameraVision {
        resource_id,
        resource_sha256,
        adapter_sha256: crate::workload_sources::vision_adapter_digest(),
        parameters: Parameters {
            capture: parameters,
            images: Images {
                width: 320,
                height: 240,
                max_image_bytes: 65536,
                jpeg_quality: 70,
                max_images: 1,
                min_interval_ms: 1000,
                trigger: Trigger::Rising,
            },
            model: ModelSelection {
                backend: Backend::Ollama,
                model_id: "selected".into(),
                installed_digest: Some("a".repeat(64)),
            },
            prompt: "Describe movement".into(),
            inference: AnalysisLimits {
                max_tokens: 32,
                max_response_bytes: 8192,
                max_result_bytes: 256,
                timeout_ms: 2000,
            },
        },
    };
    d.validate().unwrap();
    d
}
fn parameters(d: &Definition) -> &Parameters {
    let Source::CameraVision { parameters, .. } = &d.source else {
        panic!("Vision fixture required")
    };
    parameters
}

#[test]
fn vision_definition_requires_a_distinct_version_and_cannot_expand_old_motion_grants() {
    let d = vision();
    assert_eq!(d.source.source_kind(), SourceKind::CameraVision);
    assert_eq!(d.source.kind(), "camera_vision");
    assert_eq!(d.source.capability(), "camera.capture");
    assert_eq!(d.source.max_observations(), 3);
    let mut wrong = d.clone();
    wrong.schema_version = 2;
    assert!(wrong.validate().is_err());
    assert!(wrong.template().validate().is_err());
    let legacy = crate::workload_leases::definition::camera_tests::camera();
    assert_eq!(legacy.schema_version, 2);
    let raw = json!(legacy);
    let legacy_digest = legacy.scope().unwrap().grant_sha256;
    let decoded: Definition = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(decoded.scope().unwrap().grant_sha256, legacy_digest);
    assert_eq!(json!(decoded), raw);
    let mut raised = legacy;
    raised.schema_version = 3;
    assert!(raised.validate().is_err());
    let mut raw = raw;
    raw["source"]["analysis"] = json!({"model":"other"});
    assert!(serde_json::from_value::<Definition>(raw).is_err());
}

#[test]
fn vision_scope_digest_binds_every_model_image_call_and_result_setting() {
    let d = vision();
    let baseline = d.scope().unwrap().grant_sha256;
    for (path, value) in [
        ("/source/parameters/model/model_id", json!("another")),
        (
            "/source/parameters/model/installed_digest",
            json!("b".repeat(64)),
        ),
        ("/source/parameters/prompt", json!("Count people")),
        ("/source/parameters/images/width", json!(160)),
        ("/source/parameters/images/height", json!(120)),
        ("/source/parameters/images/jpeg_quality", json!(65)),
        ("/source/parameters/images/max_images", json!(2)),
        ("/source/parameters/images/max_image_bytes", json!(32768)),
        ("/source/parameters/images/min_interval_ms", json!(2000)),
        (
            "/source/parameters/images/trigger",
            json!("active_interval"),
        ),
        ("/source/parameters/inference/max_tokens", json!(64)),
        (
            "/source/parameters/inference/max_response_bytes",
            json!(16384),
        ),
        ("/source/parameters/inference/max_result_bytes", json!(512)),
        ("/source/parameters/inference/timeout_ms", json!(1000)),
    ] {
        let mut raw = json!(d);
        *raw.pointer_mut(path).unwrap() = value;
        let changed: Definition = serde_json::from_value(raw).unwrap();
        assert_ne!(
            changed.scope().unwrap().grant_sha256,
            baseline,
            "Unbound {path}"
        );
    }
    let mut other = d.clone();
    if let Source::CameraVision { parameters, .. } = &mut other.source {
        parameters.model.backend = Backend::Llamacpp;
        parameters.model.installed_digest = None;
    }
    assert_ne!(other.scope().unwrap().grant_sha256, baseline);
}

#[test]
fn vision_private_binding_contains_only_capture_image_scope_and_owner_device() {
    let d = vision();
    let bytes = crate::workload_sources::vision_source_bytes();
    let resource = crate::workload_leases::definition::camera_tests::resource();
    let request = d.bound_python_parameters(&resource, &bytes).unwrap();
    assert_eq!(request["device"], resource.parameters["device"]);
    assert_eq!(request["max_frames"], 3);
    assert_eq!(request["analysis"], json!(parameters(&d).images));
    for key in [
        "model",
        "prompt",
        "inference",
        "backend",
        "installed_digest",
        "context",
    ] {
        assert!(request.get(key).is_none());
    }
    assert!(
        !serde_json::to_string(&d)
            .unwrap()
            .contains("PRIVATE_CAMERA_PASSWORD")
    );
    assert!(
        d.bound_python_parameters(&resource, crate::workload_sources::CAMERA_SOURCE)
            .is_err()
    );
    let mut changed = resource.clone();
    changed.parameters.insert("device".into(), json!(2));
    assert!(d.bound_python_parameters(&changed, &bytes).is_err());
}

#[test]
fn vision_budgets_are_derived_from_single_use_images_and_exact_per_call_limits() {
    let mut p = parameters(&vision()).clone();
    p.images.max_images = 3;
    assert_eq!(p.max_calls(), 3);
    assert_eq!(p.max_reserved_output_tokens().unwrap(), 96);
    assert_eq!(p.max_total_result_bytes().unwrap(), 768);
    p.images.max_images = u64::MAX;
    assert!(p.max_reserved_output_tokens().is_err());
    assert!(p.max_total_result_bytes().is_err());
}

#[test]
fn vision_parameters_reject_widened_unknown_and_incompatible_settings() {
    let p = parameters(&vision()).clone();
    for (path, value) in [
        ("/images/width", json!(641)),
        ("/images/height", json!(481)),
        ("/images/max_image_bytes", json!(2 * 1024 * 1024 + 1)),
        ("/images/jpeg_quality", json!(0)),
        ("/images/max_images", json!(4)),
        ("/images/min_interval_ms", json!(3_600_001)),
        ("/prompt", json!("")),
        ("/prompt", json!("x".repeat(4097))),
        ("/inference/timeout_ms", json!(10001)),
        ("/inference/max_tokens", json!(0)),
    ] {
        let mut raw = json!(p);
        *raw.pointer_mut(path).unwrap() = value;
        let parsed: Parameters = serde_json::from_value(raw).unwrap();
        assert!(parsed.validate().is_err(), "Widened {path} accepted");
    }
    let mut too_small = p.clone();
    too_small.capture.width = 160;
    assert!(too_small.validate().is_err());
    for value in [json!(-1), json!(true), json!(1.5), json!("1")] {
        let mut raw = json!(p);
        raw["images"]["max_images"] = value;
        assert!(serde_json::from_value::<Parameters>(raw).is_err());
    }
    for target in ["images", "model", "inference", "capture"] {
        let mut raw = json!(p);
        raw[target]["unknown"] = json!(true);
        assert!(serde_json::from_value::<Parameters>(raw).is_err());
    }
}

#[test]
fn metadata_protocol_and_advertisement_never_accept_a_vision_scope() {
    use crate::workload_network::{Advertisement, Offer};
    let d = vision();
    let policy = crate::workload_leases::Policy {
        enabled: true,
        ..Default::default()
    };
    assert!(
        Advertisement::for_sources(
            &uuid::Uuid::new_v4().to_string(),
            policy,
            vec![SourceKind::CameraVision]
        )
        .is_err()
    );
    let runtime = uuid::Uuid::new_v4().to_string();
    let offer = Offer {
        kind: "execute_workload".into(),
        schema_version: 2,
        id: d.grant_id.clone(),
        expected_runtime_id: runtime.clone(),
        definition: d.clone(),
    };
    assert!(
        offer
            .validate(&d.issuer_id, &d.session_id, &d.executor_node, &runtime)
            .is_err()
    );
    let encoded = serde_json::to_vec(&d.template()).unwrap();
    let template: Template = serde_json::from_slice(&encoded).unwrap();
    template.validate().unwrap();
}
