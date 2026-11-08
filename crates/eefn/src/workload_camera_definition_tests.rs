use super::*;
use crate::python::stream::StreamObservation;
use crate::workload_events::{Event, Filter, Kind};
use std::time::{Duration, Instant};

pub(crate) fn resource() -> crate::context::Resource {
    serde_json::from_value(json!({"id":"camera","capability":"camera.capture",
        "parameters":{"device":"rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid:554/live"}}))
    .unwrap()
}
pub(crate) fn camera() -> Definition {
    let mut d = crate::grants::workload::tests::definition();
    d.source = Source::CameraMotion {
        resource_id: crate::context::resource_id(&d.executor_node, "camera"),
        resource_sha256: camera_resource_digest(&resource()).unwrap(),
        adapter_sha256: crate::workload_sources::camera_adapter_digest(),
        parameters: parameters(),
    };
    d.limits.max_source_frames = 3;
    d.validate().unwrap();
    d
}
fn parameters() -> MotionParameters {
    MotionParameters {
        width: 640,
        height: 480,
        sample_width: 64,
        sample_height: 48,
        max_frames: 3,
        interval_ms: 200,
        activation_level: 15,
        deactivation_level: 5,
        io_timeout_ms: 2000,
        max_duration_ms: 10000,
    }
}
fn observation(
    sequence: u64,
    active: Option<bool>,
    level: Option<u16>,
    gap: bool,
) -> StreamObservation {
    StreamObservation {
        sequence,
        source_frames: sequence,
        active,
        level,
        discontinuity: gap,
    }
}
#[test]
fn camera_definition_binds_private_source_without_advertising_endpoint_or_grant() {
    let d = camera();
    let r = resource();
    let request = d
        .bound_python_parameters(&r, crate::workload_sources::CAMERA_SOURCE)
        .unwrap();
    assert_eq!(request["device"], r.parameters["device"]);
    assert_eq!(request["max_frames"], 3);
    let mut public = request.clone();
    public.as_object_mut().unwrap().remove("device");
    assert_eq!(
        serde_json::from_value::<MotionParameters>(public).unwrap(),
        parameters()
    );
    assert!(
        !serde_json::to_string(&d)
            .unwrap()
            .contains("PRIVATE_CAMERA_PASSWORD")
    );
    assert_eq!(d.source.capability(), "camera.capture");
    assert_eq!(d.source.frames_per_observation(), 1);
    let mut disabled = r.clone();
    disabled.available = false;
    assert!(
        d.bound_python_parameters(&disabled, crate::workload_sources::CAMERA_SOURCE)
            .is_err()
    );
    assert!(
        d.bound_python_parameters(&r, b"substituted source")
            .is_err()
    );
    let mut changed = r.clone();
    changed.parameters.insert("device".into(), json!(1));
    assert!(
        d.bound_python_parameters(&changed, crate::workload_sources::CAMERA_SOURCE)
            .is_err()
    );
    let mut missing = r.clone();
    missing.parameters.clear();
    let mut default = d.clone();
    if let Source::CameraMotion {
        resource_sha256, ..
    } = &mut default.source
    {
        *resource_sha256 = camera_resource_digest(&missing).unwrap();
    }
    assert_eq!(
        default
            .bound_python_parameters(&missing, crate::workload_sources::CAMERA_SOURCE)
            .unwrap()["device"],
        0
    );
}
#[test]
fn camera_parameters_and_session_budget_reject_widening_or_authority_fields() {
    for (key, value) in [
        ("width", json!(1921)),
        ("height", json!(119)),
        ("sample_width", json!(161)),
        ("sample_height", json!(7)),
        ("max_frames", json!(100001)),
        ("interval_ms", json!(99)),
        ("activation_level", json!(256)),
        ("deactivation_level", json!(16)),
        ("io_timeout_ms", json!(10001)),
        ("max_duration_ms", json!(100)),
        ("width", json!(true)),
        ("device", json!("private")),
        ("approved", json!(true)),
    ] {
        let mut raw = json!(parameters());
        raw[key] = value;
        assert!(
            !serde_json::from_value::<MotionParameters>(raw)
                .ok()
                .is_some_and(|p| p.validate().is_ok()),
            "{key}"
        );
    }
    let mut d = camera();
    d.limits.max_source_frames = 2;
    assert!(d.validate().is_err());
    let mut d = camera();
    d.limits.max_lease_ms = 1000;
    d.limits.max_session_ms = 9999;
    assert!(d.validate().is_err());
    let mut raw = json!(camera());
    raw["source"]["parameters"]["endpoint"] = json!("rtsp://unreviewed/live");
    assert!(serde_json::from_value::<Definition>(raw).is_err());
}
#[test]
fn camera_resource_binding_refuses_files_protocols_and_private_adapter_overrides() {
    for device in [
        json!(true),
        json!(-1),
        json!(1024),
        json!("movie.mp4"),
        json!("file:///private"),
        json!("https://camera.invalid/live"),
        json!("rtsp:///missing"),
        json!("rtsp://camera.invalid:0/live"),
        json!("rtsp://camera.invalid/live#fragment"),
        json!("rtsp://camera.invalid/\nprivate"),
    ] {
        let mut r = resource();
        r.parameters.insert("device".into(), device);
        assert!(camera_resource_digest(&r).is_err());
    }
    let mut r = resource();
    r.parameters.insert("record_to".into(), json!("forbidden"));
    assert!(camera_resource_digest(&r).is_err());
    let mut r = resource();
    r.capability = "audio.capture".into();
    assert!(camera_resource_digest(&r).is_err());
}
#[test]
fn camera_filter_and_payload_prefix_keep_motion_distinct_from_microphone_activity() {
    let d = camera();
    let mut filter = Filter::new(&d).unwrap();
    let start = Instant::now();
    let first = filter
        .observe(observation(1, None, None, false), start)
        .unwrap()
        .unwrap();
    let second = filter
        .observe(
            observation(2, Some(true), Some(20), false),
            start + Duration::from_millis(200),
        )
        .unwrap()
        .unwrap();
    assert!(
        filter
            .observe(
                observation(3, Some(true), Some(25), false),
                start + Duration::from_millis(400)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(first.kind, Kind::CameraMotion);
    assert_eq!(first.source_frames, 1);
    assert_eq!(second.source_frames, 2);
    second.follows(&d, Some(&first)).unwrap();
    let summary =
        crate::workload_network::summarize_events(&d, &[first.clone(), second.clone()]).unwrap();
    let seal = filter
        .seal(&crate::python::stream::StreamSummary {
            stream_id: d.grant_id.clone(),
            observations: 3,
            source_frames: 3,
        })
        .unwrap();
    assert!(seal.confirms(2, summary.bytes, &second));
    let mut audio = first.clone();
    audio.kind = Kind::MicrophoneActivity;
    assert!(audio.validate(&d).is_err());
    assert!(crate::workload_network::summarize_events(&d, &[audio]).is_err());
    let mut high = second.clone();
    high.level = Some(256);
    assert!(high.validate(&d).is_err());
    let mut wrong = first.clone();
    wrong.source_frames = 80;
    assert!(wrong.validate(&d).is_err());
    let mut raw = json!(first);
    raw["image_base64"] = json!("forbidden");
    assert!(serde_json::from_value::<Event>(raw).is_err());
}
#[test]
fn camera_discontinuity_and_invalid_observation_cannot_mint_a_completion_seal() {
    let d = camera();
    let start = Instant::now();
    let mut filter = Filter::new(&d).unwrap();
    filter
        .observe(observation(1, None, None, false), start)
        .unwrap();
    let gap = filter
        .observe(
            observation(2, None, None, true),
            start + Duration::from_millis(200),
        )
        .unwrap()
        .unwrap();
    assert!(gap.discontinuity);
    assert_eq!(gap.active, None);
    gap.validate(&d).unwrap();
    assert!(
        filter
            .observe(
                observation(3, Some(true), Some(300), false),
                start + Duration::from_millis(400)
            )
            .is_err()
    );
    assert!(
        filter
            .observe(
                observation(3, Some(true), Some(20), false),
                start + Duration::from_millis(600)
            )
            .is_err()
    );
    assert!(
        filter
            .seal(&crate::python::stream::StreamSummary {
                stream_id: d.grant_id.clone(),
                observations: 2,
                source_frames: 2
            })
            .is_err()
    );
}
#[test]
fn microphone_serialization_and_observation_shape_remain_unchanged() {
    let d = crate::grants::workload::tests::definition();
    assert_eq!(
        json!(d.source),
        json!({"kind":"microphone_energy","resource_id":"owner::microphone",
        "resource_sha256":"d".repeat(64),"adapter_sha256":"e".repeat(64),
        "parameters":{"sample_rate":16000,"channels":1,"block_frames":80,"max_blocks":100,
            "activation_level":1000,"deactivation_level":500}})
    );
    assert_eq!(d.source.capability(), "audio.capture");
    assert_eq!(d.source.frames_per_observation(), 80);
    assert_eq!(d.source.max_observations(), 100);
    assert_eq!(d.source.max_level(), 32768);
    let input = crate::request_planning::PlanInput {
        schema_version: 1,
        template: TEMPLATE.into(),
        params: json!({"workload":d.template()}),
        constraints: json!({}),
    };
    assert_eq!(
        crate::request_planning::concrete_capabilities(&input).unwrap(),
        ["workload.session", "audio.capture"]
    );
    let camera = camera();
    let mut input = input;
    input.params = json!({"workload":camera.template()});
    assert_eq!(
        crate::request_planning::concrete_capabilities(&input).unwrap(),
        ["workload.session", "camera.capture"]
    );
}
