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
fn result_fixture(stage: &str) -> (ExecutionGrant, Value) {
    let task = task(stage);
    let g = grant(&task);
    let output = if stage == "method" {
        json!({"schema_version":1,"method":{"schema_version":1,"template":"system_info","params":{},"constraints":{}},"clarification":null})
    } else {
        proposal(false)
    };
    let candidate = task
        .candidate(&serde_json::to_vec(&output).unwrap(), "ollama", "chosen")
        .unwrap();
    let value = candidate_report(&g, &task, candidate);
    (g, value)
}

#[test]
fn completed_service_reports_revalidate_all_three_stage_sources_and_semantics() {
    for stage in ["initial", "clarification", "method"] {
        let (g, value) = result_fixture(stage);
        let report =
            crate::grants::GrantReport::new(&g, ReceiptState::Completed, value.clone(), None);
        assert!(report.confirms(&g));
        for (pointer, replacement) in [
            ("/task_sha256", json!("0".repeat(64))),
            ("/snapshot_sha256", json!("0".repeat(64))),
            ("/job_id", json!(uuid::Uuid::new_v4().simple().to_string())),
            ("/source_revision", json!(999)),
            ("/model/backend", json!("llamacpp")),
            ("/model/model_id", json!("another")),
            ("/permission_revision", json!(1)),
            (
                "/candidate/stage",
                json!(if stage == "method" {
                    "initial"
                } else {
                    "method"
                }),
            ),
            ("/model_output_is_untrusted", json!(false)),
            ("/execution_authorized", json!(true)),
            ("/delegation_requested", json!(true)),
            ("/automatically_retried", json!(true)),
        ] {
            let mut bad = report.clone();
            *bad.data.pointer_mut(pointer).unwrap() = replacement;
            assert!(!bad.confirms(&g), "stage={stage} pointer={pointer}");
        }
        let mut extra = report;
        extra.data["peer_attestation"] = json!("PRIVATE_UNTRUSTED_CLAIM");
        assert!(!extra.confirms(&g));
    }
}

#[test]
fn service_reports_reject_candidate_authority_evidence_and_undisclosed_capabilities() {
    for stage in ["initial", "clarification", "method"] {
        let (g, data) = result_fixture(stage);
        let report = crate::grants::GrantReport::new(&g, ReceiptState::Completed, data, None);
        for field in ["execution_authorized", "model_output_is_untrusted"] {
            let mut bad = report.clone();
            bad.data["candidate"]["candidate"][field] = json!(field == "execution_authorized");
            assert!(!bad.confirms(&g));
        }
        if stage == "method" {
            for (field, replacement) in [
                ("source_sha256", json!("0".repeat(64))),
                ("source_revision", json!(999)),
                ("source_proposal_count", json!(999)),
                ("backend", json!("llamacpp")),
                ("model_id", json!("another")),
            ] {
                let mut bad = report.clone();
                bad.data["candidate"]["candidate"]["evidence"][field] = replacement;
                assert!(!bad.confirms(&g), "evidence={field}");
            }
            let mut bad = report;
            bad.data["candidate"]["candidate"]["proposal"]["method"]["template"] =
                json!("untrusted_template");
            assert!(!bad.confirms(&g));
        } else {
            let mut bad = report.clone();
            bad.data["candidate"]["candidate"]["proposal"]["suggested_capabilities"] =
                json!(["filesystem.write"]);
            assert!(!bad.confirms(&g));
            let mut bad = report;
            bad.data["candidate"]["candidate"]["original_text"] = json!("Another origin's text");
            assert!(!bad.confirms(&g));
        }
    }
}

