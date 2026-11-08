#[tokio::test]
async fn camera_dependency_control_requires_pinned_runtime_and_current_remote_owner_approval() {
    let f = camera_authoring_fixture().await;
    let runtime = f.runtime.node_server.node_state("origin").await.unwrap()
        .registration["registration_receipt"]["runtime_id"].as_str().unwrap().to_string();
    let request = eefn::camera_check::Request {
        schema_version: 1, expected_node_id: "origin".into(), expected_runtime_id: runtime,
        request_id: uuid::Uuid::new_v4().to_string(), settings: Default::default(),
    };
    let before = std::fs::read(&f.node_path).unwrap();
    assert!(crate::node_camera::check_command(&f.runtime.node_server, request.clone()).await.is_err());
    assert_eq!(std::fs::read(&f.node_path).unwrap(), before);
    let mut allowed: Value = serde_json::from_slice(&before).unwrap();
    allowed["management"] = json!({"allow_remote":true});
    f.service.save_configuration(allowed).unwrap();
    let after = std::fs::read(&f.node_path).unwrap();
    let checked = crate::node_camera::check_command(&f.runtime.node_server, request.clone()).await.unwrap();
    assert!(eefn::camera_check::confirmed(&checked, &request));
    assert_eq!(checked["success"], false);
    assert_eq!(checked["status"], "runtime_unavailable");
    assert!(!checked.to_string().contains("PRIVATE_"));
    assert_eq!(std::fs::read(&f.node_path).unwrap(), after);
    let unpinned = f.runtime.node_server.invoke_remote("origin", "node.configure", "camera_check", json!(request),
        std::time::Duration::from_secs(5)).await.unwrap();
    assert_eq!(unpinned["success"], false);
    assert_eq!(unpinned["error_type"], "PinnedSessionRequired");
    let stale = eefn::camera_check::Request { expected_runtime_id: uuid::Uuid::new_v4().to_string(), ..request };
    assert!(f.runtime.node_server.invoke_camera_check(stale).await.is_err());
    assert_eq!(std::fs::read(&f.node_path).unwrap(), after);
    f.close().await;
}

#[tokio::test]
async fn pinned_camera_configuration_obeys_current_owner_approval_and_keeps_private_bindings_on_node()
 {
    use eefn::camera_setup::{Command, Settings};
    let f = camera_authoring_fixture().await;
    let show = crate::node_camera::Request {
        schema_version: 1,
        node_id: "origin".into(),
        expected_runtime_id: None,
        command: Command::Show,
    };
    let inspected = crate::node_camera::command(&f.runtime.node_server, show.clone())
        .await
        .unwrap();
    assert!(crate::node_camera::confirmed(&inspected, &show));
    let mut configure = crate::node_camera::Request {
        command: Command::Configure {
            settings: Settings {
                resource: Some("camera".into()),
                rtsp: Some("rtsps://owner:PRIVATE_REMOTE_CAMERA@camera.invalid:7441/live".into()),
                ..Default::default()
            },
            expected_digest: inspected["result"]["saved_digest"].as_str().unwrap().into(),
        },
        expected_runtime_id: Some(inspected["result"]["runtime_id"].as_str().unwrap().into()),
        ..show.clone()
    };
    let before = std::fs::read(&f.node_path).unwrap();
    let denied = crate::node_camera::command(&f.runtime.node_server, configure.clone())
        .await
        .unwrap();
    assert_eq!(denied["success"], false);
    assert_eq!(std::fs::read(&f.node_path).unwrap(), before);
    let mut allowed: Value = serde_json::from_slice(&before).unwrap();
    allowed["management"] = json!({"allow_remote":true});
    f.service.save_configuration(allowed).unwrap();
    let saved = crate::node_camera::command(&f.runtime.node_server, configure.clone())
        .await
        .unwrap();
    assert!(crate::node_camera::confirmed(&saved, &configure));
    assert_eq!(saved["result"]["changed"], true);
    assert_eq!(saved["result"]["restart_required"], true);
    assert!(
        !saved.to_string().contains("PRIVATE_") && !saved.to_string().contains("camera.invalid")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&f.node_path).unwrap()).unwrap()["metadata"]
            ["resources"][0]["parameters"]["device"],
        "rtsps://owner:PRIVATE_REMOTE_CAMERA@camera.invalid:7441/live"
    );
    let after = std::fs::read(&f.node_path).unwrap();
    let stale = crate::node_camera::command(&f.runtime.node_server, configure.clone())
        .await
        .unwrap();
    assert_eq!(stale["success"], false);
    assert_eq!(std::fs::read(&f.node_path).unwrap(), after);
    let current = crate::node_camera::command(&f.runtime.node_server, show.clone())
        .await
        .unwrap();
    if let Command::Configure {
        expected_digest, ..
    } = &mut configure.command
    {
        *expected_digest = current["result"]["saved_digest"].as_str().unwrap().into();
    }
    let noop = crate::node_camera::command(&f.runtime.node_server, configure.clone())
        .await
        .unwrap();
    assert!(crate::node_camera::confirmed(&noop, &configure));
    assert_eq!(noop["result"]["changed"], false);
    let mut revoked: Value = serde_json::from_slice(&after).unwrap();
    revoked["management"]["allow_remote"] = json!(false);
    f.service.save_configuration(revoked).unwrap();
    let before = std::fs::read(&f.node_path).unwrap();
    assert_eq!(
        crate::node_camera::command(&f.runtime.node_server, configure)
            .await
            .unwrap()["success"],
        false
    );
    assert_eq!(std::fs::read(&f.node_path).unwrap(), before);
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
async fn camera_configuration_refuses_old_runtime_and_reserved_status_mutation_without_saving() {
    let f = camera_authoring_fixture().await;
    let before = std::fs::read(&f.node_path).unwrap();
    let node = eefn::camera_setup::Request {
        schema_version: 1,
        expected_node_id: "origin".into(),
        expected_runtime_id: uuid::Uuid::new_v4().to_string(),
        command: eefn::camera_setup::Command::Show,
    };
    assert!(
        f.runtime
            .node_server
            .invoke_camera_setup(node.clone())
            .await
            .is_err()
    );
    let current = f
        .runtime
        .node_server
        .node_state("origin")
        .await
        .unwrap()
        .registration["registration_receipt"]["runtime_id"]
        .as_str()
        .unwrap()
        .to_string();
    let request = eefn::camera_setup::Request {
        expected_runtime_id: current,
        command: eefn::camera_setup::Command::Configure {
            settings: Default::default(),
            expected_digest: "a".repeat(64),
        },
        ..node
    };
    let refused = f
        .runtime
        .node_server
        .invoke_remote(
            "origin",
            "node.configure",
            "camera_setup_status",
            json!(request),
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(refused["success"], false);
    assert_eq!(std::fs::read(&f.node_path).unwrap(), before);
    let mut outer = crate::node_camera::Request {
        schema_version: 1,
        node_id: "origin".into(),
        expected_runtime_id: None,
        command: request.command,
    };
    assert!(outer.validate().is_err());
    outer.expected_runtime_id = Some(request.expected_runtime_id);
    outer.validate().unwrap();
    assert!(!crate::node_camera::confirmed(
        &json!({"success":true,"execution_authorized":true}),
        &outer
    ));
    f.close().await;
}
