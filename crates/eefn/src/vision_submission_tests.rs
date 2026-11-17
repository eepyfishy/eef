use super::*;
use clap::Parser;
use serde_json::{Value, json};
#[derive(Parser)]
struct Args {
    #[command(flatten)]
    input: VisionSubmission,
}
fn argv() -> Vec<String> {
    vec![
        "probe".into(),
        "--model".into(),
        "selected".into(),
        "--backend".into(),
        "ollama".into(),
        "--model-digest".into(),
        "a".repeat(64),
        "--prompt".into(),
        "Describe the movement".into(),
    ]
}
fn command() -> CameraCommand {
    Args::try_parse_from(argv())
        .unwrap()
        .input
        .command()
        .unwrap()
}
fn snapshot() -> crate::vision_sources::Report {
    let runtime = uuid::Uuid::new_v4().to_string();
    serde_json::from_value(json!({"schema_version":1,"report_type":"vision_workload_sources","success":true,
        "node_id":"origin","runtime_id":runtime,"source_kind":"camera_vision","adapter_sha256":crate::workload_sources::vision_adapter_digest(),
        "camera":{"schema_version":1,"report_type":"camera_workload_sources","success":true,
            "node_id":"origin","runtime_id":runtime,"source_kind":"camera_motion","adapter_sha256":crate::workload_sources::camera_adapter_digest(),
            "resources":[{"resource_id":"origin::camera","name":"Camera","area":[],"available":true,"resource_sha256":"b".repeat(64)}],
            "local_bindings_current":true,"listening_enabled":false,"listening_revision":0,"listening_epoch":0,"listening_blocked":true,
            "saved_camera_permission":false,"applied_camera_permission":false,"python_configured":false,
            "capture_started":false,"execution_authorized":false,"execution_available":false},
        "models":[{"backend":"ollama","model_id":"selected","local_endpoint_configured":true}],
        "model_bindings_current":true,"workload_enabled":false,"vision_enabled":false,"policy_revision":0,"policy_epoch":0,
        "policy_blocked":false,"configuration_ready":false,"capture_started":false,"model_contacted":false,
        "execution_authorized":false,"execution_available":false})).unwrap()
}
#[test]
fn vision_cli_defaults_reserve_metadata_and_text_without_expanding_fresh_owner_limits() {
    let command = command();
    assert_eq!(command.input.schema_version, 2);
    assert_eq!(command.input.events.max_total_bytes, 32768);
    let analysis = command.input.vision.as_ref().unwrap();
    assert_eq!(analysis.model.model_id, "selected");
    assert_eq!(
        analysis.model.installed_digest.as_deref(),
        Some("a".repeat(64).as_str())
    );
    let report = snapshot();
    let concrete = command.input.resolve_vision("origin", &report).unwrap();
    let template =
        crate::workload_leases::definition::Template::from_params(&concrete.input.params).unwrap();
    assert_eq!(concrete.input.schema_version, 2);
    concrete.input.validate_for_new_work().unwrap();
    assert_eq!(template.schema_version, 3);
    assert!(
        template.max_retained_result_bytes().unwrap()
            <= crate::grants::GrantPolicy::default().max_result_bytes
    );
    assert_eq!(template.model_binding(), Some(("selected", "ollama")));
    assert_eq!(
        crate::request_planning::concrete_capabilities(&concrete.input).unwrap(),
        ["workload.session", "camera.capture", "vlm.analyze"]
    );
    assert!(!report.execution_available && !report.configuration_ready && !report.model_contacted);
}
#[test]
fn vision_cli_requires_explicit_model_prompt_and_backend_and_has_no_execution_or_source_url_flags()
{
    for key in ["--model", "--backend", "--model-digest", "--prompt"] {
        let mut args = argv();
        let at = args.iter().position(|v| v == key).unwrap();
        args.drain(at..at + 2);
        assert!(Args::try_parse_from(args).is_err(), "{key}");
    }
    for flag in [
        "--approve",
        "--execute",
        "--device",
        "--rtsp",
        "--file",
        "--retry",
    ] {
        let mut args = argv();
        args.push(flag.into());
        assert!(Args::try_parse_from(args).is_err(), "{flag}");
    }
    let mut args = argv();
    let backend = args.iter().position(|v| v == "ollama").unwrap();
    args[backend] = "llamacpp".into();
    assert!(
        Args::try_parse_from(args.clone())
            .unwrap()
            .input
            .command()
            .is_err(),
        "A provider digest cannot be silently ignored for a different backend"
    );
    let at = args.iter().position(|v| v == "--model-digest").unwrap();
    args.drain(at..at + 2);
    let local = Args::try_parse_from(args).unwrap().input.command().unwrap();
    assert!(local.input.vision.unwrap().model.installed_digest.is_none());
    let mut invalid = argv();
    invalid.extend(["--max-images".into(), "101".into()]);
    assert!(
        Args::try_parse_from(invalid)
            .unwrap()
            .input
            .command()
            .is_err()
    );
}
#[test]
fn vision_review_requires_exact_applied_camera_and_model_and_never_uses_camera_only_resolution() {
    let command = command();
    let report = snapshot();
    let concrete = command.input.resolve_vision("origin", &report).unwrap();
    assert!(command.input.resolve("origin", &report.camera).is_err());
    let template =
        crate::workload_leases::definition::Template::from_params(&concrete.input.params).unwrap();
    for (pointer, value) in [
        ("/vision/prompt", json!("Count faces instead")),
        ("/vision/model/model_id", json!("another")),
        ("/vision/model/installed_digest", json!("c".repeat(64))),
        ("/vision/images/max_images", json!(2)),
        ("/vision/inference/max_result_bytes", json!(2048)),
    ] {
        let mut changed = json!(command.input);
        *changed.pointer_mut(pointer).unwrap() = value;
        let changed: crate::camera_submission::CameraInput =
            serde_json::from_value(changed).unwrap();
        changed.validate().unwrap();
        assert!(
            changed.concrete(&template, Some("origin")).is_err(),
            "{pointer}"
        );
    }
    let mut missing = report.clone();
    missing.models.clear();
    assert_eq!(
        *command
            .input
            .resolve_vision("origin", &missing)
            .unwrap_err()
            .downcast_ref::<crate::camera_submission::outcomes::RefusalReason>()
            .unwrap(),
        crate::camera_submission::outcomes::RefusalReason::VisionModelUnavailable
    );
    let mut pending = report.clone();
    pending.model_bindings_current = false;
    assert!(command.input.resolve_vision("origin", &pending).is_err());
    assert!(
        command
            .input
            .resolve_vision("other-origin", &report)
            .is_err()
    );
}
#[test]
fn legacy_camera_intent_cannot_acquire_vision_parameters_without_distinct_version_and_review() {
    let command = command();
    let mut wrong = json!(command.input);
    wrong["schema_version"] = json!(1);
    assert!(
        serde_json::from_value::<crate::camera_submission::CameraInput>(wrong)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut missing = json!(command.input);
    missing.as_object_mut().unwrap().remove("vision");
    assert!(
        serde_json::from_value::<crate::camera_submission::CameraInput>(missing.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    missing["schema_version"] = json!(1);
    let legacy: crate::camera_submission::CameraInput = serde_json::from_value(missing).unwrap();
    legacy.validate().unwrap();
    assert!(json!(legacy).get("vision").is_none());
    assert!(legacy.resolve_vision("origin", &snapshot()).is_err());
    let current = command.input.resolve_vision("origin", &snapshot()).unwrap();
    let mut historical = current.input.clone();
    historical.schema_version = 1;
    historical.validate().unwrap();
    assert_eq!(
        crate::request_planning::concrete_capabilities(&historical).unwrap(),
        ["workload.session", "camera.capture"],
        "Historical capability bytes stay unchanged"
    );
    assert!(historical.validate_for_new_work().is_err());
    let mut metadata = legacy.resolve("origin", &snapshot().camera).unwrap().input;
    metadata.validate_for_new_work().unwrap();
    metadata.schema_version = 2;
    assert!(
        metadata.validate().is_err(),
        "Metadata input cannot borrow the vision review version"
    );
    let mut unknown = json!(command.input);
    unknown["vision"]["auto_download"] = Value::Bool(true);
    assert!(serde_json::from_value::<crate::camera_submission::CameraInput>(unknown).is_err());
}
