use super::*;
use clap::Parser;
#[derive(Parser)]
struct Args {
    #[command(flatten)]
    input: CameraSubmission,
}
fn command() -> CameraCommand {
    Args::try_parse_from([
        "probe",
        "--operation-id",
        "11111111-1111-4111-8111-111111111111",
    ])
    .unwrap()
    .input
    .command()
    .unwrap()
}
fn snapshot() -> crate::camera_sources::Report {
    serde_json::from_value(json!({"schema_version":1,"report_type":"camera_workload_sources","success":true,
        "node_id":"origin","runtime_id":uuid::Uuid::new_v4().to_string(),"source_kind":"camera_motion","adapter_sha256":crate::workload_sources::camera_adapter_digest(),
        "resources":[{"resource_id":"origin::camera","name":"Camera","area":[],"available":true,"resource_sha256":"b".repeat(64)}],
        "local_bindings_current":true,"listening_enabled":false,"listening_revision":0,"listening_epoch":0,"listening_blocked":true,
        "saved_camera_permission":false,"applied_camera_permission":false,"python_configured":false,"capture_started":false,"execution_authorized":false,"execution_available":false})).unwrap()
}
fn draft(input: &CameraInput, concrete: &ConcreteInput) -> crate::request_drafts::DraftRecord {
    crate::request_drafts::DraftRecord {
        schema_version: 1,
        original_text: input.original_text.clone(),
        concrete_input: Some(concrete.input.clone()),
        audio_source: None,
        workload_source: None,
        camera_source: Some(input.clone()),
        proposals: vec![crate::request_drafts::ProposalRevision {
            operation_id: command().operation_id,
            proposal: concrete.proposal().unwrap(),
            allowed_capabilities: vec![
                crate::workload_leases::definition::CAPABILITY.into(),
                "camera.capture".into(),
            ],
            at_ms: 1,
            resolution: None,
            preparation: None,
        }],
        answers: vec![],
    }
}
#[test]
fn camera_intent_resolves_only_public_pins_without_execution_readiness_or_private_endpoints() {
    let command = command();
    let inspected = snapshot();
    let concrete = command.input.resolve("origin", &inspected).unwrap();
    let template = Template::from_params(&concrete.input.params).unwrap();
    assert_eq!(template.executor_node, "origin");
    assert_eq!(template.resource_id(), "origin::camera");
    assert!(matches!(template.source, Source::CameraMotion { .. }));
    assert_eq!(template.limits.max_source_frames, 30);
    assert_eq!(
        command.input.concrete(&template, Some("origin")).unwrap(),
        concrete
    );
    assert!(
        !inspected.execution_available
            && !inspected.saved_camera_permission
            && !inspected.python_configured
    );
    let record = draft(&command.input, &concrete);
    record.validate().unwrap();
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(serialized.contains("camera_source"));
    assert!(!serialized.contains("workload_source") && !serialized.contains("audio_source"));
}
#[test]
fn ambiguous_missing_pending_and_foreign_resource_choices_refuse_instead_of_falling_back() {
    let input = command().input;
    let mut source = snapshot();
    let mut second = source.resources[0].clone();
    second.resource_id = "origin::second".into();
    source.resources.push(second);
    let error = input.resolve("origin", &source).unwrap_err();
    assert_eq!(
        error.downcast_ref::<outcomes::RefusalReason>(),
        Some(&outcomes::RefusalReason::ResourceAmbiguous)
    );
    let mut selected = input.clone();
    selected.camera = Some("origin::second".into());
    assert_eq!(
        Template::from_params(&selected.resolve("origin", &source).unwrap().input.params)
            .unwrap()
            .resource_id(),
        "origin::second"
    );
    source.local_bindings_current = false;
    assert!(selected.resolve("origin", &source).is_err());
    source.local_bindings_current = true;
    for id in ["missing", "other::camera"] {
        selected.camera = Some(id.into());
        assert!(selected.resolve("origin", &source).is_err());
    }
    source.resources.clear();
    assert!(input.resolve("origin", &source).is_err());
}
#[test]
fn camera_choices_cannot_be_relabelled_as_microphone_or_changed_after_resolution() {
    let input = command().input;
    let concrete = input.resolve("origin", &snapshot()).unwrap();
    let template = Template::from_params(&concrete.input.params).unwrap();
    let microphone = crate::grants::workload::tests::definition().template();
    assert!(input.concrete(&microphone, Some("origin")).is_err());
    for field in ["resource_sha256", "adapter_sha256"] {
        let mut altered = json!(template);
        altered["source"][field] = json!("f".repeat(64));
        // A matching retained input alone cannot attest the source; fresh inspection
        // must independently reject a changed binding at compilation/admission.
        assert!(
            snapshot()
                .confirms_template(&serde_json::from_value(altered).unwrap())
                .is_err()
        );
    }
    let mut changed = input.clone();
    changed.parameters.activation_level += 1;
    assert!(changed.concrete(&template, Some("origin")).is_err());
    let mut changed = input;
    changed.source_node = Some("other".into());
    assert!(changed.concrete(&template, Some("origin")).is_err());
}
#[test]
fn durable_camera_intent_is_immutable_and_legacy_reviews_omit_the_new_field() {
    let input = command().input;
    let concrete = input.resolve("origin", &snapshot()).unwrap();
    let record = draft(&input, &concrete);
    for field in ["priority", "original_text", "parameters"] {
        let mut altered = json!(record);
        altered["camera_source"][field] = match field {
            "priority" => json!(1),
            "original_text" => json!("Another request"),
            _ => json!({}),
        };
        assert!(
            serde_json::from_value::<crate::request_drafts::DraftRecord>(altered)
                .map_or(true, |r| r.validate().is_err())
        );
    }
    let mut changed = record.clone();
    changed.camera_source.as_mut().unwrap().camera = Some("other::camera".into());
    assert!(changed.validate().is_err());
    assert!(!record.same_opening(&changed));
    let mut mixed = record.clone();
    let microphone = crate::grants::workload::tests::definition();
    let Source::MicrophoneEnergy { parameters, .. } = microphone.source else {
        unreachable!()
    };
    mixed.workload_source = Some(crate::microphone_submission::MicrophoneInput {
        schema_version: 1,
        original_text: input.original_text.clone(),
        title: input.title.clone(),
        source_node: None,
        microphone: None,
        priority: 0,
        parameters,
        limits: microphone.limits,
        events: microphone.events,
    });
    assert!(
        mixed
            .validate()
            .unwrap_err()
            .to_string()
            .contains("multiple typed source intents")
    );
    let mut legacy = record;
    legacy.camera_source = None;
    legacy.validate().unwrap();
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("camera_source")
    );
}
#[tokio::test]
async fn missing_origin_mailbox_and_wrong_scope_never_save_or_imply_execution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.json");
    crate::setup::write_json(&path, &json!({"node_id":"origin"})).unwrap();
    let service = crate::NodeService::new(path.clone(), "origin".into());
    let before = std::fs::read(&path).unwrap();
    let command = command();
    let request = CameraRequest {
        schema_version: 1,
        expected_node_id: "origin".into(),
        command: command.clone(),
    };
    let result = service.camera_request(request).await.unwrap();
    assert!(confirmed_node(&result, "origin", &command));
    assert_eq!(result["error_code"], "camera_authoring_not_sent");
    assert_eq!(result["outcome_unknown"], false);
    assert!(
        service
            .camera_request(CameraRequest {
                schema_version: 1,
                expected_node_id: "other".into(),
                command: command.clone()
            })
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn uncertain_and_refused_reports_cannot_be_promoted_to_success_or_crossed_with_other_receipts() {
    let command = command();
    let unavailable = unavailable("origin", command.clone(), true);
    assert!(confirmed_node(&unavailable, "origin", &command));
    for field in [
        "success",
        "coordinator_acknowledged",
        "execution_authorized",
        "dispatched",
        "automatically_retried",
    ] {
        let mut forged = unavailable.clone();
        forged[field] = json!(true);
        assert!(!confirmed_node(&forged, "origin", &command));
    }
    let refusal = outcomes::Refusal::new(
        "origin",
        &command,
        outcomes::RefusalReason::ResourceAmbiguous,
    )
    .unwrap();
    let report = json!(NodeReport {
        schema_version: 1,
        report_type: "node_camera_request".into(),
        node_id: "origin".into(),
        command: command.clone(),
        success: false,
        coordinator_acknowledged: true,
        outcome_unknown: false,
        result: None,
        error_code: Some(refusal.reason.code().into()),
        refusal: Some(refusal),
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false,
        note: REFUSAL_NOTE.into()
    });
    assert!(confirmed_node(&report, "origin", &command));
    assert!(render_node(&report).unwrap().contains("--camera"));
    let mut other = command;
    other.operation_id = uuid::Uuid::new_v4().to_string();
    assert!(!confirmed_node(&report, "origin", &other));
}
