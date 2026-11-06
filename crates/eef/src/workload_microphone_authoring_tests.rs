async fn microphone_input(f: &SourceAdmission) -> eefn::microphone_submission::MicrophoneInput {
    let concrete = f.input().await;
    let template =
        eefn::workload_leases::definition::Template::from_params(&concrete.input.params).unwrap();
    let eefn::workload_leases::definition::Source::MicrophoneEnergy { parameters, .. } =
        template.source;
    eefn::microphone_submission::MicrophoneInput {
        schema_version: 1,
        original_text: concrete.original_text,
        title: concrete.title,
        source_node: None,
        microphone: None,
        priority: template.priority,
        parameters,
        limits: template.limits,
        events: template.events,
    }
}

async fn retained_incomplete_microphone() -> (
    SourceAdmission,
    eefn::microphone_submission::MicrophoneCommand,
    crate::task::Plan,
) {
    use eefn::microphone_submission::{MicrophoneCommand, MicrophoneRequest};
    let f = SourceAdmission::new().await;
    let command = MicrophoneCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: microphone_input(&f).await,
    };
    let db = rusqlite::Connection::open(f._dir.path().join("owner.db")).unwrap();
    db.execute_batch(r#"CREATE TRIGGER refuse_saved_compile BEFORE UPDATE ON durable_jobs
        WHEN NEW.plan_json LIKE '%"planning":{%' BEGIN SELECT RAISE(ABORT,'synthetic saved compile failure'); END;"#).unwrap();
    let result = f
        .service
        .microphone_request(MicrophoneRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(result["success"], false);
    assert_eq!(result["outcome_unknown"], true);
    let mut saved = f.runtime.engine.store.list().unwrap();
    assert_eq!(saved.len(), 1);
    let saved = saved.pop().unwrap();
    assert_eq!(saved.status, PlanStatus::Review);
    assert!(saved.planning.is_none());
    db.execute_batch("DROP TRIGGER refuse_saved_compile")
        .unwrap();
    drop(db);
    (f, command, saved)
}

#[tokio::test]
async fn explicit_saved_compilation_preserves_original_microphone_receipt_and_requires_origin() {
    use eefn::request_planning::{PlanningCommand, PlanningRequest};
    let (f, opening, saved) = retained_incomplete_microphone().await;
    let id = saved.plan_id.clone();
    let original = json!(saved);
    let command = PlanningCommand::CompileSaved {
        id: id.clone(),
        expected_revision: 1,
    };
    let foreign =
        RequestContext::new(uuid::Uuid::new_v4().to_string(), "other".into(), vec![]).unwrap();
    assert!(
        f.runtime
            .origin_planning_command(command.clone(), &foreign)
            .await
            .is_err()
    );
    assert_eq!(json!(f.runtime.engine.store.get(&id).unwrap()), original);
    let stale = f
        .service
        .planning_request(PlanningRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            command: PlanningCommand::CompileSaved {
                id: id.clone(),
                expected_revision: 2,
            },
        })
        .await
        .unwrap();
    assert_eq!(stale["success"], false);
    assert!(stale.get("result").is_none());
    assert_eq!(json!(f.runtime.engine.store.get(&id).unwrap()), original);
    let transport = uuid::Uuid::new_v4().to_string();
    let prepared = f
        .service
        .planning_request(PlanningRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            request_id: transport.clone(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        prepared["success"], true,
        "Saved compilation failed: {prepared}"
    );
    assert!(eefn::request_planning::confirmed(
        &prepared["result"],
        "origin",
        &transport,
        &command
    ));
    let compiled = f.runtime.engine.store.get(&id).unwrap();
    assert_eq!(compiled.status, PlanStatus::AwaitingApproval);
    assert_eq!(compiled.revision, 2);
    assert_eq!(json!(compiled.request_review), json!(saved.request_review));
    assert_eq!(compiled.goal.request_context, saved.goal.request_context);
    assert!(compiled.approval.is_none() && compiled.execution.is_none());
    assert_eq!(
        compiled.planning.as_ref().unwrap().operation_id,
        eefn::request_submission::compilation_receipt("origin", &opening.operation_id, &id)
            .unwrap()
    );
    let snapshot = json!(compiled);
    let reused = f
        .service
        .planning_request(PlanningRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(reused["success"], true);
    assert_eq!(reused["result"]["reused"], true);
    let original_receipt = f
        .service
        .microphone_request(eefn::microphone_submission::MicrophoneRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: opening.clone(),
        })
        .await
        .unwrap();
    assert_eq!(original_receipt["success"], true);
    assert_eq!(
        original_receipt["result"]["result"]["creation_reused"],
        true
    );
    assert_eq!(json!(f.runtime.engine.store.get(&id).unwrap()), snapshot);
    let mut f = f;
    f.disconnect().await;
    let context =
        RequestContext::new(uuid::Uuid::new_v4().to_string(), "origin".into(), vec![]).unwrap();
    let offline = f
        .runtime
        .origin_planning_command(command.clone(), &context)
        .await
        .unwrap();
    assert!(eefn::request_planning::confirmed(
        &offline,
        "origin",
        context.request_id(),
        &command
    ));
    assert_eq!(offline["reused"], true);
    assert_eq!(json!(f.runtime.engine.store.get(&id).unwrap()), snapshot);
    f.runtime.shutdown().await;
}

#[tokio::test]
async fn saved_compilation_cannot_finish_an_uncompiled_source_after_provider_disconnect() {
    let (mut f, _, saved) = retained_incomplete_microphone().await;
    f.disconnect().await;
    let context =
        RequestContext::new(uuid::Uuid::new_v4().to_string(), "origin".into(), vec![]).unwrap();
    let command = eefn::request_planning::PlanningCommand::CompileSaved {
        id: saved.plan_id.clone(),
        expected_revision: 1,
    };
    assert!(
        f.runtime
            .origin_planning_command(command, &context)
            .await
            .is_err()
    );
    assert_eq!(
        json!(f.runtime.engine.store.get(&saved.plan_id).unwrap()),
        json!(saved)
    );
    f.runtime.shutdown().await;
}
#[tokio::test]
async fn typed_microphone_origin_request_retains_choices_and_receipt_without_provider_rebinding() {
    use eefn::microphone_submission::{MicrophoneCommand, MicrophoneRequest};
    let mut f = SourceAdmission::new().await;
    let input = microphone_input(&f).await;
    let command = MicrophoneCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: input.clone(),
    };
    let result = f
        .service
        .microphone_request(MicrophoneRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        result["success"], true,
        "Typed microphone authoring failed: {result}"
    );
    assert!(eefn::microphone_submission::confirmed_node(
        &result, "origin", &command
    ));
    let id = result["result"]["result"]["plan"]["id"].as_str().unwrap();
    let saved = f.runtime.engine.store.get(id).unwrap();
    assert_eq!(saved.status, PlanStatus::AwaitingApproval);
    assert!(saved.approval.is_none() && saved.execution.is_none());
    assert_eq!(
        saved.request_review.as_ref().unwrap().workload_source,
        Some(input.clone())
    );
    let context = saved.goal.request_context.as_ref().unwrap().clone();
    for fault in 0..3 {
        let refused = f
            .runtime
            .engine
            .store
            .change(id, "Synthetic source replacement", |p| {
                let draft = p.request_review.as_mut().unwrap();
                match fault {
                    0 => draft.workload_source = None,
                    1 => {
                        p.goal.request_context = Some(RequestContext::new(
                            context.request_id().into(),
                            "other".into(),
                            vec![],
                        )?)
                    }
                    _ => {
                        draft.workload_source.as_mut().unwrap().priority += 1;
                        draft.concrete_input.as_mut().unwrap().params["workload"]["priority"] =
                            json!(1);
                    }
                }
                Ok(())
            });
        assert!(refused.is_err());
        assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
    }
    let mut changed = input.clone();
    changed.microphone = Some("mic".into());
    let conflicting = MicrophoneCommand {
        operation_id: command.operation_id.clone(),
        input: changed.clone(),
    };
    let refusal = f
        .runtime
        .origin_microphone_request(changed, &context)
        .await
        .unwrap();
    assert!(eefn::microphone_submission::outcomes::confirmed(
        &refusal,
        "origin",
        &conflicting
    ));
    assert_eq!(
        refusal["reason"], "intent_conflict",
        "Even an equivalent selector cannot replace immutable caller intent"
    );
    assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
    f.disconnect().await;
    let retained = f
        .runtime
        .origin_microphone_request(input, &context)
        .await
        .unwrap();
    assert!(eefn::microphone_submission::confirmed(
        &retained, "origin", &command
    ));
    assert_eq!(retained["result"]["creation_reused"], true);
    assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
    f.runtime.shutdown().await;
}
#[tokio::test]
async fn typed_microphone_missing_or_undisclosed_resource_creates_no_job_or_effect() {
    use eefn::microphone_submission::{MicrophoneCommand, MicrophoneRequest};
    let f = SourceAdmission::new().await;
    let input = microphone_input(&f).await;
    for fault in 0..2 {
        let mut changed = input.clone();
        if fault == 0 {
            changed.microphone = Some("missing".into());
        } else {
            changed.source_node = Some("undisclosed".into());
        }
        let command = MicrophoneCommand {
            operation_id: uuid::Uuid::new_v4().to_string(),
            input: changed,
        };
        let result = f
            .service
            .microphone_request(MicrophoneRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                command: command.clone(),
            })
            .await
            .unwrap();
        assert!(eefn::microphone_submission::confirmed_node(
            &result, "origin", &command
        ));
        assert_eq!(result["success"], false);
        assert_eq!(result["coordinator_acknowledged"], true);
        assert_eq!(result["outcome_unknown"], false);
        assert_eq!(
            result["refusal"]["reason"],
            if fault == 0 {
                "resource_unavailable"
            } else {
                "source_unavailable_or_not_disclosed"
            }
        );
        assert_eq!(result["refusal"]["review_checkpoint_attempted"], false);
        assert!(f.runtime.engine.store.list().unwrap().is_empty());
        assert!(
            f.service
                .live
                .lock()
                .unwrap()
                .get("last_workload")
                .is_none()
        );
    }
    f.close().await;
}

