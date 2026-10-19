use super::*;

pub(crate) fn input() -> AudioInput {
    AudioInput {
        schema_version: 1,
        original_text: "PRIVATE_AUDIO_INTENT".into(),
        title: "Finite audio".into(),
        source_node: None,
        destination_node: "destination".into(),
        duration_ms: 500,
        sample_rate: 16000,
        channels: 1,
        source_transport: None,
        destination_transport: None,
        microphone: None,
        speaker: None,
        limits: default_limits(),
    }
}
fn endpoints() -> (EndpointResources, EndpointResources) {
    let endpoint = |node: &str, cap: &str, id: &str| EndpointResources {
        node_id: node.into(),
        finite_audio: true,
        can_receive: node == "destination",
        resources: vec![
            ResourceChoice {
                id: format!("{node}::transport"),
                capability: "peer.transfer".into(),
                available: true,
            },
            ResourceChoice {
                id: format!("{node}::{id}"),
                capability: cap.into(),
                available: true,
            },
        ],
    };
    (
        endpoint("source", "audio.capture", "mic"),
        endpoint("destination", "audio.play", "speaker"),
    )
}
fn draft(input: &AudioInput, concrete: &ConcreteInput) -> crate::request_drafts::DraftRecord {
    crate::request_drafts::DraftRecord {
        schema_version: 1,
        original_text: concrete.original_text.clone(),
        concrete_input: Some(concrete.input.clone()),
        audio_source: Some(input.clone()),
        proposals: vec![crate::request_drafts::ProposalRevision {
            operation_id: uuid::Uuid::new_v4().to_string(),
            proposal: concrete.proposal().unwrap(),
            allowed_capabilities: crate::request_planning::concrete_capabilities(&concrete.input)
                .unwrap(),
            at_ms: 1,
            resolution: None,
            preparation: None,
        }],
        answers: vec![],
    }
}
#[test]
fn audio_authoring_resolves_only_exact_nodes_format_and_unambiguous_resources() {
    let source = input();
    let (a, b) = endpoints();
    let concrete = source.resolve("source", &a, &b).unwrap();
    let pair = Pair::from_params(&concrete.input.params).unwrap();
    assert_eq!(pair.payload_bytes, 17026);
    assert!(pair.payload_sha256.is_none());
    assert_eq!(pair.source.resource_id, "source::transport");
    assert_eq!(
        pair.audio.as_ref().unwrap().source_resource_id,
        "source::mic"
    );
    assert_eq!(
        concrete.proposal().unwrap().suggested_capabilities,
        vec!["peer.transfer", "audio.capture", "audio.play"]
    );
    assert_eq!(source.concrete(&pair, Some("source")).unwrap(), concrete);
    assert!(source.concrete(&pair, Some("other-origin")).is_err());
    let mut wrong = source.clone();
    wrong.source_node = Some("another-node".into());
    assert!(wrong.resolve("source", &a, &b).is_err());
    let mut no_receive = b.clone();
    no_receive.can_receive = false;
    assert!(source.resolve("source", &a, &no_receive).is_err());
    let mut old = a.clone();
    old.finite_audio = false;
    assert!(source.resolve("source", &old, &b).is_err());
    let mut self_target = source.clone();
    self_target.destination_node = "source".into();
    assert!(self_target.resolve("source", &a, &a).is_err());
}
#[test]
fn audio_authoring_never_substitutes_disabled_ambiguous_or_foreign_resources() {
    let mut source = input();
    let (mut a, b) = endpoints();
    a.resources.push(ResourceChoice {
        id: "source::second-mic".into(),
        capability: "audio.capture".into(),
        available: true,
    });
    assert!(source.resolve("source", &a, &b).is_err());
    source.microphone = Some("source::second-mic".into());
    let concrete = source.resolve("source", &a, &b).unwrap();
    assert_eq!(
        concrete.input.params["transfer"]["audio"]["source_resource_id"],
        "source::second-mic"
    );
    a.resources[2].available = false;
    assert!(
        source.resolve("source", &a, &b).is_err(),
        "never fall back to another active microphone"
    );
    source.microphone = Some("other::mic".into());
    assert!(source.resolve("source", &a, &b).is_err());
    let (mut a, b) = endpoints();
    a.resources[1].id = "other::mic".into();
    assert!(source.resolve("source", &a, &b).is_err());
}
#[test]
fn audio_source_rejects_invalid_format_limits_and_authority_fields() {
    for modify in [
        |i: &mut AudioInput| i.duration_ms = 99,
        |i: &mut AudioInput| i.duration_ms = 10001,
        |i: &mut AudioInput| i.channels = 3,
        |i: &mut AudioInput| i.sample_rate = 96000,
        |i: &mut AudioInput| i.limits.max_wire_bytes = 1,
        |i: &mut AudioInput| i.limits.max_frame_bytes = 511,
        |i: &mut AudioInput| i.limits.max_frames = 1,
        |i: &mut AudioInput| i.microphone = Some("../device".into()),
    ] {
        let mut source = input();
        modify(&mut source);
        assert!(source.validate().is_err());
    }
    let mut value = json!(input());
    value["permissions"] = json!(true);
    assert!(serde_json::from_value::<AudioInput>(value).is_err());
    let mut value = json!(input());
    value["execute"] = json!(true);
    assert!(serde_json::from_value::<AudioInput>(value).is_err());
}
#[test]
fn retained_audio_intent_is_consistent_with_initial_proposal_and_concrete_input() {
    let source = input();
    let (a, b) = endpoints();
    let concrete = source.resolve("source", &a, &b).unwrap();
    let saved = draft(&source, &concrete);
    saved.validate().unwrap();
    let mut changed = saved.clone();
    changed.audio_source.as_mut().unwrap().duration_ms += 100;
    assert!(changed.validate().is_err());
    let mut changed = saved.clone();
    changed.original_text = "changed".into();
    assert!(changed.validate().is_err());
    let mut changed = saved.clone();
    changed.concrete_input = None;
    assert!(changed.validate().is_err());
    let mut changed = saved.clone();
    changed.proposals.push(changed.proposals[0].clone());
    assert!(changed.validate().is_err());
    let mut legacy = saved;
    legacy.audio_source = None;
    legacy.validate().unwrap();
    assert!(
        json!(legacy).get("audio_source").is_none(),
        "legacy serialized shapes remain unchanged without this source"
    );
}
#[tokio::test]
async fn disconnected_audio_authoring_retains_receipt_and_never_claims_saved_work() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "source".into());
    let command = AudioCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: input(),
    };
    let report = service
        .audio_request(AudioRequest {
            schema_version: 1,
            expected_node_id: "source".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert!(confirmed_node(&report, "source", &command));
    assert_eq!(report["outcome_unknown"], false);
    assert_eq!(report["success"], false);
    assert_eq!(report["command"], json!(command));
    assert!(
        !render_node(&report)
            .unwrap()
            .contains("PRIVATE_AUDIO_INTENT")
    );
    for (field, value) in [
        ("execution_authorized", json!(true)),
        ("dispatched", json!(true)),
        ("automatically_retried", json!(true)),
        ("coordinator_acknowledged", json!(true)),
        ("success", json!(true)),
        ("result", json!({})),
    ] {
        let mut changed = report.clone();
        changed[field] = value;
        assert!(!confirmed_node(&changed, "source", &command));
    }
    assert!(!confirmed_node(&report, "other", &command));
}
