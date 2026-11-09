use super::*;
use crate::{
    grants::{AcceptedGrant, GrantLedger, GrantPolicy, ReceiptState},
    interpreter_task::{Snapshot, Source},
};
use std::sync::{Arc, Mutex};

fn proposal(question: bool) -> Value {
    json!({"schema_version":1,"intent":"information","goal":"Inspect node status",
        "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],
        "complexity":"simple","workload":"one_shot","needs_clarification":question,
        "clarification":if question {json!("Which node?")}else{Value::Null}})
}
fn task(stage: &str) -> Task {
    let mut draft: crate::request_drafts::DraftRecord = serde_json::from_value(json!({
        "schema_version":1,"original_text":"Inspect node status",
        "proposals":[{"operation_id":uuid::Uuid::new_v4().to_string(),"proposal":proposal(stage=="clarification"),
            "allowed_capabilities":["system.info"],"at_ms":1}],"answers":[]
    })).unwrap();
    if stage == "clarification" {
        let source = crate::request_drafts::AnswerSource::from_draft(3, &draft).unwrap();
        draft
            .answers
            .push(crate::request_drafts::ClarificationAnswer {
                operation_id: uuid::Uuid::new_v4().to_string(),
                proposal_index: 0,
                question: "Which node?".into(),
                text: "This node".into(),
                at_ms: 2,
                source: Some(source),
            });
    }
    let source = match stage {
        "initial" => Source::Initial {
            input: crate::request_intent::Input {
                schema_version: 1,
                original_text: draft.original_text,
            },
        },
        "clarification" => Source::Clarification {
            draft: Box::new(draft),
        },
        "method" => Source::Method {
            draft: Box::new(draft),
        },
        _ => panic!("unknown fixture stage"),
    };
    let tokens = source.max_output_tokens();
    Task::new(
        Snapshot {
            schema_version: 1,
            job_id: uuid::Uuid::new_v4().simple().to_string(),
            revision: if stage == "clarification" { 4 } else { 2 },
            operation_id: uuid::Uuid::new_v4().to_string(),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                "input-node".into(),
                vec!["Bedroom".into()],
            )
            .unwrap(),
            allowed_capabilities: vec!["system.info".into()],
            source,
        },
        tokens,
    )
    .unwrap()
}
fn grant(task: &Task) -> ExecutionGrant {
    let mut grant = crate::grants::tests::grant();
    grant.capability = "llm.infer".into();
    grant.model_id = Some("chosen".into());
    grant.backend = Some("ollama".into());
    grant.model_role = Some("request_interpreter".into());
    grant.job_id = task.snapshot.job_id.clone();
    grant.request_context = task.snapshot.request_context.clone();
    grant.plan_sha256 = definition_digest(task).unwrap();
    grant.params = json!({"model_id":"chosen","backend":"ollama","permission_revision":0,"interpreter_task":task});
    grant.validate().unwrap();
    grant
}
struct Fixture {
    engine: NodeEngine,
    ledger: Arc<GrantLedger>,
    service: Arc<crate::NodeService>,
    requests: Arc<Mutex<Vec<Value>>>,
    response: Arc<Mutex<Value>>,
    revoke: Arc<Mutex<Option<(Arc<crate::NodeService>, bool)>>>,
    server: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn new(enabled: bool, role: bool) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let response = Arc::new(Mutex::new(
            json!({"done":true,"done_reason":"stop","message":{"content":proposal(false).to_string()}}),
        ));
        let revoke: Arc<Mutex<Option<(Arc<crate::NodeService>, bool)>>> =
            Arc::new(Mutex::new(None));
        let (seen, reply, revoked) = (requests.clone(), response.clone(), revoke.clone());
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let (seen, reply, revoked) = (seen.clone(), reply.clone(), revoked.clone());
                async move {
                    seen.lock().unwrap().push(body);
                    if let Some((service, restore)) = revoked.lock().unwrap().take() {
                        let mut config = service.read_config().unwrap();
                        config["permissions"]["interpreter_sharing"] = json!(false);
                        service.save_configuration(config).unwrap();
                        if restore {
                            let mut config = service.read_config().unwrap();
                            config["permissions"]["interpreter_sharing"] = json!(true);
                            service.save_configuration(config).unwrap();
                        }
                    }
                    let body = reply.lock().unwrap().clone();
                    axum::Json(body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let selected: crate::SelectedModel =
            serde_json::from_value(json!({"model_id":"chosen","modality":"text",
            "selection":{"roles":if role {json!(["request_interpreter"])}else{json!([])}}}))
            .unwrap();
        let mut config = crate::setup::defaults();
        config["node_id"] = json!("owner");
        config["name"] = json!("Interpreter fixture");
        config["permissions"]["interpreter_sharing"] = json!(enabled);
        config["models"] = json!({"provider":"ollama","ollama":{"base_url":url,"selected":[selected]},"llamacpp":{"slots":[]}});
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path, &config).unwrap();
        let service = crate::NodeService::new(path, "owner".into());
        let policy = serde_json::from_value(config["permissions"].clone()).unwrap();
        let engine = NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_policy(policy)
            .unwrap()
            .with_ollama(url, [selected])
            .unwrap()
            .with_service(service.clone());
        let ledger = GrantLedger::open(
            &dir.path().join("grants.db"),
            GrantPolicy::default(),
            "owner",
        )
        .unwrap();
        Self {
            engine,
            ledger,
            service,
            requests,
            response,
            revoke,
            server,
            _dir: dir,
        }
    }
    fn accept(&self, g: &ExecutionGrant) -> Result<AcceptedGrant> {
        AcceptedGrant::prepare(
            g.clone(),
            self.ledger.clone(),
            &self.engine,
            &g.issuer_id,
            &g.session_id,
            &g.executor_node,
        )
    }
}

