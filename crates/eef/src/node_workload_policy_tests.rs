use super::*;

fn set(runtime: Option<String>) -> Request {
    Request {
        schema_version: 1,
        node_id: "node".into(),
        expected_runtime_id: runtime,
        command: Command::Set {
            policy: eefn::workload_leases::Policy {
                enabled: true,
                max_session_ms: 60000,
                max_lease_ms: 10000,
            },
            expected_revision: 0,
            expected_epoch: 0,
        },
    }
}
#[test]
fn remote_workload_mutation_requires_exact_runtime_valid_limits_and_revision() {
    let mut request = set(None);
    assert!(request.validate().is_err());
    request.expected_runtime_id = Some(uuid::Uuid::new_v4().to_string());
    request.validate().unwrap();
    if let Command::Set { policy, .. } = &mut request.command {
        policy.max_lease_ms = 0;
    }
    assert!(request.validate().is_err());
    let mut request = set(Some(uuid::Uuid::new_v4().to_string()));
    if let Command::Set {
        expected_revision, ..
    } = &mut request.command
    {
        *expected_revision = u64::MAX;
    }
    assert!(request.validate().is_err());
    assert!(!confirmed(&json!({"success":true}), &request));
    assert!(
        serde_json::from_value::<Request>(
            json!({"schema_version":1,"node_id":"node","expected_runtime_id":null,
        "command":{"operation":"show"},"unexpected":true})
        )
        .is_err()
    );
}

