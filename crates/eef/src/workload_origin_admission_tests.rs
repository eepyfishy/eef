// Actual origin mailbox and credential/TLS node source inspection. No Python,
// model, media plugin, device access, approval or execution participates.
struct SourceAdmission {
    _dir: tempfile::TempDir,
    runtime: std::sync::Arc<crate::Runtime>,
    service: std::sync::Arc<eefn::NodeService>,
    engine: std::sync::Arc<eefn::NodeEngine>,
    _binding: eefn::request_preview::PreviewGuard,
    connection: tokio::task::JoinHandle<anyhow::Result<()>>,
    node_path: std::path::PathBuf,
}
impl SourceAdmission {
    async fn new() -> Self {
        let metadata=serde_json::from_value(json!({"resources":[{"id":"mic","capability":"audio.capture","parameters":{"device":"PRIVATE_DEVICE"}}]})).unwrap();
        Self::with_metadata(metadata).await
    }
    async fn with_metadata(metadata: eefn::context::NodeMetadata) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
        let credential = eefn::authentication::generate_credential();
        let owner_path = dir.path().join("owner.json");
        std::fs::write(
            &owner_path,
            serde_json::to_vec(&json!({
                "node":{"psk":"","host":"127.0.0.1","port":0,"authentication":{"allow_legacy":false,
                    "tls":identity,"credentials":{"origin":credential}}},
                "update":{"policy":"off"},"python":{"plugins":[]},
                "firmware":{"repository":dir.path().join("firmware")}
            }))
            .unwrap(),
        )
        .unwrap();
        let runtime = crate::Runtime::initialize(
            crate::config::Config::load(Some(&owner_path)).unwrap(),
            dir.path().join("owner.db"),
            false,
        )
        .await
        .unwrap();
        let node_path = dir.path().join("node.json");
        std::fs::write(
            &node_path,
            serde_json::to_vec(&json!({
                "node_id":"origin","metadata":metadata
            }))
            .unwrap(),
        )
        .unwrap();
        let service = eefn::NodeService::new(node_path.clone(), "origin".into());
        let engine = std::sync::Arc::new(
            eefn::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_resources("origin".into(), metadata.clone())
                .unwrap()
                .with_service(service.clone()),
        );
        let generation = uuid::Uuid::new_v4().to_string();
        let binding = service.attach_preview_runtime(&engine, generation.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(generation);
        let config: eefn::NodeClientConfig = serde_json::from_value(json!({
            "node_id":"origin","metadata":metadata,"auth_mode":"node_credential","psk":credential,
            "coordinator_certificate":identity.certificate,
            "endpoints":[format!("127.0.0.1:{}",runtime.node_server.port())],
            "ollama_url":"http://127.0.0.1:9"
        }))
        .unwrap();
        let node = eefn::NodeClient::new(config, engine.clone())
            .unwrap()
            .with_status(service.live.clone())
            .with_submissions(service.submissions.clone())
            .with_registration_runtime(&generation)
            .unwrap()
            .with_grants(&dir.path().join("node-grants.db"), Default::default());
        let connection = tokio::spawn(async move { node.connect_once().await });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if runtime.node_server.node_state("origin").await.is_some()
                    && service.submissions.state() == eefn::submission::InputChannelState::Ready
                {
                    break;
                }
                assert!(
                    !connection.is_finished(),
                    "Origin connection ended before registration"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            _dir: dir,
            runtime,
            service,
            engine,
            _binding: binding,
            connection,
            node_path,
        }
    }
    async fn input(&self) -> ConcreteInput {
        use eefn::workload_leases::definition::{Source, Template};
        let source = self
            .runtime
            .node_server
            .workload_sources("origin", 5)
            .await
            .unwrap();
        assert!(
            !source.execution_available && !source.capture_started && !source.execution_authorized
        );
        assert!(!source.python_configured && !source.applied_microphone_permission);
        let mut input = crate::workload_jobs::tests::template_input();
        let mut template = Template::from_params(&input.params).unwrap();
        template.executor_node = "origin".into();
        let Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &mut template.source
        else {
            panic!("Expected microphone fixture");
        };
        *resource_id = source.resources[0].resource_id.clone();
        *resource_sha256 = source.resources[0].resource_sha256.clone().unwrap();
        *adapter_sha256 = source.adapter_sha256;
        input.params = json!({"workload":template});
        ConcreteInput {
            schema_version: 1,
            original_text: "Review this bounded microphone source".into(),
            title: "Reviewed microphone source".into(),
            input,
        }
    }
    async fn submit(&self, input: ConcreteInput) -> (ConcreteCommand, Value) {
        let command = ConcreteCommand {
            operation_id: uuid::Uuid::new_v4().to_string(),
            request: input,
        };
        let result = self
            .service
            .concrete_request(eefn::request_submission::ConcreteRequest {
                schema_version: 1,
                expected_node_id: "origin".into(),
                command: command.clone(),
            })
            .await
            .unwrap();
        (command, result)
    }
    async fn disconnect(&mut self) {
        self.connection.abort();
        let _ = (&mut self.connection).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self
                .runtime
                .node_server
                .node_state("origin")
                .await
                .is_some()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    async fn close(mut self) {
        self.disconnect().await;
        self.runtime.shutdown().await;
    }
}

#[tokio::test]
async fn continuous_origin_mailbox_confirms_the_coordinator_result_without_hiding_rejection() {
    let f = SourceAdmission::new().await;
    let input = f.input().await;
    let command = ConcreteCommand {
        operation_id: uuid::Uuid::new_v4().to_string(),
        request: input,
    };
    // This is the same running-origin mailbox used by concrete_request. The
    // synthetic fixture retains a backend rejection in test output instead of
    // replacing it with the public wrapper's conservative unconfirmed report.
    let reply = f
        .service
        .submissions
        .request_correlated(
            "requests.concrete",
            json!(command.request),
            Some(command.operation_id.clone()),
        )
        .await
        .expect("Synthetic continuous origin admission was rejected by the coordinator");
    assert!(
        eefn::request_submission::confirmed(&reply, "origin", &command),
        "Synthetic coordinator receipt failed confirmation: {reply}"
    );
    assert!(
        f.runtime
            .engine
            .store
            .list()
            .unwrap()
            .iter()
            .all(|p| p.status == PlanStatus::AwaitingApproval
                && p.approval.is_none()
                && p.execution.is_none())
    );
    f.close().await;
}

#[tokio::test]
async fn continuous_source_origin_admission_needs_no_one_shot_plugin_or_execution_permission() {
    let mut f = SourceAdmission::new().await;
    assert!(!f.engine.capabilities().contains(&"audio.capture".into()));
    let input = f.input().await;
    let (command, result) = f.submit(input.clone()).await;
    assert_eq!(result["success"], true, "Origin authoring failed: {result}");
    assert!(eefn::request_submission::confirmed(
        &result["result"],
        "origin",
        &command
    ));
    let id = result["result"]["plan"]["id"].as_str().unwrap();
    let saved = f.runtime.engine.store.get(id).unwrap();
    assert_eq!(saved.status, PlanStatus::AwaitingApproval);
    assert!(saved.approval.is_none() && saved.execution.is_none());
    let context = saved.goal.request_context.as_ref().unwrap().clone();
    assert_eq!(context.origin_node(), "origin");
    assert_eq!(context.request_id(), command.operation_id);
    assert!(
        f.service
            .live
            .lock()
            .unwrap()
            .get("last_workload")
            .is_none()
    );
    f.disconnect().await;
    let retained = f
        .runtime
        .origin_concrete_request(input, &context)
        .await
        .unwrap();
    assert!(eefn::request_submission::confirmed(
        &retained, "origin", &command
    ));
    assert_eq!(retained["creation_reused"], true);
    assert_eq!(json!(f.runtime.engine.store.get(id).unwrap()), json!(saved));
    f.runtime.shutdown().await;
}

#[tokio::test]
async fn continuous_origin_admission_refuses_changed_private_binding_adapter_and_undisclosed_source()
 {
    use eefn::workload_leases::definition::{Source, Template};
    let f = SourceAdmission::new().await;
    let input = f.input().await;
    for fault in 0..4 {
        let mut changed = input.clone();
        let mut template = Template::from_params(&changed.input.params).unwrap();
        let Source::MicrophoneEnergy {
            resource_id,
            resource_sha256,
            adapter_sha256,
            ..
        } = &mut template.source
        else {
            panic!("Expected microphone fixture");
        };
        match fault {
            0 => *resource_sha256 = "a".repeat(64),
            1 => *adapter_sha256 = "b".repeat(64),
            2 => *resource_id = "origin::missing".into(),
            _ => {
                template.executor_node = "undisclosed".into();
                *resource_id = "undisclosed::mic".into();
            }
        }
        changed.input.params = json!({"workload":template});
        let (_, refused) = f.submit(changed).await;
        assert_eq!(
            refused["success"], false,
            "Admission fault {fault}: {refused}"
        );
        assert!(f.runtime.engine.store.list().unwrap().is_empty());
    }
    let mut config: Value = serde_json::from_slice(&std::fs::read(&f.node_path).unwrap()).unwrap();
    config["metadata"]["resources"][0]["parameters"]["device"] = json!("CHANGED_PRIVATE_DEVICE");
    std::fs::write(&f.node_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (_, refused) = f.submit(input).await;
    assert_eq!(refused["success"], false);
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
