// Original coordinator windows/journal, synthetic authenticated result values.
// These fixtures do not create native source witnesses or claim model quality.
fn vision_input(large: bool) -> PlanInput {
    use eefn::{model_selection::Backend, workload_leases::definition::vision::*};
    let mut input = template_input();
    input.schema_version = 2;
    let mut t = Template::from_params(&input.params).unwrap();
    t.schema_version = 3;
    t.limits.max_source_frames = if large { 101 } else { 3 };
    t.events.max_total_bytes = 32 * 1024;
    t.source = Source::CameraVision {
        resource_id: "executor::camera".into(),
        resource_sha256: "d".repeat(64),
        adapter_sha256: eefn::workload_sources::vision_adapter_digest(),
        parameters: Parameters {
            capture: MotionParameters {
                width: 640,
                height: 480,
                sample_width: 64,
                sample_height: 48,
                max_frames: if large { 101 } else { 3 },
                interval_ms: 200,
                activation_level: 15,
                deactivation_level: 5,
                io_timeout_ms: 2000,
                max_duration_ms: if large { 60000 } else { 10000 },
            },
            images: Images {
                width: 320,
                height: 240,
                max_image_bytes: 65536,
                jpeg_quality: 70,
                max_images: if large { 100 } else { 1 },
                min_interval_ms: 1000,
                trigger: Trigger::Rising,
            },
            model: ModelSelection {
                backend: Backend::Ollama,
                model_id: "selected".into(),
                installed_digest: Some("a".repeat(64)),
            },
            prompt: "Describe movement".into(),
            inference: AnalysisLimits {
                max_tokens: 32,
                max_response_bytes: 8192,
                max_result_bytes: if large { 4096 } else { 256 },
                timeout_ms: 2000,
            },
        },
    };
    t.validate().unwrap();
    input.params = json!({"workload":t});
    input
}
fn vision_approved(engine: &TaskEngine, large: bool) -> Plan {
    let (review, input) = review_input(engine, vision_input(large));
    let command = PlanningCommand::Compile {
        id: review.plan_id,
        expected_revision: review.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    let plan = compile(engine, &command).unwrap();
    let approval = ApprovalCommand {
        id: plan.plan_id.clone(),
        expected_revision: plan.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
        valid_for_ms: 120000,
    };
    approve(engine, &approval, &"a".repeat(64)).unwrap()
}
fn vision_policy() -> Policy {
    Policy {
        vision_analysis: true,
        ..policy()
    }
}
fn vision_event() -> eefn::workload_network::Event {
    eefn::workload_network::Event {
        schema_version: 2,
        kind: eefn::workload_network::EventKind::CameraMotion,
        sequence: 1,
        source_sequence: 1,
        source_frames: 1,
        elapsed_ms: 0,
        active: None,
        level: None,
        discontinuity: false,
    }
}
fn vision_values(
    d: &Definition,
    ack: &eefn::grants::workload::Receipt,
) -> (
    eefn::grants::workload::Receipt,
    eefn::grants::workload::VisionHistory,
) {
    use sha2::{Digest, Sha256};
    // Canonical protocol structs are deliberately independent of the private
    // journal constructors. Only ordinary historical values cross this fixture.
    #[derive(Serialize)]
    struct Intent {
        schema_version: u32,
        grant_sha256: String,
        sequence: u64,
        source_sequence: u64,
        image_sha256: String,
        image_bytes: usize,
        lease_sequence: u64,
        recorded_at_ms: u64,
    }
    #[derive(Serialize)]
    struct Analysis {
        schema_version: u32,
        sequence: u64,
        intent_sha256: String,
        text: String,
        lease_sequence: u64,
        recorded_at_ms: u64,
    }
    let hash = |raw: &[u8]| format!("{:x}", Sha256::digest(raw));
    let scope = d.scope().unwrap();
    let intent = Intent {
        schema_version: 1,
        grant_sha256: scope.grant_sha256.clone(),
        sequence: 1,
        source_sequence: 2,
        image_sha256: "e".repeat(64),
        image_bytes: 8,
        lease_sequence: 1,
        recorded_at_ms: 43,
    };
    let intent_sha = hash(&serde_json::to_vec(&intent).unwrap());
    let analysis = Analysis {
        schema_version: 1,
        sequence: 1,
        intent_sha256: intent_sha.clone(),
        text: "movement detected".into(),
        lease_sequence: 1,
        recorded_at_ms: 44,
    };
    let result_sha = hash(&serde_json::to_vec(&analysis).unwrap());
    let summary = eefn::grants::workload::VisionSummary {
        calls: 1,
        completed: 1,
        result_bytes: 19,
        reserved_output_tokens: 32,
        prefix_sha256: hash(
            format!(
                "{}{}{}",
                hash(scope.grant_sha256.as_bytes()),
                intent_sha,
                result_sha
            )
            .as_bytes(),
        ),
        last_source_sequence: 2,
        last_lease_sequence: 1,
    };
    let event_summary = eefn::workload_network::summarize_events(d, &[vision_event()]).unwrap();
    let mut receipt = ack.receipt.clone();
    receipt.state = "completed".into();
    receipt.updated_at_ms = 44;
    let report = eefn::grants::workload::CompletionReport {
        schema_version: 3,
        grant_sha256: scope.grant_sha256,
        adapter_sha256: d.source.adapter_sha256().into(),
        source_observations: d.source.max_observations(),
        source_frames: d.limits.max_source_frames,
        event_count: 1,
        event_bytes: event_summary.bytes,
        event_prefix_sha256: "f".repeat(64),
        event_payload_prefix_sha256: Some(event_summary.prefix_sha256),
        analysis: Some(summary.clone()),
        lease_sequence: 1,
        recorded_at_ms: 44,
        owned_process_exit_observed: true,
        physical_effects_stopped: false,
    };
    let history=eefn::grants::workload::VisionHistory {schema_version:1,receipt:receipt.clone(),completion:report.clone(),summary,
        analyses:vec![serde_json::from_value(json!({"intent":intent,"intent_sha256":intent_sha,"result":analysis,"result_sha256":result_sha})).unwrap()]};
    history.validate(d).unwrap();
    let node = eefn::grants::workload::Receipt {
        schema_version: 3,
        receipt,
        definition: d.clone(),
        completion: Some(report),
        execution_authorized: false,
        restart_authorized: false,
    };
    node.validate().unwrap();
    (node, history)
}
async fn vision_issued(
    store: &Arc<JobStore>,
    events: bool,
) -> (
    Issued,
    eefn::grants::workload::Receipt,
    eefn::grants::workload::VisionHistory,
) {
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let approved = vision_approved(&engine, false);
    let mut issued = issue(
        store,
        &execute(&approved),
        &uuid::Uuid::new_v4().to_string(),
        &vision_policy(),
        tokio::time::Instant::now() + Duration::from_secs(10),
    )
    .await
    .unwrap();
    lease(&mut issued, 1).await.unwrap();
    let ack = synthetic_ack(issued.definition());
    issued
        .acknowledge(ack.clone(), probe_deadline())
        .await
        .unwrap();
    if events {
        store
            .queue_workload_event(
                issued.definition().clone(),
                vision_event(),
                probe_deadline(),
            )
            .unwrap()
            .await
            .unwrap()
            .unwrap();
    }
    let (node, history) = vision_values(issued.definition(), &ack);
    (issued, node, history)
}
async fn finish_vision(
    issued: &mut Issued,
    node: eefn::grants::workload::Receipt,
    history: eefn::grants::workload::VisionHistory,
) -> Result<Plan> {
    let d = issued.definition().clone();
    issued
        .complete_with_vision(
            node,
            history,
            &d.issuer_id,
            &d.session_id,
            &d.executor_node,
            probe_deadline(),
        )
        .await
}

#[tokio::test]
async fn vision_planning_records_exact_model_and_result_budget_before_opted_in_issuance() {
    let engine = TaskEngine::new(EventBus::new(20));
    let approved = vision_approved(&engine, false);
    let binding = &approved.planning.as_ref().unwrap().bindings[0];
    assert_eq!(binding.model_id.as_deref(), Some("selected"));
    assert_eq!(binding.backend.as_deref(), Some("ollama"));
    assert_eq!(
        approved.approval.as_ref().unwrap().limits.max_result_bytes,
        32 * 1024 + 256
    );
    let before = json!(engine.store.get(&approved.plan_id).unwrap());
    assert!(
        issue(
            &engine.store,
            &execute(&approved),
            &uuid::Uuid::new_v4().to_string(),
            &policy(),
            probe_deadline()
        )
        .await
        .is_err()
    );
    assert_eq!(json!(engine.store.get(&approved.plan_id).unwrap()), before);
    let issued = issue(
        &engine.store,
        &execute(&approved),
        &uuid::Uuid::new_v4().to_string(),
        &vision_policy(),
        probe_deadline(),
    )
    .await
    .unwrap();
    assert_eq!(issued.definition().schema_version, 3);
    let saved = engine.store.get(&approved.plan_id).unwrap();
    let assignment = &saved.tasks[0].assignments[0];
    assert_eq!(assignment.model_id.as_deref(), Some("selected"));
    assert_eq!(assignment.backend.as_deref(), Some("ollama"));
    assert!(
        crate::request_execution::reserved_bytes(&saved)
            >= 32 * 1024 + 8192 + completion_reserve(issued.definition())
    );
}

#[tokio::test]
async fn completed_vision_output_reopens_for_original_origin_without_contacting_or_replaying_node()
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vision.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let (mut issued, node, history) = vision_issued(&store, true).await;
    let d = issued.definition().clone();
    let done = finish_vision(&mut issued, node.clone(), history.clone())
        .await
        .unwrap();
    assert_eq!(done.status, PlanStatus::Completed);
    assert_eq!(
        done.execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .completion
            .as_ref()
            .unwrap()
            .schema_version,
        2
    );
    assert_eq!(crate::request_execution::reserved_bytes(&done), 0);
    assert!(lease(&mut issued, 2).await.is_err());
    assert_eq!(
        finish_vision(&mut issued, node, history)
            .await
            .unwrap()
            .revision,
        done.revision
    );
    drop(issued);
    drop(store);
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    let saved = reopened.get(&d.job_id).unwrap();
    let command = eefn::request_result::ResultCommand {
        id: d.job_id.clone(),
        grant_id: d.grant_id.clone(),
    };
    let runtime = uuid::Uuid::new_v4().to_string();
    let view =
        crate::workload_job_inspection::result(&saved, &command, Some("origin"), &runtime).unwrap();
    assert_eq!(
        view["vision"]["analyses"][0]["result"]["text"],
        "movement detected"
    );
    assert!(eefn::request_result::confirmed(
        &view,
        &command,
        Some("origin"),
        Some(&runtime)
    ));
    assert!(
        crate::workload_job_inspection::result(&saved, &command, Some("other-origin"), &runtime)
            .is_err()
    );
    for (pointer, value) in [
        ("/reconciled", json!(true)),
        ("/execution_requested", json!(true)),
        ("/read_only", json!(false)),
        ("/metadata", serde_json::Value::Null),
        (
            "/definition/source/parameters/model/model_id",
            json!("other"),
        ),
        ("/vision/analyses/0/result/text", json!("altered")),
    ] {
        let mut forged = view.clone();
        *forged.pointer_mut(pointer).unwrap() = value;
        assert!(
            !eefn::request_result::confirmed(&forged, &command, Some("origin"), Some(&runtime)),
            "{pointer}"
        );
    }
    assert_eq!(json!(reopened.get(&d.job_id).unwrap()), json!(done));
}

