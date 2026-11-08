use super::*;

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn proposal(question: bool) -> Value {
    json!({"schema_version":1,"intent":"information","goal":"Inspect node status",
        "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],
        "complexity":"simple","workload":"one_shot","needs_clarification":question,
        "clarification":if question {json!("Which node?")}else{Value::Null}})
}
fn draft(question: bool, answered: bool) -> crate::request_drafts::DraftRecord {
    let mut draft:crate::request_drafts::DraftRecord=serde_json::from_value(json!({
        "schema_version":1,"original_text":"  Inspect node status\n",
        "proposals":[{"operation_id":uuid(),"proposal":proposal(question),"allowed_capabilities":["system.info"],"at_ms":1}],"answers":[]
    })).unwrap();
    if answered {
        let source = crate::request_drafts::AnswerSource::from_draft(3, &draft).unwrap();
        draft
            .answers
            .push(crate::request_drafts::ClarificationAnswer {
                operation_id: uuid(),
                proposal_index: 0,
                question: "Which node?".into(),
                text: "This node".into(),
                at_ms: 2,
                source: Some(source),
            });
    }
    draft.validate().unwrap();
    draft
}
fn snapshot(source: Source) -> Snapshot {
    Snapshot {
        schema_version: 1,
        job_id: uuid::Uuid::new_v4().simple().to_string(),
        revision: match &source {
            Source::Initial { .. } => 2,
            Source::Clarification { .. } => 4,
            Source::Method { .. } => 3,
        },
        operation_id: uuid(),
        request_context: crate::context::RequestContext::new(
            uuid(),
            "origin-node".into(),
            vec!["home".into()],
        )
        .unwrap(),
        allowed_capabilities: vec!["system.info".into()],
        source,
    }
}
fn initial() -> Snapshot {
    snapshot(Source::Initial {
        input: crate::request_intent::Input {
            schema_version: 1,
            original_text: "  Inspect node status\n".into(),
        },
    })
}

#[test]
fn all_three_stages_reconstruct_exact_existing_inputs_and_round_trip() {
    for source in [
        initial(),
        snapshot(Source::Clarification {
            draft: Box::new(draft(true, true)),
        }),
        snapshot(Source::Method {
            draft: Box::new(draft(false, false)),
        }),
    ] {
        let ceiling = source.source.max_output_tokens();
        let task = Task::new(source.clone(), ceiling).unwrap();
        let spec = task.validate().unwrap();
        let expected = match &source.source {
            Source::Initial { input } => {
                crate::interpretation::messages(&input.original_text, &source.allowed_capabilities)
                    .unwrap()
            }
            Source::Clarification { draft } => {
                crate::interpretation::clarification_messages(draft, &source.allowed_capabilities)
                    .unwrap()
            }
            Source::Method { draft } => {
                crate::request_preparation::messages(draft, &source.allowed_capabilities).unwrap()
            }
        };
        assert_eq!(spec.messages, expected);
        if matches!(&source.source, Source::Method { .. }) {
            assert_eq!(ceiling, 1024);
            assert!(spec.output_schema.is_none());
        } else {
            assert_eq!(ceiling, 512);
            assert_eq!(
                spec.output_schema,
                Some(crate::interpretation::output_schema(&source.allowed_capabilities).unwrap())
            );
        }
        let restored: Task = serde_json::from_slice(&serde_json::to_vec(&task).unwrap()).unwrap();
        assert_eq!(restored.validate().unwrap(), spec);
        assert_eq!(restored.snapshot_sha256, task.snapshot_sha256);
        assert_eq!(restored.inference_spec_sha256, task.inference_spec_sha256);
    }
}

#[test]
fn pinned_source_changes_invalidate_task_even_when_prompt_text_is_unchanged() {
    let task = Task::new(initial(), 512).unwrap();
    let mut changed = task.clone();
    changed.snapshot.job_id = uuid::Uuid::new_v4().simple().to_string();
    assert!(changed.validate().is_err());
    assert_eq!(
        changed.snapshot.spec(512).unwrap(),
        task.validate().unwrap()
    );
    let rebuilt = Task::new(changed.snapshot, 512).unwrap();
    assert_ne!(rebuilt.snapshot_sha256, task.snapshot_sha256);
    assert_eq!(
        rebuilt.inference_spec_sha256, task.inference_spec_sha256,
        "a matching prompt is not source ownership or authority"
    );
    let mut changed = task.clone();
    changed.snapshot.revision += 1;
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    changed.snapshot.operation_id = uuid();
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    changed.snapshot.request_context = crate::context::RequestContext::new(
        task.snapshot.request_context.request_id().into(),
        "another-origin".into(),
        vec!["elsewhere".into()],
    )
    .unwrap();
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    changed
        .snapshot
        .allowed_capabilities
        .push("filesystem.read".into());
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    if let Source::Initial { input } = &mut changed.snapshot.source {
        input.original_text.push('!');
    }
    assert!(changed.validate().is_err());
}

