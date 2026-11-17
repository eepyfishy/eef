// Actual origin mailbox, TLS source inspection and immutable coordinator journal;
// applied model descriptors only, no Python/device/model process or inference.
#[tokio::test]
async fn vision_authoring_preserves_explicit_model_intent_and_retained_retry_without_provider_or_execution()
 {
    use eefn::workload_leases::definition::vision::{
        AnalysisLimits, Images, ModelSelection, Trigger,
    };
    let metadata = serde_json::from_value(
        json!({"resources":[{"id":"camera","capability":"camera.capture",
        "parameters":{"device":"rtsp://owner:PRIVATE_CAMERA_PASSWORD@camera.invalid/live"}}]}),
    )
    .unwrap();
    let selected = serde_json::from_value(json!({"model_id":"selected","modality":"vlm"})).unwrap();
    let mut f = SourceAdmission::with_selection(metadata, Some(selected)).await;
    let before = std::fs::read(&f.node_path).unwrap();
    let mut input = camera_intent();
    input.schema_version = 2;
    input.events.max_total_bytes = 32768;
    input.vision = Some(eefn::vision_submission::Analysis {
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
            backend: eefn::model_selection::Backend::Ollama,
            model_id: "selected".into(),
            installed_digest: Some("a".repeat(64)),
        },
        prompt: "Describe movement".into(),
        inference: AnalysisLimits {
            max_tokens: 32,
            max_response_bytes: 8192,
            max_result_bytes: 1024,
            timeout_ms: 2000,
        },
    });
    let mut missing = input.clone();
    missing.vision.as_mut().unwrap().model.model_id = "unselected".into();
    let missing = eefn::camera_submission::CameraCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input: missing,
    };
    let refused = submit_camera(&f, missing.clone()).await;
    assert!(eefn::camera_submission::confirmed_node(
        &refused, "origin", &missing
    ));
    assert_eq!(refused["success"], false);
    assert_eq!(refused["error_code"], "camera_vision_model_unavailable");
    assert!(f.runtime.engine.store.list().unwrap().is_empty());
    let command = eefn::camera_submission::CameraCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    let report = submit_camera(&f, command.clone()).await;
    assert!(
        eefn::camera_submission::confirmed_node(&report, "origin", &command),
        "{report}"
    );
    assert_eq!(report["success"], true, "{report}");
    let id = report["result"]["result"]["plan"]["id"].as_str().unwrap();
    let saved = f.runtime.engine.store.get(id).unwrap();
    assert_eq!(saved.status, PlanStatus::AwaitingApproval);
    assert!(saved.approval.is_none() && saved.execution.is_none());
    let binding = &saved.planning.as_ref().unwrap().bindings[0];
    assert_eq!(binding.model_id.as_deref(), Some("selected"));
    assert_eq!(binding.backend.as_deref(), Some("ollama"));
    assert!(
        saved
            .request_review
            .as_ref()
            .unwrap()
            .current()
            .suggested_capabilities
            .iter()
            .any(|c| c == "vlm.analyze")
    );
    let raw = serde_json::to_string(&report).unwrap();
    assert!(!raw.contains("PRIVATE_CAMERA_PASSWORD") && !raw.contains("rtsp://"));
    let context = saved.goal.request_context.as_ref().unwrap().clone();
    let mut changed = command.input.clone();
    changed.vision.as_mut().unwrap().prompt = "Different task".into();
    let changed_command = eefn::camera_submission::CameraCommand {
        operation_id: command.operation_id.clone(),
        input: changed.clone(),
    };
    let conflict = f
        .runtime
        .origin_camera_request(changed, &context)
        .await
        .unwrap();
    assert!(eefn::camera_submission::outcomes::confirmed(
        &conflict,
        "origin",
        &changed_command
    ));
    assert_eq!(conflict["reason"], "intent_conflict");
    assert_eq!(std::fs::read(&f.node_path).unwrap(), before);
    assert!(
        f.service
            .live
            .lock()
            .unwrap()
            .get("last_workload")
            .is_none()
    );
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