#[tokio::test]
async fn vision_completion_requires_both_full_results_and_exact_durable_event_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(&dir.path().join("vision.db"), 8 * 1024 * 1024);
    let (mut issued, node, history) = vision_issued(&store, false).await;
    let d = issued.definition().clone();
    let before = json!(store.get(&d.job_id).unwrap());
    assert!(
        finish_vision(&mut issued, node.clone(), history.clone())
            .await
            .is_err()
    );
    assert_eq!(json!(store.get(&d.job_id).unwrap()), before);
    store
        .queue_workload_event(d.clone(), vision_event(), probe_deadline())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let before = json!(store.get(&d.job_id).unwrap());
    let mut missing = history.clone();
    missing.analyses.clear();
    assert!(
        finish_vision(&mut issued, node.clone(), missing)
            .await
            .is_err()
    );
    assert!(
        finish(&mut issued, node.clone()).await.is_err(),
        "Metadata completion cannot omit vision results"
    );
    assert_eq!(json!(store.get(&d.job_id).unwrap()), before);
    lease(&mut issued, 2).await.unwrap();
    assert_eq!(
        finish_vision(&mut issued, node, history)
            .await
            .unwrap()
            .status,
        PlanStatus::Completed
    );
}

#[tokio::test]
async fn vision_result_storage_is_reserved_before_issuance_or_any_source_effect() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(&dir.path().join("quota.db"), 512 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let approved = vision_approved(&engine, true);
    let before = json!(store.get(&approved.plan_id).unwrap());
    assert!(
        issue(
            &store,
            &execute(&approved),
            &uuid::Uuid::new_v4().to_string(),
            &vision_policy(),
            probe_deadline()
        )
        .await
        .is_err()
    );
    assert_eq!(json!(store.get(&approved.plan_id).unwrap()), before);
    assert!(store.get(&approved.plan_id).unwrap().execution.is_none());
}
