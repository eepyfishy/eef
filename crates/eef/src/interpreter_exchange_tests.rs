use super::*;
use eefn::{
    context::RequestContext,
    interpreter_policy::{Authorization, Command, Scope},
    interpreter_task::{Snapshot, Source},
};
use serde_json::Value;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn proposal(question: bool) -> Value {
    json!({"schema_version":1,"intent":"information","goal":"Inspect node status","context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot","needs_clarification":question,"clarification":if question{json!("Which node?")}else{Value::Null}})
}
struct Model {
    calls: AtomicUsize,
    requests: Mutex<Vec<Value>>,
    hold: AtomicBool,
    bad: AtomicBool,
    gate: tokio::sync::Semaphore,
}
struct Fixture {
    runtime: Option<Arc<Runtime>>,
    nodes: Vec<tokio::task::JoinHandle<anyhow::Result<()>>>,
    server: Option<tokio::task::JoinHandle<()>>,
    model: Arc<Model>,
    pin: PolicyPin,
    _dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for node in &self.nodes {
            node.abort()
        }
        if let Some(server) = &self.server {
            server.abort()
        }
    }
}
impl Fixture {
    fn runtime(&self) -> &Arc<Runtime> {
        self.runtime.as_ref().unwrap()
    }
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let model = Arc::new(Model {
            calls: AtomicUsize::new(0),
            requests: Mutex::new(vec![]),
            hold: AtomicBool::new(false),
            bad: AtomicBool::new(false),
            gate: tokio::sync::Semaphore::new(0),
        });
        let called = model.clone();
        let app = axum::Router::new()
            .route(
                "/api/tags",
                axum::routing::get(|| async { axum::Json(json!({"models":[{"name":"chosen"}]})) }),
            )
            .route(
                "/api/chat",
                axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                    let model = called.clone();
                    async move {
                        model.requests.lock().unwrap().push(body.clone());
                        model.calls.fetch_add(1, Ordering::SeqCst);
                        if model.hold.load(Ordering::SeqCst) {
                            model.gate.acquire().await.unwrap().forget();
                        }
                        let text = if model.bad.load(Ordering::SeqCst) {
                            "broken model output".into()
                        } else if body["format"].is_object() {
                            proposal(false).to_string()
                        } else {
                            json!({"schema_version":1,"method":null,"clarification":"Which node?"})
                                .to_string()
                        };
                        axum::Json(
                            json!({"done":true,"done_reason":"stop","message":{"content":text}}),
                        )
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
        let keys = std::collections::BTreeMap::from([
            ("input", eefn::authentication::generate_credential()),
            ("provider", eefn::authentication::generate_credential()),
        ]);
        let config = dir.path().join("owner.json");
        std::fs::write(&config,serde_json::to_vec(&json!({"node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,"tls":identity,"credentials":keys}},"discovery":{"grants":{"input":["provider"]},"freshness_seconds":30},"update":{"policy":"off"},"python":{"plugins":[]},"firmware":{"repository":dir.path().join("firmware")}})).unwrap()).unwrap();
        let runtime = Runtime::initialize(
            crate::config::Config::load(Some(&config)).unwrap(),
            dir.path().join("owner.db"),
            false,
        )
        .await
        .unwrap();
        runtime
            .config
            .edit_for_restart(|saved| {
                saved["node"]["port"] = json!(runtime.node_server.port());
                Ok(((), true))
            })
            .unwrap();
        let mut nodes = vec![];
        for node in ["input", "provider"] {
            let serving = node == "provider";
            let selected:eefn::SelectedModel=serde_json::from_value(json!({"model_id":"chosen","modality":"text","selection":{"roles":["request_interpreter"]}})).unwrap();
            let mut settings = eefn::setup::defaults();
            settings["node_id"] = json!(node);
            settings["name"] = json!("Exchange fixture");
            settings["permissions"]["interpreter_sharing"] = json!(serving);
            settings["models"] = json!({"provider":"ollama","ollama":{"base_url":url,"selected":if serving{vec![selected.clone()]}else{vec![]}},"llamacpp":{"slots":[]}});
            let path = dir.path().join(format!("{node}.json"));
            eefn::setup::write_json(&path, &settings).unwrap();
            let service = eefn::NodeService::new(path, node.into());
            let generation = uuid();
            service.live.lock().unwrap()["runtime_id"] = json!(generation);
            let engine = Arc::new(
                eefn::NodeEngine::new(false, vec![], dir.path().into())
                    .unwrap()
                    .with_policy(serde_json::from_value(settings["permissions"].clone()).unwrap())
                    .unwrap()
                    .with_ollama(
                        url.clone(),
                        if serving {
                            vec![selected.clone()]
                        } else {
                            vec![]
                        },
                    )
                    .unwrap()
                    .with_service(service.clone()),
            );
            let client_config:eefn::NodeClientConfig=serde_json::from_value(json!({"node_id":node,"auth_mode":"node_credential","psk":keys[node],"coordinator_certificate":identity.certificate,"endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],"ollama_url":url,"ollama_models":if serving{vec![selected]}else{vec![]},"heartbeat_seconds":0.2})).unwrap();
            let client = eefn::NodeClient::new(client_config, engine)
                .unwrap()
                .with_status(service.live.clone())
                .with_registration_runtime(&generation)
                .unwrap()
                .with_grants(
                    &dir.path().join(format!("{node}-grants.db")),
                    Default::default(),
                );
            nodes.push(tokio::spawn(async move { client.connect_once().await }));
        }
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if runtime.model_registry.list().len() == 1
                    && runtime
                        .node_server
                        .interpreter_session("input", 30)
                        .await
                        .is_ok()
                    && runtime
                        .node_server
                        .interpreter_session("provider", 30)
                        .await
                        .is_ok()
                    && runtime
                        .registry
                        .find("llm.infer", "run")
                        .iter()
                        .any(|p| p.node_id == "provider")
                {
                    break;
                }
                if let Some(index) = nodes.iter().position(|n| n.is_finished()) {
                    let result = nodes.swap_remove(index).await;
                    panic!("Private exchange node ended before registration: {result:?}")
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let saved = runtime
            .interpreter_policy_command(eefn::interpreter_policy::Request {
                schema_version: 1,
                expected_runtime_id: runtime.runtime_id().into(),
                command: Command::Approve {
                    operation_id: uuid(),
                    generation: 0,
                    scope: Scope {
                        origin_nodes: vec!["input".into()],
                        provider_nodes: vec!["provider".into()],
                    },
                    limits: Default::default(),
                    valid_for_ms: None,
                },
            })
            .unwrap();
        let policy: Authorization = serde_json::from_value(saved["authorization"].clone()).unwrap();
        let pin = PolicyPin::from(&policy);
        Self {
            runtime: Some(runtime),
            nodes,
            server: Some(server),
            model,
            pin,
            _dir: dir,
        }
    }
    async fn participants(&self) -> Participants {
        let server = &self.runtime().node_server;
        Participants {
            origin: server.interpreter_session("input", 30).await.unwrap(),
            provider: server.interpreter_session("provider", 30).await.unwrap(),
            backend: "ollama".into(),
            model_id: "chosen".into(),
        }
    }
    async fn task(&self, stage: &str) -> Task {
        let runtime = self.runtime();
        let capabilities = runtime
            .disclosed_interpretation_capabilities("input")
            .await
            .unwrap();
        let context = RequestContext::new(
            uuid(),
            "input".into(),
            vec!["home".into(), "bedroom".into()],
        )
        .unwrap();
        let saved = if stage == "initial" {
            crate::request_intent::save(
                &runtime.engine,
                json!({"schema_version":1,"original_text":"Inspect node status"}),
                &context,
            )
            .unwrap()
        } else {
            let opened=crate::request_drafts::open(&runtime.engine,json!({"schema_version":1,"original_text":"Inspect node status","proposal":proposal(stage=="clarification")}),&context,&capabilities).unwrap();
            if stage == "clarification" {
                let answer = RequestContext::new(uuid(), "input".into(), vec![]).unwrap();
                crate::request_drafts::command(&runtime.engine,"requests.draft_answer",json!({"id":opened["id"],"expected_revision":opened["revision"],"text":"This node"}),&answer,&capabilities).unwrap()
            } else {
                opened
            }
        };
        let source = match stage {
            "initial" => Source::Initial {
                input: serde_json::from_value(saved["input"].clone()).unwrap(),
            },
            "clarification" => Source::Clarification {
                draft: Box::new(serde_json::from_value(saved["draft"].clone()).unwrap()),
            },
            _ => Source::Method {
                draft: Box::new(serde_json::from_value(saved["draft"].clone()).unwrap()),
            },
        };
        Task::new(
            Snapshot {
                schema_version: 1,
                job_id: saved["id"].as_str().unwrap().into(),
                revision: saved["revision"].as_u64().unwrap(),
                operation_id: uuid(),
                request_context: context,
                allowed_capabilities: capabilities,
                source,
            },
            128,
        )
        .unwrap()
    }
    async fn called(&self) {
        tokio::time::timeout(Duration::from_secs(15), async {
            while self.model.calls.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
        })
        .await
        .unwrap();
    }
    async fn close(mut self) {
        self.model.gate.add_permits(64);
        for node in self.nodes.drain(..) {
            node.abort();
            let _ = node.await;
        }
        self.runtime
            .take()
            .unwrap()
            .retire(Duration::from_secs(5))
            .await
            .unwrap();
        if let Some(server) = self.server.take() {
            server.abort();
            let _ = server.await;
        }
    }
}

#[tokio::test]
async fn real_core_allowances_exchange_all_three_stages_through_private_grants_without_promotion_or_replay()
 {
    let f = Fixture::new().await;
    for (index, stage) in ["initial", "clarification", "method"]
        .into_iter()
        .enumerate()
    {
        let task = f.task(stage).await;
        let participants = f.participants().await;
        assert!(!participants.origin.serves("ollama", "chosen"));
        let before = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
        let outcome = f
            .runtime()
            .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
            .await
            .unwrap();
        let Exchange::Completed(completed) = outcome else {
            panic!("Missing live completion for {stage}")
        };
        let record = completed.record();
        let grant = &record.binding.grant;
        assert_eq!(grant.request_context, task.snapshot.request_context);
        assert_eq!(grant.executor_node, "provider");
        assert_eq!(grant.backend.as_deref(), Some("ollama"));
        assert_eq!(grant.model_id.as_deref(), Some("chosen"));
        assert_eq!(record.stage().unwrap(), stage);
        assert!(record.report.as_ref().unwrap().confirms(grant));
        assert!(record.report.as_ref().unwrap().success);
        assert_eq!(f.model.calls.load(Ordering::SeqCst), index + 1);
        let request = f.model.requests.lock().unwrap()[index].clone();
        assert_eq!(request["messages"], task.validate().unwrap().messages);
        assert_eq!(request["options"]["num_predict"], 128);
        let plan = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
        assert!(
            plan.tasks.is_empty()
                && plan.planning.is_none()
                && plan.approval.is_none()
                && plan.execution.is_none()
        );
        assert_eq!(json!(plan.request_review), json!(before.request_review));
        drop(completed);
        if stage == "initial" {
            assert_eq!(
                f.runtime()
                    .engine
                    .store
                    .get(&task.snapshot.job_id)
                    .unwrap()
                    .input_interpretation
                    .unwrap()
                    .state,
                eefn::request_interpretation::State::Unknown
            )
        }
        assert!(matches!(
            f.runtime()
                .exchange_interpreter(task, f.pin.clone(), &participants)
                .await
                .unwrap(),
            Exchange::Historical(_)
        ));
        assert_eq!(f.model.calls.load(Ordering::SeqCst), index + 1);
        assert!(
            f.runtime()
                .registry
                .find("llm.infer", "run")
                .iter()
                .all(|p| p.in_flight == 0)
        );
    }
    assert_eq!(
        f.runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .1[&f.pin.id]
            .charged
            .calls,
        3
    );
    f.close().await;
}

#[tokio::test]
async fn dropped_live_exchange_retires_exact_ownership_without_reconstructing_a_continuation() {
    let f = Fixture::new().await;
    f.model.hold.store(true, Ordering::SeqCst);
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let (runtime, pin, request) = (f.runtime().clone(), f.pin.clone(), task.clone());
    let running = tokio::spawn(async move {
        runtime
            .exchange_interpreter(request, pin, &participants)
            .await
    });
    f.called().await;
    running.abort();
    assert!(matches!(running.await,Err(error) if error.is_cancelled()));
    let records = f
        .runtime()
        .engine
        .store
        .interpreter_allowance_records()
        .unwrap()
        .0;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].phase,
        crate::interpreter_allowance::Phase::Interrupted
    );
    assert!(records[0].holds_slot());
    assert_eq!(
        f.runtime()
            .engine
            .store
            .get(&task.snapshot.job_id)
            .unwrap()
            .input_interpretation
            .unwrap()
            .state,
        eefn::request_interpretation::State::Unknown
    );
    let participants = f.participants().await;
    assert!(matches!(
        f.runtime()
            .exchange_interpreter(task, f.pin.clone(), &participants)
            .await
            .unwrap(),
        Exchange::Historical(_)
    ));
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn source_policy_settings_and_peer_changes_stop_one_exact_exchange_without_substitution() {
    for change in ["source", "policy", "settings", "peer"] {
        let f = Fixture::new().await;
        f.model.hold.store(true, Ordering::SeqCst);
        let task = f.task("initial").await;
        let participants = f.participants().await;
        let (runtime, pin, request) = (f.runtime().clone(), f.pin.clone(), task.clone());
        let running = tokio::spawn(async move {
            runtime
                .exchange_interpreter(request, pin, &participants)
                .await
        });
        f.called().await;
        match change {
            "source" => {
                f.runtime().engine.cancel(&task.snapshot.job_id).unwrap();
            }
            "policy" => {
                f.runtime()
                    .interpreter_policy_command(eefn::interpreter_policy::Request {
                        schema_version: 1,
                        expected_runtime_id: f.runtime().runtime_id().into(),
                        command: Command::Revoke {
                            id: f.pin.id.clone(),
                            revision: 1,
                            operation_id: uuid(),
                        },
                    })
                    .unwrap();
            }
            "settings" => {
                f.runtime()
                    .config
                    .edit_for_restart(|saved| {
                        saved["node"]["authentication"]["credentials"]["input"] = Value::Null;
                        Ok(((), true))
                    })
                    .unwrap();
            }
            _ => f.nodes[1].abort(),
        }
        let result = tokio::time::timeout(Duration::from_secs(10), running)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err(), "{change}");
        let records = f
            .runtime()
            .engine
            .store
            .interpreter_allowance_records()
            .unwrap()
            .0;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert!(record.holds_slot());
        assert!(record.stop.is_some(), "{change}");
        if let Some(report) = &record.stop.as_ref().unwrap().report {
            assert!(!report.execution_stopped && !report.external_effects_stopped);
            assert!(report.confirms(&record.binding.grant));
        }
        assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
        let plan = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
        assert!(plan.request_review.is_none() && plan.tasks.is_empty() && plan.execution.is_none());
        assert_eq!(
            plan.input_interpretation.unwrap().state,
            eefn::request_interpretation::State::Unknown
        );
        f.close().await;
    }
}

#[tokio::test]
async fn malformed_model_output_remains_failed_and_never_becomes_a_review_or_another_inference() {
    let f = Fixture::new().await;
    f.model.bad.store(true, Ordering::SeqCst);
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let outcome = f
        .runtime()
        .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
        .await
        .unwrap();
    let Exchange::Failed(record) = outcome else {
        panic!("Malformed output acquired a live completion")
    };
    assert!(!record.report.as_ref().unwrap().success);
    assert!(record.report.as_ref().unwrap().data.is_null());
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    let plan = f.runtime().engine.store.get(&task.snapshot.job_id).unwrap();
    assert!(
        plan.request_review.is_none()
            && plan.tasks.is_empty()
            && plan.approval.is_none()
            && plan.execution.is_none()
    );
    assert!(matches!(
        f.runtime()
            .exchange_interpreter(task, f.pin.clone(), &participants)
            .await
            .unwrap(),
        Exchange::Historical(_)
    ));
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 1);
    f.close().await;
}

#[tokio::test]
async fn existing_scheduler_capacity_is_reserved_before_any_allowance_charge_or_source_claim() {
    let f = Fixture::new().await;
    let task = f.task("initial").await;
    let participants = f.participants().await;
    let mut provider = f
        .runtime()
        .registry
        .find("llm.infer", "run")
        .into_iter()
        .find(|p| p.node_id == "provider")
        .unwrap();
    provider.max_concurrent = 1;
    f.runtime().registry.register(provider);
    let held = f
        .runtime()
        .registry
        .reserve_best_with_context(
            "llm.infer",
            "run",
            &json!({"node_id":"provider"}),
            Some(&task.snapshot.request_context),
        )
        .unwrap();
    let before = f.runtime().engine.store.usage().unwrap();
    assert!(
        f.runtime()
            .exchange_interpreter(task.clone(), f.pin.clone(), &participants)
            .await
            .is_err()
    );
    assert_eq!(f.runtime().engine.store.usage().unwrap(), before);
    assert_eq!(f.model.calls.load(Ordering::SeqCst), 0);
    assert!(
        f.runtime()
            .engine
            .store
            .get(&task.snapshot.job_id)
            .unwrap()
            .input_interpretation
            .is_none()
    );
    drop(held);
    f.close().await;
}
