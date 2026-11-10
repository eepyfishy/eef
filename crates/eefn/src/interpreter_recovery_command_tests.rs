use super::*;
fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn request() -> Request {
    Request {
        schema_version: 1,
        expected_node_id: "owner".into(),
        expected_runtime_id: uuid(),
        command: Command::Inspect {
            id: uuid::Uuid::new_v4().simple().to_string(),
            operation_id: uuid(),
        },
    }
}
fn report(request: &Request) -> Value {
    json!(Report {
        schema_version: 1,
        report_type: "coordinator_interpreter_recovery".into(),
        runtime_id: uuid(),
        origin_node: request.expected_node_id.clone(),
        command: request.command.clone(),
        revision: 4,
        status: "review".into(),
        attempts: vec![],
        truncated: false,
        recovery: None,
        reused: false,
        model_inference_requested: false,
        execution_authorized: false,
        dispatched: false,
        automatically_retried: false
    })
}
#[test]
fn interpreter_recovery_commands_accept_references_only_and_separate_inspection_from_application() {
    let mut req = request();
    req.validate().unwrap();
    let apply = Command::Recover {
        command: crate::interpreter_recovery::Command {
            id: req.command.id().into(),
            expected_revision: 4,
            operation_id: uuid(),
            grant_id: uuid(),
        },
    };
    apply.validate().unwrap();
    for command in [req.command.clone(), apply] {
        for key in [
            "prompt",
            "proposal",
            "result",
            "claim",
            "actor",
            "permission",
            "execute",
        ] {
            let mut injected = json!(command);
            injected[key] = json!(true);
            assert!(
                serde_json::from_value::<Command>(injected).is_err(),
                "{key}"
            );
        }
    }
    req.expected_runtime_id = uuid::Uuid::nil().to_string();
    assert!(req.validate().is_err());
    req = request();
    if let Command::Inspect { operation_id, .. } = &mut req.command {
        *operation_id = uuid::Uuid::nil().to_string();
    }
    assert!(req.validate().is_err());
}
#[test]
fn interpreter_inspection_confirmation_rejects_authority_and_mismatched_or_unbounded_headers() {
    let req = request();
    let original = report(&req);
    assert!(confirmed(&original, &req.command, "owner"));
    for key in [
        "model_inference_requested",
        "execution_authorized",
        "dispatched",
        "automatically_retried",
        "reused",
    ] {
        let mut bad = original.clone();
        bad[key] = json!(true);
        assert!(!confirmed(&bad, &req.command, "owner"), "{key}");
    }
    assert!(!confirmed(&original, &req.command, "foreign"));
    let attempt = json!({"attempt":{"id":uuid(),"operation_id":uuid(),"source_revision":2,"phase":"not_sent",
        "provider":"peer","backend":"ollama","model_id":"chosen","execution_limit_ms":5000},
        "stage":"initial","completed_result":false,"source_applied":false});
    let mut headers = original.clone();
    headers["attempts"] = json!([attempt.clone()]);
    assert!(confirmed(&headers, &req.command, "owner"));
    headers["attempts"][0]["completed_result"] = json!(true);
    assert!(!confirmed(&headers, &req.command, "owner"));
    headers["attempts"] = json!([attempt.clone(), attempt.clone()]);
    assert!(!confirmed(&headers, &req.command, "owner"));
    headers["attempts"] = json!(vec![attempt; 49]);
    assert!(!confirmed(&headers, &req.command, "owner"));
}
#[test]
fn interpreter_recovery_confirmation_pins_the_new_decision_and_preserves_uncertainty() {
    let mut req = request();
    req.command = Command::Recover {
        command: crate::interpreter_recovery::Command {
            id: req.command.id().into(),
            expected_revision: 4,
            operation_id: uuid(),
            grant_id: uuid(),
        },
    };
    let Command::Recover { command } = &req.command else {
        unreachable!()
    };
    let receipt = crate::interpreter_recovery::Record {
        schema_version: 1,
        command: command.clone(),
        actor: crate::interpreter_recovery::RecoveryActor::OriginNode,
        stage: crate::interpreter_command::Stage::Initial,
        source_revision: 1,
        applied_revision: 5,
        decided_at_ms: 1,
        binding_sha256: "a".repeat(64),
        report_sha256: "b".repeat(64),
        proposal_sha256: "c".repeat(64),
        capabilities_sha256: "d".repeat(64),
    };
    let mut result = report(&req);
    result["revision"] = json!(5);
    result["recovery"] = json!(receipt);
    assert!(confirmed(&result, &req.command, "owner"));
    let original = result.clone();
    for key in ["operation_id", "grant_id"] {
        let mut bad = original.clone();
        bad["recovery"]["command"][key] = json!(uuid());
        assert!(!confirmed(&bad, &req.command, "owner"));
    }
    result["recovery"]["actor"] = json!("coordinator_owner");
    assert!(!confirmed(&result, &req.command, "owner"));
    let unknown = unconfirmed(&req);
    assert!(confirmed_node(&unknown, &req));
    assert!(render(&unknown).unwrap().contains("outcome is unconfirmed"));
    let plain = render(&unknown).unwrap();
    assert!(plain.contains(&format!(
        "eefn requests interpreter-results {}",
        req.command.id()
    )));
    assert!(plain.contains("do not assume the source stayed unchanged"));
    assert!(!plain.contains("--grant-id"));
    let mut forged = unknown;
    forged["success"] = json!(true);
    assert!(!confirmed_node(&forged, &req));
}
#[tokio::test]
async fn interpreter_recovery_shared_admission_and_runtime_retirement_prevent_late_success() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "owner".into());
    let engine =
        std::sync::Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let mut req = request();
    let runtime = req.expected_runtime_id.clone();
    let unavailable = service
        .interpreter_recovery_command(req.clone())
        .await
        .unwrap();
    assert_eq!(unavailable["error_code"], "runtime_unavailable");
    let guard = service.attach_preview_runtime(&engine, runtime);
    let mut receiver = service.submissions.connect();
    req.command = Command::Recover {
        command: crate::interpreter_recovery::Command {
            id: req.command.id().into(),
            expected_revision: 4,
            operation_id: uuid(),
            grant_id: uuid(),
        },
    };
    let held = service.input_workflow_permit().unwrap();
    let busy = service
        .interpreter_recovery_command(req.clone())
        .await
        .unwrap();
    assert_eq!(busy["error_code"], "busy");
    assert!(
        render(&busy)
            .unwrap()
            .contains("busy with another input or recovery operation")
    );
    assert!(!busy["outcome_unknown"].as_bool().unwrap());
    drop(held);
    req.expected_runtime_id = uuid();
    let stale = service
        .interpreter_recovery_command(req.clone())
        .await
        .unwrap();
    assert_eq!(stale["error_code"], "runtime_changed");
    assert!(render(&stale).unwrap().contains("node runtime changed"));
    assert!(receiver.try_recv().is_err());
    req.expected_runtime_id = service.input_workflow_runtime().unwrap().0;
    let (reply, late) = tokio::join!(service.interpreter_recovery_command(req.clone()), async {
        let submitted = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submitted.kind, "requests.interpreter_recovery");
        assert_eq!(
            submitted.operation_id.as_deref(),
            Some(req.command.operation_id())
        );
        drop(guard);
        submitted.reply
    });
    let reply = reply.unwrap();
    assert!(confirmed_node(&reply, &req));
    assert_eq!(reply["error_code"], "runtime_changed");
    assert_eq!(reply["outcome_unknown"], true);
    assert!(late.send(Ok(report(&req))).is_err());
    assert!(receiver.try_recv().is_err());
    assert!(service.input_workflow_permit().is_ok());
}

#[tokio::test]
async fn interpreter_inspection_uses_bounded_submission_without_borrowing_input_mutation_admission()
{
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "owner".into());
    let engine =
        std::sync::Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let req = request();
    let _guard = service.attach_preview_runtime(&engine, req.expected_runtime_id.clone());
    let held = service.input_workflow_permit().unwrap();
    let mut receiver = service.submissions.connect();
    let (reply, ()) = tokio::join!(service.interpreter_recovery_command(req.clone()), async {
        let submitted = tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(submitted.kind, "requests.interpreter_recovery");
        let wire: WireRequest = serde_json::from_value(submitted.payload).unwrap();
        assert_eq!(wire.command, req.command);
        assert!(matches!(wire.command, Command::Inspect { .. }));
        assert!(service.input_workflow_permit().is_err());
        submitted.reply.send(Ok(report(&req))).unwrap();
    });
    let reply = reply.unwrap();
    assert!(confirmed_node(&reply, &req));
    assert_eq!(reply["success"], true);
    assert!(service.input_workflow_permit().is_err());
    assert!(receiver.try_recv().is_err());
    drop(held);
    assert!(service.input_workflow_permit().is_ok());
}