#[test]
fn prompt_format_and_token_definition_are_bound_and_never_substituted() {
    let task = Task::new(initial(), 512).unwrap();
    let mut changed = task.clone();
    changed.inference_spec_sha256 = "0".repeat(64);
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    changed.max_output_tokens = 1;
    assert!(changed.validate().is_err());
    let restricted = Task::new(task.snapshot.clone(), 1).unwrap();
    assert_ne!(restricted.inference_spec_sha256, task.inference_spec_sha256);
    assert_eq!(restricted.snapshot_sha256, task.snapshot_sha256);
    for tokens in [0, 513, u64::MAX] {
        assert!(Task::new(task.snapshot.clone(), tokens).is_err());
    }
    let mut changed = task.clone();
    changed.snapshot_sha256 = "A".repeat(64);
    assert!(changed.validate().is_err());
}

#[test]
fn malformed_versions_authority_fields_and_context_extensions_are_rejected() {
    let task = Task::new(initial(), 512).unwrap();
    for pointer in [
        "/execution_authorized",
        "/messages",
        "/snapshot/permissions",
        "/snapshot/request_context/grant",
        "/snapshot/source/input/approve",
    ] {
        let mut wire = json!(task);
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        wire.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(true));
        assert!(
            serde_json::from_value::<Task>(wire).is_err(),
            "accepted {pointer}"
        );
    }
    let mut changed = task.clone();
    changed.schema_version = 2;
    assert!(changed.validate().is_err());
    let mut changed = task.clone();
    changed.snapshot.schema_version = 2;
    assert!(changed.validate().is_err());
    let mut wire = json!(task);
    wire["snapshot"]["source"]["stage"] = json!("unknown");
    assert!(serde_json::from_value::<Task>(wire).is_err());
}

#[test]
fn semantic_candidates_stay_untrusted_and_method_evidence_retains_source() {
    let bytes = serde_json::to_vec(&proposal(false)).unwrap();
    for source in [
        initial(),
        snapshot(Source::Clarification {
            draft: Box::new(draft(true, true)),
        }),
    ] {
        let task = Task::new(source, 512).unwrap();
        let result = serde_json::to_value(
            task.candidate(&bytes, "ollama", "selected-by-owner")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["candidate"]["execution_authorized"], false);
        assert_eq!(result["candidate"]["model_output_is_untrusted"], true);
        assert_eq!(
            result["candidate"]["original_text"],
            "  Inspect node status\n"
        );
        let mut malicious = proposal(false);
        malicious["PRIVATE_OUTPUT_AUTHORITY"] = json!(true);
        let error = task
            .candidate(
                &serde_json::to_vec(&malicious).unwrap(),
                "ollama",
                "selected-by-owner",
            )
            .err()
            .unwrap();
        assert!(!error.to_string().contains("PRIVATE_OUTPUT_AUTHORITY"));
    }
    let task = Task::new(
        snapshot(Source::Method {
            draft: Box::new(draft(false, false)),
        }),
        1024,
    )
    .unwrap();
    let bytes=serde_json::to_vec(&json!({"schema_version":1,"method":{"schema_version":1,"template":"system_info","params":{},"constraints":{}},"clarification":null})).unwrap();
    let result = serde_json::to_value(
        task.candidate(&bytes, "llamacpp", "selected-by-owner")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["candidate"]["execution_authorized"], false);
    assert_eq!(result["candidate"]["model_output_is_untrusted"], true);
    assert_eq!(
        result["candidate"]["evidence"]["job_id"],
        task.snapshot.job_id
    );
    assert_eq!(
        result["candidate"]["evidence"]["source_revision"],
        task.snapshot.revision
    );
    assert_eq!(result["candidate"]["evidence"]["backend"], "llamacpp");
    if let Source::Method { draft } = &task.snapshot.source {
        assert_eq!(
            result["candidate"]["evidence"]["source_sha256"],
            crate::grants::definition_digest(&json!(draft)).unwrap()
        );
    }
    assert!(
        task.candidate(&bytes, "invented-backend", "selected-by-owner")
            .is_err()
    );
}

#[test]
fn ineligible_history_encoded_overflow_and_invalid_output_never_truncate_or_escape() {
    assert!(
        Task::new(
            snapshot(Source::Clarification {
                draft: Box::new(draft(true, false))
            }),
            512
        )
        .is_err()
    );
    assert!(
        Task::new(
            snapshot(Source::Method {
                draft: Box::new(draft(true, true))
            }),
            1024
        )
        .is_err()
    );
    for revision in [0, u64::MAX] {
        let mut source = initial();
        source.revision = revision;
        assert!(Task::new(source, 512).is_err());
    }
    let mut source = initial();
    if let Source::Initial { input } = &mut source.source {
        input.original_text = "\\".repeat(32700);
        input.validate().unwrap();
    }
    assert!(serde_json::to_vec(&source).unwrap().len() > MAX_SNAPSHOT_BYTES);
    let before = serde_json::to_vec(&source).unwrap();
    assert!(Task::new(source.clone(), 512).is_err());
    assert_eq!(serde_json::to_vec(&source).unwrap(), before);
    let task = Task::new(initial(), 512).unwrap();
    for output in [
        b"".as_slice(),
        b"{\"thinking\":\"PRIVATE_REASONING\"}".as_slice(),
        b"{}".as_slice(),
    ] {
        assert!(
            task.candidate(output, "ollama", "selected-by-owner")
                .is_err()
        );
    }
}
