use super::*;
use clap::Parser;
#[derive(Parser)]
struct Args {
    #[command(flatten)]
    input: MicrophoneSubmission,
}
fn command() -> MicrophoneCommand {
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
fn snapshot(node: &str) -> crate::workload_sources::Report {
    serde_json::from_value(json!({"schema_version":2,"report_type":"workload_sources","success":true,
        "node_id":node,"runtime_id":uuid::Uuid::new_v4().to_string(),"source_kind":"microphone_energy",
        "adapter_sha256":"a".repeat(64),"resources":[{"resource_id":format!("{node}::mic"),"name":"Microphone",
            "area":[],"available":true,"resource_sha256":"b".repeat(64)}],"local_bindings_current":true,
        "listening_enabled":false,"listening_revision":0,"listening_epoch":0,"listening_blocked":true,
        "saved_microphone_permission":false,"applied_microphone_permission":false,"python_configured":false,
        "capture_started":false,"execution_authorized":false,"execution_available":false})).unwrap()
}
fn draft(input: &MicrophoneInput, concrete: &ConcreteInput) -> crate::request_drafts::DraftRecord {
    crate::request_drafts::DraftRecord {
        schema_version: 1,
        original_text: input.original_text.clone(),
        concrete_input: Some(concrete.input.clone()),
        audio_source: None,
        workload_source: Some(input.clone()),
        proposals: vec![crate::request_drafts::ProposalRevision {
            operation_id: uuid::Uuid::new_v4().to_string(),
            proposal: concrete.proposal().unwrap(),
            allowed_capabilities: vec![crate::workload_leases::definition::CAPABILITY.into(), "audio.capture".into()],
            at_ms: 1,
            resolution: None,
            preparation: None,
        }],
        answers: vec![],
    }
}
#[test]
fn microphone_selection_is_unique_bound_and_never_needs_execution_readiness() {
    let command = command();
    let mut source = snapshot("origin");
    let concrete = command.input.resolve("origin", &source).unwrap();
    let t = Template::from_params(&concrete.input.params).unwrap();
    assert_eq!(t.resource_id(), "origin::mic");
    assert_eq!(t.limits.max_source_frames, 160_000);
    assert!(
        !source.execution_available
            && !source.saved_microphone_permission
            && !source.python_configured
    );
    let mut second = source.resources[0].clone();
    second.resource_id = "origin::other".into();
    source.resources.push(second);
    assert!(command.input.resolve("origin", &source).is_err());
    let mut explicit = command.input.clone();
    explicit.microphone = Some("mic".into());
    assert_eq!(
        explicit.resolve("origin", &source).unwrap().input,
        concrete.input
    );
    explicit.microphone = Some("origin::other".into());
    assert_eq!(
        Template::from_params(&explicit.resolve("origin", &source).unwrap().input.params)
            .unwrap()
            .resource_id(),
        "origin::other"
    );
    source.local_bindings_current = false;
    assert!(explicit.resolve("origin", &source).is_err());
}
#[test]
fn microphone_origin_and_executor_are_distinct_and_retained_choices_cannot_drift() {
    let mut input = command().input;
    input.source_node = Some("living-room".into());
    let concrete = input.resolve("bedroom", &snapshot("living-room")).unwrap();
    let mut t = Template::from_params(&concrete.input.params).unwrap();
    assert_eq!(input.concrete(&t, Some("bedroom")).unwrap(), concrete);
    input.source_node = None;
    assert!(input.concrete(&t, Some("bedroom")).is_err());
    assert!(input.resolve("bedroom", &snapshot("living-room")).is_err());
    input.source_node = Some("living-room".into());
    input.microphone = Some("bedroom::mic".into());
    assert!(input.validate().is_err());
    input.microphone = None;
    t.limits.max_session_ms -= 1;
    assert!(input.concrete(&t, Some("bedroom")).is_err());
}
#[test]
fn microphone_intent_and_snapshot_refuse_authority_fields_and_invalid_bounds() {
    let input = command().input;
    for field in ["execute", "approve", "retry", "python", "code", "path"] {
        let mut value = json!(input);
        value[field] = json!(true);
        assert!(serde_json::from_value::<MicrophoneInput>(value).is_err());
    }
    for fault in 0..5 {
        let mut changed = input.clone();
        match fault {
            0 => changed.parameters.block_frames = 0,
            1 => changed.limits.max_source_frames = 1,
            2 => changed.events.max_pending = 0,
            3 => changed.limits.max_lease_ms = changed.limits.max_session_ms + 1,
            _ => changed.microphone = Some("../private".into()),
        }
        assert!(changed.validate().is_err());
    }
    for fault in 0..4 {
        let mut source = snapshot("origin");
        match fault {
            0 => source.capture_started = true,
            1 => source.execution_authorized = true,
            2 => source.resources[0].available = false,
            _ => source.resources[0].resource_sha256 = None,
        }
        assert!(input.resolve("origin", &source).is_err());
    }
}
#[test]
fn microphone_typed_history_validates_intent_and_preserves_legacy_absence() {
    let input = command().input;
    let concrete = input.resolve("origin", &snapshot("origin")).unwrap();
    let retained = draft(&input, &concrete);
    retained.validate().unwrap();
    let mut changed = retained.clone();
    changed
        .workload_source
        .as_mut()
        .unwrap()
        .parameters
        .sample_rate = 8000;
    assert!(changed.validate().is_err());
    let mut changed = retained.clone();
    changed.concrete_input.as_mut().unwrap().params["workload"]["limits"]["max_session_ms"] =
        json!(59000);
    assert!(changed.validate().is_err());
    let mut legacy = retained;
    legacy.workload_source = None;
    legacy.validate().unwrap();
    let raw = serde_json::to_string(&legacy).unwrap();
    assert!(!raw.contains("workload_source"));
    let old: crate::request_drafts::DraftRecord = serde_json::from_str(&raw).unwrap();
    assert_eq!(serde_json::to_string(&old).unwrap(), raw);
}
#[test]
fn microphone_unconfirmed_reports_bind_node_operation_and_never_claim_effects() {
    let command = command();
    for unknown in [false, true] {
        let report = unavailable("origin", command.clone(), unknown);
        assert!(confirmed_node(&report, "origin", &command));
        assert!(!confirmed_node(&report, "another", &command));
        let text = render_node(&report).unwrap();
        assert!(text.contains(&command.operation_id) && text.contains("eefn jobs find"));
        for field in [
            "execution_authorized",
            "dispatched",
            "automatically_retried",
        ] {
            let mut changed = report.clone();
            changed[field] = json!(true);
            assert!(!confirmed_node(&changed, "origin", &command));
        }
    }
}
#[tokio::test]
async fn microphone_authoring_offline_is_not_sent_and_never_creates_a_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.json");
    std::fs::write(&path, b"{\"node_id\":\"origin\"}").unwrap();
    let before = std::fs::read(&path).unwrap();
    let service = crate::NodeService::new(path.clone(), "origin".into());
    let command = command();
    let result = service
        .microphone_request(MicrophoneRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert!(confirmed_node(&result, "origin", &command));
    assert_eq!(result["success"], false);
    assert_eq!(result["outcome_unknown"], false);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(service.live.lock().unwrap().get("runtime_id").is_none());
    assert!(
        service
            .microphone_request(MicrophoneRequest {
                schema_version: 1,
                expected_node_id: "another".into(),
                command
            })
            .await
            .is_err()
    );
}
