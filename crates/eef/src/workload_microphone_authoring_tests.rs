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
    assert!(
        f.runtime
            .origin_microphone_request(changed, &context)
            .await
            .is_err(),
        "Even an equivalent selector cannot replace immutable caller intent"
    );
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