#[test]
fn service_result_bounds_and_unknown_reports_preserve_existing_non_execution_semantics() {
    let ordinary = crate::grants::tests::grant();
    assert!(
        crate::grants::GrantReport::new(
            &ordinary,
            ReceiptState::Completed,
            json!({"success":true}),
            None
        )
        .confirms(&ordinary)
    );
    let (g, value) = result_fixture("initial");
    let mut oversized =
        crate::grants::GrantReport::new(&g, ReceiptState::Completed, value.clone(), None);
    oversized.data["padding"] = json!("x".repeat(g.limits.max_result_bytes));
    assert!(!oversized.confirms(&g));
    let too_large = crate::grants::GrantReport::new(
        &g,
        ReceiptState::Completed,
        Value::Null,
        Some("result_too_large"),
    );
    assert!(too_large.confirms(&g));
    assert!(!too_large.success);
    let unknown = crate::grants::GrantReport::new(
        &g,
        ReceiptState::Unknown,
        Value::Null,
        Some("local_policy_changed"),
    );
    assert!(unknown.confirms(&g));
    assert!(!unknown.success);
    let mut invented = unknown;
    invented.data = value;
    assert!(
        !invented.confirms(&g),
        "Unknown outcomes cannot carry a successful candidate"
    );
}
struct Fixture {
    engine: Arc<NodeEngine>,
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
            engine: Arc::new(engine),
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

// Actual authenticated TCP/TLS and production fresh-lease admission, with a
// synthetic issuer/provider. This is not a coordinator broker or physical test.
#[tokio::test]
async fn pinned_service_tasks_use_production_leases_and_retained_results_across_reconnect() {
    use crate::grants::{GrantObserver, GrantReport};
    use std::time::Duration;
    #[derive(Default)]
    struct Observer(Mutex<Vec<GrantReport>>);
    impl GrantObserver for Observer {
        fn record(&self, report: &GrantReport) -> Result<()> {
            self.0.lock().unwrap().push(report.clone());
            Ok(())
        }
    }
    let f = Fixture::new(true, true).await;
    let identity = crate::tls::CoordinatorIdentity::generate().unwrap();
    let key = crate::authentication::generate_credential();
    let gateway = crate::NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        crate::authentication::AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([("owner".into(), Some(key.clone()))]),
            tls: Some(identity.clone()),
        },
    )
    .unwrap();
    gateway.start().await.unwrap();
    let status = Arc::new(Mutex::new(json!({})));
    let config: crate::NodeClientConfig = serde_json::from_value(json!({
        "node_id":"owner","auth_mode":"node_credential","psk":key,
        "coordinator_certificate":identity.certificate,
        "endpoints":[format!("127.0.0.1:{}",gateway.port())],"heartbeat_seconds":0.2
    }))
    .unwrap();
    let start = || {
        let client = crate::NodeClient::new(config.clone(), f.engine.clone())
            .unwrap()
            .with_status(status.clone())
            .with_grants(&f._dir.path().join("grants.db"), Default::default());
        tokio::spawn(async move { client.connect_once().await })
    };
    let ready = || async {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(session) = gateway.grant_session("owner").await {
                    break session;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Private grant connection did not become ready")
    };
    let node = start();
    let session = ready().await;
    let observer = Arc::new(Observer::default());
    let mut completed = Vec::new();
    for stage in ["initial", "clarification", "method", "revoked"] {
        let task = task(if stage == "revoked" { "initial" } else { stage });
        let mut g = grant(&task);
        g.issuer_id = session["issuer_id"].as_str().unwrap().into();
        g.session_id = session["session_id"].as_str().unwrap().into();
        g.executor_node = "owner".into();
        g.issued_at_ms = crate::protocol::now_ms();
        g.limits.max_execution_ms = 10_000;
        g.expires_at_ms = g.issued_at_ms + 10_000;
        let lease_deadline = std::time::Instant::now() + Duration::from_secs(10);
        if stage == "method" {
            *f.response.lock().unwrap() = json!({"done":true,"done_reason":"stop","message":{"content":json!({"schema_version":1,"method":{"schema_version":1,"template":"system_info","params":{},"constraints":{}},"clarification":null}).to_string()}});
        } else {
            *f.response.lock().unwrap() = json!({"done":true,"done_reason":"stop","message":{"content":proposal(false).to_string()}});
        }
        if stage == "revoked" {
            *f.revoke.lock().unwrap() = Some((f.service.clone(), true));
        }
        let before = f.requests.lock().unwrap().len();
        let response = gateway
            .invoke_grant_observed(
                g.clone(),
                Duration::from_secs(15),
                observer.clone(),
                lease_deadline,
            )
            .await
            .unwrap();
        let report: GrantReport = serde_json::from_value(response["grant"].clone()).unwrap();
        assert!(report.confirms(&g) && report.persisted);
        assert_eq!(f.requests.lock().unwrap().len(), before + 1);
        if stage == "revoked" {
            assert_eq!(report.state, ReceiptState::Unknown);
            assert!(!report.success && report.data.is_null());
        } else {
            assert_eq!(
                report.state,
                ReceiptState::Completed,
                "stage={stage} error={:?}",
                report.error_code
            );
            assert!(report.success);
            assert_eq!(report.data["snapshot_sha256"], task.snapshot_sha256);
            assert_eq!(report.data["candidate"]["stage"], stage);
            assert_eq!(
                report.data["candidate"]["candidate"]["model_output_is_untrusted"],
                true
            );
            assert_eq!(report.data["execution_authorized"], false);
            let requests = f.requests.lock().unwrap();
            let body = requests.last().unwrap();
            let spec = task.validate().unwrap();
            assert_eq!(body["messages"], spec.messages);
            assert_eq!(body["options"]["num_predict"], spec.max_output_tokens);
            assert_eq!(body.get("format").cloned(), spec.output_schema);
        }
        let reports = observer.0.lock().unwrap();
        let matching: Vec<_> = reports
            .iter()
            .filter(|r| r.grant_id == g.grant_id)
            .collect();
        assert_eq!(matching.len(), 2);
        assert_eq!(matching[0].state, ReceiptState::Accepted);
        assert_eq!(json!(matching[1]), json!(report));
        drop(reports);
        let retained = gateway.inspect_grant_result(&g).await.unwrap();
        assert!(retained.confirms(&g));
        assert_eq!(json!(retained.report), json!(Some(report.clone())));
        let duplicate = gateway
            .invoke_grant(g.clone(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(duplicate["grant"]["state"], "rejected");
        assert_eq!(f.requests.lock().unwrap().len(), before + 1);
        completed.push((g, report));
    }
    node.abort();
    let _ = node.await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while gateway.connected_nodes().await.contains_key("owner") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let node = start();
    let replacement = ready().await;
    assert_ne!(replacement["session_id"], session["session_id"]);
    let calls = f.requests.lock().unwrap().len();
    for (g, report) in completed {
        let receipt = gateway.inspect_grant_receipt(&g).await.unwrap();
        assert!(receipt.confirms(&g));
        let retained = gateway.inspect_grant_result(&g).await.unwrap();
        assert!(retained.confirms(&g));
        assert_eq!(json!(retained.report), json!(Some(report)));
        assert!(
            gateway
                .invoke_grant(g, Duration::from_secs(1))
                .await
                .is_err()
        );
    }
    assert_eq!(f.requests.lock().unwrap().len(), calls);
    node.abort();
    let _ = node.await;
    gateway.stop().await;
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