#[test]
fn service_task_grants_bind_source_context_role_model_and_strict_parameters() {
    let original = grant(&task("initial"));
    for field in [
        "job",
        "context",
        "role",
        "model",
        "backend",
        "definition",
        "override",
        "authority",
        "capability",
    ] {
        let mut changed = original.clone();
        match field {
            "job" => changed.job_id = uuid::Uuid::new_v4().simple().to_string(),
            "context" => {
                changed.request_context = crate::context::RequestContext::new(
                    uuid::Uuid::new_v4().to_string(),
                    "other-node".into(),
                    vec![],
                )
                .unwrap()
            }
            "role" => changed.model_role = None,
            "model" => changed.params["model_id"] = json!("substituted"),
            "backend" => changed.params["backend"] = json!("llamacpp"),
            "definition" => changed.plan_sha256 = "0".repeat(64),
            "override" => changed.params["messages"] = json!([]),
            "authority" => changed.params["interpreter_task"]["execution_authorized"] = json!(true),
            "capability" => changed.capability = "system.info".into(),
            _ => unreachable!(),
        }
        assert!(changed.validate().is_err(), "accepted {field}");
    }
    let mut local = original;
    local.backend = Some("llamacpp".into());
    local.params["backend"] = json!("llamacpp");
    assert!(
        local.validate().is_ok(),
        "binding permits the explicit alternate backend, never a fallback"
    );
}

