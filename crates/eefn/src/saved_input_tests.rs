use super::*;
use std::sync::Arc;

fn request() -> Request {
    Request {
        schema_version: 1,
        expected_node_id: "origin-node".into(),
        expected_runtime_id: uuid::Uuid::new_v4().to_string(),
        command: crate::request_interpretation::Command {
            id: uuid::Uuid::new_v4().simple().to_string(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            inference_timeout_seconds: None,
        },
    }
}
fn raw(r: &Request) -> Value {
    json!({"id":r.command.id,"revision":1,"status":"review","review_only":true,"task_count":0,"proposal_count":0,"planning_state":"not_interpreted","can_resume":false,
        "input_intent":{"schema_version":1,"original_text":"PRIVATE_SAVED_INPUT"},
        "request_context":{"request_id":uuid::Uuid::new_v4().to_string(),"origin_node":"origin-node","origin_area":["home","bedroom"]}})
}
fn pending_source(r: &Request, state: State) -> Value {
    let mut value = raw(r);
    let input: crate::request_intent::Input =
        serde_json::from_value(value["input_intent"].clone()).unwrap();
    let context: crate::context::RequestContext =
        serde_json::from_value(value["request_context"].clone()).unwrap();
    let record = Record {
        schema_version: 1,
        claim: Claim {
            id: r.command.id.clone(),
            expected_revision: 1,
            source_sha256: input.digest(&context).unwrap(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            origin_runtime_id: r.expected_runtime_id.clone(),
            interpreter: None,
        },
        input,
        request_context: context,
        claimed_at_ms: 1,
        state,
        promoted_revision: None,
        proposal_sha256: None,
    };
    value["revision"] = json!(2);
    value["input_interpretation"] = json!(record);
    value
}

#[tokio::test]
async fn saved_interpretation_never_launches_a_stopped_node_or_writes_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.json");
    let service = crate::NodeService::new(path.clone(), "origin-node".into());
    let r = request();
    let result = service.interpret_saved_input(r.clone()).await.unwrap();
    assert!(confirmed(&result, &r));
    assert_eq!(result["error_code"], "runtime_unavailable");
    assert_eq!(result["claim_attempted"], false);
    assert!(!path.exists());
}
#[tokio::test]
async fn missing_interpreter_preserves_raw_source_without_claiming_or_calling_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.json");
    let service = crate::NodeService::new(path.clone(), "origin-node".into());
    let r = request();
    let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let _guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
    let mut receiver = service.submissions.connect();
    let local = service.clone();
    let command = r.clone();
    let task = tokio::spawn(async move { local.interpret_saved_input(command).await });
    let get = receiver.recv().await.unwrap();
    assert_eq!(get.kind, "jobs.get");
    get.reply.send(Ok(raw(&r))).unwrap();
    let result = task.await.unwrap().unwrap();
    assert!(confirmed(&result, &r));
    assert_eq!(result["error_code"], "interpreter_unavailable");
    assert_eq!(
        result["source"]["input"]["original_text"],
        "PRIVATE_SAVED_INPUT"
    );
    assert_eq!(result["claim_attempted"], false);
    assert_eq!(result["model_inference_requested"], false);
    assert_eq!(result["outcome_unknown"], false);
    let plain = render(&result).unwrap();
    assert!(plain.contains("remains saved"));
    assert!(!plain.contains("PRIVATE_SAVED_INPUT"));
    assert!(receiver.try_recv().is_err());
    assert!(!path.exists());
}
#[tokio::test]
async fn claimed_or_unknown_source_is_inspection_only_even_without_an_interpreter() {
    for state in [State::Claimed, State::Unknown] {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::NodeService::new(dir.path().join("node.json"), "origin-node".into());
        let r = request();
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
        let mut receiver = service.submissions.connect();
        let local = service.clone();
        let command = r.clone();
        let task = tokio::spawn(async move { local.interpret_saved_input(command).await });
        receiver
            .recv()
            .await
            .unwrap()
            .reply
            .send(Ok(pending_source(&r, state)))
            .unwrap();
        let result = task.await.unwrap().unwrap();
        assert!(confirmed(&result, &r));
        assert_eq!(result["error_code"], "existing_attempt");
        assert_eq!(result["historical_snapshot"], true);
        assert_eq!(result["outcome_unknown"], true);
        assert_eq!(result["model_inference_requested"], false);
        assert!(receiver.try_recv().is_err());
    }
}
#[tokio::test]
async fn same_id_runtime_replacement_fences_the_lookup_before_claim_or_inference() {
    let dir = tempfile::tempdir().unwrap();
    let service = crate::NodeService::new(dir.path().join("node.json"), "origin-node".into());
    let r = request();
    let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
    let guard = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
    let mut receiver = service.submissions.connect();
    let local = service.clone();
    let command = r.clone();
    let task = tokio::spawn(async move { local.interpret_saved_input(command).await });
    let get = receiver.recv().await.unwrap();
    let _next = service.attach_preview_runtime(&engine, r.expected_runtime_id.clone());
    drop(guard);
    let _ = get.reply.send(Ok(raw(&r)));
    let result = task.await.unwrap().unwrap();
    assert!(confirmed(&result, &r));
    assert_eq!(result["error_code"], "runtime_changed");
    assert_eq!(result["claim_attempted"], false);
    assert_eq!(result["model_inference_requested"], false);
    assert!(receiver.try_recv().is_err());
}
#[test]
fn client_confirmation_rejects_fake_model_authority_and_wrong_receipts() {
    let r = request();
    let report = unconfirmed(&r);
    assert!(confirmed(&report, &r));
    for field in [
        "execution_authorized",
        "dispatched",
        "automatically_retried",
        "model_inference_requested",
        "claim_confirmed",
        "promotion_attempted",
    ] {
        let mut forged = report.clone();
        forged[field] = json!(true);
        assert!(!confirmed(&forged, &r), "{field}");
    }
    let mut forged = report;
    forged["operation_id"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(!confirmed(&forged, &r));
}
