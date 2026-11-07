async fn camera_authoring_fixture() -> SourceAdmission {
    SourceAdmission::with_metadata(serde_json::from_value(json!({"resources":[{"id":"camera","capability":"camera.capture","parameters":{"device":"rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid/live"}}]})).unwrap()).await
}
fn camera_intent() -> eefn::camera_submission::CameraInput {
    use eefn::workload_leases::definition::*;
    eefn::camera_submission::CameraInput {
        schema_version: 1,
        original_text: "Review local motion observations".into(),
        title: "Camera motion".into(),
        source_node: None,
        camera: None,
        priority: 0,
        parameters: MotionParameters {
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
        },
        limits: Limits {
            max_session_ms: 60000,
            max_lease_ms: 10000,
            max_job_memory_mib: 128,
            max_source_frames: 3,
        },
        events: Events {
            mode: EventMode::ActivityTransitions,
            min_interval_ms: 100,
            max_events: 10,
            max_event_bytes: 1024,
            max_total_bytes: 65536,
            max_pending: 4,
            overflow: Overflow::Stop,
        },
    }
}
async fn submit_camera(
    f: &SourceAdmission,
    command: eefn::camera_submission::CameraCommand,
) -> Value {
    f.service
        .camera_request(eefn::camera_submission::CameraRequest {
            schema_version: 1,
            expected_node_id: "origin".into(),
            command,
        })
        .await
        .unwrap()
}
#[tokio::test]
async fn typed_camera_request_retains_origin_choices_and_history_without_rebinding_after_disconnect()
 {
    let mut f = camera_authoring_fixture().await;
    let command = eefn::camera_submission::CameraCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: camera_intent(),
    };
    let result = submit_camera(&f, command.clone()).await;
    assert_eq!(result["success"], true, "{result}");
    assert!(eefn::camera_submission::confirmed_node(
        &result, "origin", &command
    ));
    let id = result["result"]["result"]["plan"]["id"].as_str().unwrap();
    let saved = f.runtime.engine.store.get(id).unwrap();
    assert_eq!(saved.status, PlanStatus::AwaitingApproval);
    assert!(saved.approval.is_none() && saved.execution.is_none());
    assert_eq!(
        saved.request_review.as_ref().unwrap().camera_source,
        Some(command.input.clone())
    );
    assert!(
        saved
            .request_review
            .as_ref()
            .unwrap()
            .workload_source
            .is_none()
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("PRIVATE_CAMERA_PASSWORD")
    );
    let context = saved.goal.request_context.as_ref().unwrap().clone();
    for fault in 0..3 {
        assert!(
            f.runtime
                .engine
                .store
                .change(id, "Synthetic camera intent replacement", |p| {
                    match fault {
                        0 => p.request_review.as_mut().unwrap().camera_source = None,
                        1 => {
                            p.goal.request_context = Some(RequestContext::new(
                                context.request_id().into(),
                                "other".into(),
                                vec![],
                            )?)
                        }
                        _ => {
                            let d = p.request_review.as_mut().unwrap();
                            d.camera_source.as_mut().unwrap().priority = 1;
                            d.concrete_input.as_mut().unwrap().params["workload"]["priority"] =
                                json!(1);
                        }
                    }
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
    }
    let mut changed = command.input.clone();
    changed.camera = Some("camera".into());
    let conflict = eefn::camera_submission::CameraCommand {
        operation_id: command.operation_id.clone(),
        input: changed.clone(),
    };
    let refused = f
        .runtime
        .origin_camera_request(changed, &context)
        .await
        .unwrap();
    assert!(eefn::camera_submission::outcomes::confirmed(
        &refused, "origin", &conflict
    ));
    assert_eq!(refused["reason"], "intent_conflict");
    f.disconnect().await;
    f.runtime.shutdown().await;
    let retained = f
        .runtime
        .origin_camera_request(command.input.clone(), &context)
        .await
        .unwrap();
    assert!(eefn::camera_submission::confirmed(
        &retained, "origin", &command
    ));
    assert_eq!(retained["result"]["creation_reused"], true);
    assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
}
#[tokio::test]
async fn unavailable_or_undisclosed_camera_and_pending_bindings_create_no_review_or_capture() {
    let f = camera_authoring_fixture().await;
    for field in ["source_node", "camera"] {
        let mut input = json!(camera_intent());
        input[field] = json!("missing");
        let command = eefn::camera_submission::CameraCommand {
            operation_id: uuid::Uuid::new_v4().to_string(),
            input: serde_json::from_value(input).unwrap(),
        };
        let refused = submit_camera(&f, command.clone()).await;
        assert!(eefn::camera_submission::confirmed_node(
            &refused, "origin", &command
        ));
        assert_eq!(refused["success"], false);
        assert_eq!(refused["outcome_unknown"], false);
        assert!(f.runtime.engine.store.list().unwrap().is_empty());
    }
    let mut config: Value = serde_json::from_slice(&std::fs::read(&f.node_path).unwrap()).unwrap();
    config["metadata"]["resources"][0]["parameters"]["device"] = json!(2);
    f.service.save_configuration(config).unwrap();
    let command = eefn::camera_submission::CameraCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: camera_intent(),
    };
    let refused = submit_camera(&f, command).await;
    assert_eq!(refused["error_code"], "camera_binding_not_applied");
    assert!(f.runtime.engine.store.list().unwrap().is_empty());
    assert!(
        f.service
            .live
            .lock()
            .unwrap()
            .get("last_workload")
            .is_none()
    );
    f.close().await;
}
#[tokio::test]
async fn incomplete_camera_review_never_recompiles_on_receipt_reuse_and_explicit_compile_keeps_intent()
 {
    let f = camera_authoring_fixture().await;
    let command = eefn::camera_submission::CameraCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: camera_intent(),
    };
    let db = rusqlite::Connection::open(f._dir.path().join("owner.db")).unwrap();
    db.execute_batch(r#"CREATE TRIGGER refuse_camera_compile BEFORE UPDATE ON durable_jobs WHEN NEW.plan_json LIKE '%"planning":{%' BEGIN SELECT RAISE(ABORT,'synthetic camera compile failure'); END;"#).unwrap();
    let failed = submit_camera(&f, command.clone()).await;
    assert_eq!(failed["success"], false);
    assert_eq!(failed["outcome_unknown"], true);
    let saved = f.runtime.engine.store.list().unwrap().pop().unwrap();
    assert_eq!(saved.status, PlanStatus::Review);
    assert!(saved.planning.is_none());
    db.execute_batch("DROP TRIGGER refuse_camera_compile")
        .unwrap();
    drop(db);
    let repeated = submit_camera(&f, command.clone()).await;
    assert_eq!(repeated["error_code"], "camera_preparation_incomplete");
    assert_eq!(
        json!(f.runtime.engine.store.get(&saved.plan_id).unwrap()),
        json!(saved)
    );
    let context =
        RequestContext::new(uuid::Uuid::new_v4().to_string(), "origin".into(), vec![]).unwrap();
    f.runtime
        .origin_planning_command(
            eefn::request_planning::PlanningCommand::CompileSaved {
                id: saved.plan_id.clone(),
                expected_revision: 1,
            },
            &context,
        )
        .await
        .unwrap();
    let compiled = f.runtime.engine.store.get(&saved.plan_id).unwrap();
    assert_eq!(compiled.status, PlanStatus::AwaitingApproval);
    assert_eq!(json!(compiled.request_review), json!(saved.request_review));
    let retained = submit_camera(&f, command).await;
    assert_eq!(retained["success"], true);
    assert_eq!(retained["result"]["result"]["creation_reused"], true);
    f.close().await;
}