#[tokio::test]
async fn disabled_unapplied_or_unselected_role_cannot_consume_a_grant_or_call_the_model() {
    for (enabled, role) in [(false, true), (true, false)] {
        let f = Fixture::new(enabled, role).await;
        let g = grant(&task("initial"));
        let mut config = f.service.read_config().unwrap();
        config["permissions"]["interpreter_sharing"] = json!(true);
        f.service.save_configuration(config).unwrap();
        assert!(f.accept(&g).is_err());
        assert!(
            f.ledger
                .result(
                    &g.issuer_id,
                    &g.grant_id,
                    &crate::grants::definition_digest(&json!(g)).unwrap()
                )
                .is_err()
        );
        assert!(f.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn all_three_granted_stages_use_exact_existing_inputs_and_retained_results_without_replay() {
    let f = Fixture::new(true, true).await;
    for stage in ["initial", "clarification", "method"] {
        let task = task(stage);
        let spec = task.validate().unwrap();
        let g = grant(&task);
        if stage == "method" {
            *f.response.lock().unwrap() = json!({"done":true,"done_reason":"stop","message":{"content":json!({"schema_version":1,"method":{"schema_version":1,"template":"system_info","params":{},"constraints":{}},"clarification":null}).to_string()}});
        }
        let report = f.accept(&g).unwrap().run(&f.engine).await;
        assert_eq!(
            report.state,
            ReceiptState::Completed,
            "stage={stage} error={:?} requests={}",
            report.error_code,
            f.requests.lock().unwrap().len()
        );
        assert!(report.success && report.persisted);
        assert_eq!(report.data["job_id"], task.snapshot.job_id);
        assert_eq!(report.data["source_revision"], task.snapshot.revision);
        assert_eq!(report.data["snapshot_sha256"], task.snapshot_sha256);
        assert_eq!(report.data["model"]["model_id"], "chosen");
        assert_eq!(report.data["candidate"]["stage"], stage);
        assert_eq!(
            report.data["candidate"]["candidate"]["model_output_is_untrusted"],
            true
        );
        for flag in [
            "execution_authorized",
            "delegation_requested",
            "automatically_retried",
        ] {
            assert_eq!(report.data[flag], false);
        }
        let seen = f.requests.lock().unwrap();
        let body = seen.last().unwrap();
        assert_eq!(body["model"], "chosen");
        assert_eq!(body["messages"], spec.messages);
        assert_eq!(body["options"]["num_predict"], spec.max_output_tokens);
        assert_eq!(body.get("format").cloned(), spec.output_schema);
        let count = seen.len();
        drop(seen);
        assert!(f.accept(&g).is_err());
        assert_eq!(f.requests.lock().unwrap().len(), count);
        let retained = f
            .ledger
            .result(
                &g.issuer_id,
                &g.grant_id,
                &crate::grants::definition_digest(&json!(g)).unwrap(),
            )
            .unwrap();
        assert_eq!(retained["report"], json!(report));
    }
}

#[tokio::test]
async fn incomplete_or_invalid_model_output_is_unknown_and_never_replayed_or_echoed() {
    let f = Fixture::new(true, true).await;
    for complete in [false, true] {
        *f.response.lock().unwrap() = json!({"done":complete,"done_reason":if complete{"stop"}else{"length"},"message":{"content":"PRIVATE_INVALID_OUTPUT"}});
        let g = grant(&task("initial"));
        let before = f.requests.lock().unwrap().len();
        let report = f.accept(&g).unwrap().run(&f.engine).await;
        assert_eq!(report.state, ReceiptState::Unknown);
        assert!(!report.success);
        assert!(report.persisted);
        assert!(report.data.is_null());
        assert!(!json!(report).to_string().contains("PRIVATE_INVALID_OUTPUT"));
        assert_eq!(f.requests.lock().unwrap().len(), before + 1);
        assert!(f.accept(&g).is_err());
        assert_eq!(f.requests.lock().unwrap().len(), before + 1);
    }
}

#[tokio::test]
async fn serving_permission_revoked_in_the_final_provider_poll_cannot_return_a_candidate() {
    for restore in [false, true] {
        let f = Fixture::new(true, true).await;
        *f.revoke.lock().unwrap() = Some((f.service.clone(), restore));
        let g = grant(&task("initial"));
        let report = f.accept(&g).unwrap().run(&f.engine).await;
        assert_eq!(f.requests.lock().unwrap().len(), 1);
        assert_eq!(report.state, ReceiptState::Unknown);
        assert!(report.data.is_null());
        let config = f.service.read_config().unwrap();
        assert_eq!(config["permissions"]["interpreter_sharing"], restore);
        assert_eq!(
            config["permissions"]["interpreter_sharing_revision"],
            if restore { 2 } else { 1 }
        );
        assert!(f.accept(&g).is_err());
    }
}

#[tokio::test]
async fn serving_permission_generation_is_pinned_and_generic_saves_cannot_reset_it() {
    for model_change in [false, true] {
        let f = Fixture::new(true, true).await;
        let mut g = grant(&task("initial"));
        g.params["permission_revision"] = json!(1);
        assert!(g.validate().is_ok());
        assert!(f.accept(&g).is_err());
        let original = f.service.read_config().unwrap();
        let mut revoked = original.clone();
        if model_change {
            revoked["models"]["ollama"]["selected"] = json!([]);
        } else {
            revoked["permissions"]["interpreter_sharing"] = json!(false);
        }
        f.service.save_configuration(revoked).unwrap();
        f.service.save_configuration(original).unwrap();
        assert_eq!(
            f.service.read_config().unwrap()["permissions"]["interpreter_sharing_revision"],
            2
        );
        let g = grant(&task("initial"));
        assert!(f.accept(&g).is_err());
        assert!(f.requests.lock().unwrap().is_empty());
    }
}

#[test]
fn permission_revision_exhaustion_allows_disable_but_never_reenable_or_reset() {
    let enabled =
        json!({"permissions":{"interpreter_sharing":true,"interpreter_sharing_revision":u64::MAX}});
    let mut disabled =
        json!({"permissions":{"interpreter_sharing":false,"interpreter_sharing_revision":0}});
    preserve_permission(&enabled, &mut disabled).unwrap();
    assert_eq!(
        disabled["permissions"]["interpreter_sharing_revision"],
        u64::MAX
    );
    let mut reset =
        json!({"permissions":{"interpreter_sharing":false,"interpreter_sharing_revision":0}});
    preserve_permission(&disabled, &mut reset).unwrap();
    assert_eq!(reset, disabled);
    let mut reenable =
        json!({"permissions":{"interpreter_sharing":true,"interpreter_sharing_revision":0}});
    assert!(preserve_permission(&disabled, &mut reenable).is_err());
}

#[tokio::test]
async fn ordinary_execution_cannot_invoke_a_service_task_without_accepted_grant_admission() {
    let f = Fixture::new(true, true).await;
    let g = grant(&task("initial"));
    let mut params = g.params;
    params["model"] = json!("chosen");
    assert!(f.engine.execute("llm.infer", "run", params).await.is_err());
    assert!(f.requests.lock().unwrap().is_empty());
}