#[test]
fn remote_workload_confirmation_binds_node_runtime_command_and_no_effect_flags() {
    let runtime = uuid::Uuid::new_v4().to_string();
    let request = set(Some(runtime.clone()));
    let policy = match &request.command {
        Command::Set { policy, .. } => policy,
        _ => unreachable!(),
    };
    let result = json!({"schema_version":1,"report_type":"node_workload_policy","success":true,
        "node_id":"node","runtime_id":runtime,"command":request.command,"policy":policy,"revision":1,
        "runtime_epoch":1,"runtime_blocked":false,"changed":true,"execution_authorized":false,"capture_started":false,
        "restart_requested":false,"automatically_retried":false});
    let report = json!(Report {
        schema_version: 1,
        report_type: "remote_workload_policy".into(),
        node_id: "node".into(),
        command: request.command.clone(),
        success: true,
        mutation_requested: true,
        sending_attempted: true,
        outcome_unknown: false,
        result: Some(result),
        execution_authorized: false,
        capture_started: false,
        restart_requested: false,
        automatically_retried: false
    });
    assert!(confirmed(&report, &request));
    for field in [
        "capture_started",
        "execution_authorized",
        "restart_requested",
        "automatically_retried",
        "outcome_unknown",
    ] {
        let mut wrong = report.clone();
        wrong[field] = json!(true);
        assert!(!confirmed(&wrong, &request), "{field}");
    }
    let mut wrong = report.clone();
    wrong["result"]["runtime_id"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(!confirmed(&wrong, &request));
    let mut wrong = report;
    wrong["result"]["policy"]["max_session_ms"] = json!(120000);
    assert!(!confirmed(&wrong, &request));
}

#[tokio::test]
async fn real_credential_tls_node_policy_commands_share_owner_state_without_source_or_permission_changes()
 {
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
    let key = eefn::authentication::generate_credential();
    let server = eefn::NodeServer::with_authentication(
        "",
        "127.0.0.1",
        0,
        eefn::authentication::AuthenticationPolicy {
            allow_legacy: false,
            credentials: std::collections::BTreeMap::from([("node".into(), Some(key.clone()))]),
            tls: Some(identity.clone()),
        },
    )
    .unwrap();
    server.start().await.unwrap();
    let path = dir.path().join("node.json");
    eefn::setup::write_json(
        &path,
        &json!({"node_id":"node","psk":key,"endpoints":[],
        "permissions":{"media":{"microphone":false}},"management":{"allow_remote":true}}),
    )
    .unwrap();
    let service = eefn::NodeService::new(path, "node".into());
    let runtime = uuid::Uuid::new_v4().to_string();
    service.live.lock().unwrap()["runtime_id"] = json!(runtime);
    let engine = Arc::new(
        eefn::NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_service(service.clone()),
    );
    let _guard = service.attach_preview_runtime(&engine, runtime.clone());
    let config: eefn::NodeClientConfig = serde_json::from_value(json!({"node_id":"node","psk":key,
        "auth_mode":"node_credential","coordinator_certificate":identity.certificate,
        "endpoints":[format!("127.0.0.1:{}", server.port())],"heartbeat_seconds":0.2,"ollama_url":"http://127.0.0.1:9"})).unwrap();
    let client = eefn::NodeClient::new(config, engine)
        .unwrap()
        .with_status(service.live.clone())
        .with_registration_runtime(&runtime)
        .unwrap()
        .with_grants(&dir.path().join("grants.db"), Default::default());
    let running = tokio::spawn(async move { client.connect_once().await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.grant_session("node").await.is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut request = Request {
        schema_version: 1,
        node_id: "node".into(),
        expected_runtime_id: None,
        command: Command::Show {},
    };
    let shown = command(&server, request.clone()).await.unwrap();
    assert!(confirmed(&shown, &request));
    assert_eq!(shown["result"]["policy"]["enabled"], false);
    request.expected_runtime_id = Some(runtime.clone());
    request.command = Command::Set {
        policy: eefn::workload_leases::Policy {
            enabled: true,
            max_session_ms: 60000,
            max_lease_ms: 10000,
        },
        expected_revision: shown["result"]["revision"].as_u64().unwrap(),
        expected_epoch: shown["result"]["runtime_epoch"].as_u64().unwrap(),
    };
    let enabled = command(&server, request.clone()).await.unwrap();
    assert!(confirmed(&enabled, &request));
    assert_eq!(enabled["result"]["policy"]["max_session_ms"], 60000);
    let local_show = service
        .workload_policy_command(
            eefn::workload_policy::Request {
                schema_version: 1,
                expected_node_id: "node".into(),
                expected_runtime_id: runtime.clone(),
                command: Command::Show {},
            },
            false,
        )
        .unwrap();
    assert_eq!(local_show["policy"], enabled["result"]["policy"]);
    let stale = command(&server, request.clone()).await.unwrap();
    assert_eq!(stale["success"], false);
    assert_eq!(stale["sending_attempted"], true);
    assert_eq!(stale["outcome_unknown"], true);
    request.command = Command::Set {
        policy: eefn::workload_leases::Policy {
            enabled: false,
            max_session_ms: 60000,
            max_lease_ms: 10000,
        },
        expected_revision: enabled["result"]["revision"].as_u64().unwrap(),
        expected_epoch: enabled["result"]["runtime_epoch"].as_u64().unwrap(),
    };
    let disabled = command(&server, request.clone()).await.unwrap();
    assert!(confirmed(&disabled, &request));
    assert_eq!(disabled["result"]["runtime_blocked"], true);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("node.json")).unwrap()).unwrap();
    assert_eq!(saved["permissions"]["media"]["microphone"], false);
    assert_eq!(saved["listening"], Value::Null);
    // Exercise the real local-owner config adapter; no generic remote enable.
    let permission = eefn::permission_commands::PermissionRequest {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: runtime.clone(),
        command: eefn::permission_commands::PermissionCommand::Show {},
    };
    let permissions = service.permission_command(permission, false).unwrap();
    assert_eq!(permissions["success"], true);
    service
        .permission_command(
            eefn::permission_commands::PermissionRequest {
                schema_version: 1,
                expected_node_id: "node".into(),
                expected_runtime_id: runtime.clone(),
                command: eefn::permission_commands::PermissionCommand::Set {
                    feature: eefn::permission_commands::Feature::RemoteManagement,
                    enabled: false,
                    expected_digest: permissions["saved_digest"].as_str().unwrap().into(),
                },
            },
            false,
        )
        .unwrap();
    request.command = Command::Set {
        policy: eefn::workload_leases::Policy {
            enabled: true,
            max_session_ms: 60000,
            max_lease_ms: 10000,
        },
        expected_revision: disabled["result"]["revision"].as_u64().unwrap(),
        expected_epoch: disabled["result"]["runtime_epoch"].as_u64().unwrap(),
    };
    let denied = command(&server, request.clone()).await.unwrap();
    assert_eq!(denied["success"], false);
    assert_eq!(denied["outcome_unknown"], true);
    request.command = Command::Show {};
    let final_show = command(&server, request.clone()).await.unwrap();
    assert!(confirmed(&final_show, &request));
    assert_eq!(final_show["result"]["policy"]["enabled"], false);
    assert_eq!(final_show["capture_started"], false);
    assert_eq!(final_show["execution_authorized"], false);
    assert!(!running.is_finished());
    running.abort();
    let _ = running.await;
    server.stop().await;
}