#[tokio::test]
async fn microphone_compile_failure_after_review_save_remains_unknown_not_a_preflight_refusal() {
    use eefn::microphone_submission::{MicrophoneCommand, MicrophoneRequest};
    let f = SourceAdmission::new().await;
    let input = microphone_input(&f).await;
    let db = rusqlite::Connection::open(f._dir.path().join("owner.db")).unwrap();
    db.execute_batch(r#"CREATE TRIGGER refuse_microphone_compile BEFORE UPDATE ON durable_jobs
        WHEN NEW.plan_json LIKE '%"planning":{%' BEGIN SELECT RAISE(ABORT,'synthetic compile commit failure'); END;"#).unwrap();
    let command = MicrophoneCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    let result = f
        .service
        .microphone_request(MicrophoneRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command: command.clone(),
        })
        .await
        .unwrap();
    assert!(eefn::microphone_submission::confirmed_node(
        &result, "origin", &command
    ));
    assert_eq!(result["success"], false);
    assert_eq!(result["coordinator_acknowledged"], false);
    assert_eq!(result["outcome_unknown"], true);
    assert!(result.get("refusal").is_none());
    let saved = f.runtime.engine.store.list().unwrap();
    assert_eq!(
        saved.len(),
        1,
        "A retained review proves the partial authoring checkpoint"
    );
    assert_eq!(saved[0].status, PlanStatus::Review);
    assert!(
        saved[0].planning.is_none() && saved[0].approval.is_none() && saved[0].execution.is_none()
    );
    assert_eq!(
        saved[0].goal.request_context.as_ref().unwrap().request_id(),
        command.operation_id
    );
    let context = saved[0].goal.request_context.as_ref().unwrap().clone();
    let original = json!(saved[0]);
    db.execute_batch("DROP TRIGGER refuse_microphone_compile")
        .unwrap();
    drop(db);
    // Source is available and the original fixture obstruction is gone. Reuse
    // must still inspect the partial receipt, never silently finish its compile.
    let retained = f
        .runtime
        .origin_microphone_request(command.input.clone(), &context)
        .await
        .unwrap();
    assert!(eefn::microphone_submission::outcomes::confirmed(
        &retained, "origin", &command
    ));
    assert_eq!(retained["reason"], "preparation_incomplete");
    assert_eq!(
        json!(f.runtime.engine.store.get(&saved[0].plan_id).unwrap()),
        original
    );
    let mut f = f;
    f.disconnect().await;
    let offline = f
        .runtime
        .origin_microphone_request(command.input.clone(), &context)
        .await
        .unwrap();
    assert_eq!(
        offline, retained,
        "Partial receipt reuse needs no current provider"
    );
    assert_eq!(
        json!(f.runtime.engine.store.get(&saved[0].plan_id).unwrap()),
        original
    );
    f.runtime.shutdown().await;
}
