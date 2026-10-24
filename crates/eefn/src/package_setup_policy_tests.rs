use super::*;
use std::sync::Arc;

fn fixture(
    version: &str,
) -> (
    tempfile::TempDir,
    Context,
    Arc<crate::NodeService>,
    Arc<crate::NodeEngine>,
    crate::request_preview::PreviewGuard,
    Request,
) {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("eefn.exe");
    std::fs::write(&executable, b"test-only executable").unwrap();
    std::fs::write(
        root.path().join("bundle.json"),
        serde_json::to_vec(&json!({"name":"eefn","version":version})).unwrap(),
    )
    .unwrap();
    std::fs::write(root.path().join("eefn.repair-source.json"), serde_json::to_vec(&json!({"schema_version":1,"program":"eefn","version":version,"manifest_url":"https://publisher.invalid/exact.json"})).unwrap()).unwrap();
    let ctx = Context::for_executable(executable, "eefn", version).unwrap();
    let mut config = crate::setup::defaults();
    config["node_id"] = json!("node");
    config["name"] = json!("Fixture node");
    config["psk"] = json!("PRIVATE");
    config["custom"] = json!({"keep":true});
    let path = root.path().join("node.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let service = crate::NodeService::new(path, "node".into());
    let engine = Arc::new(
        crate::NodeEngine::new(false, vec![], root.path().into())
            .unwrap()
            .with_service(service.clone()),
    );
    let runtime = uuid::Uuid::new_v4().to_string();
    service.live.lock().unwrap()["runtime_id"] = json!(runtime);
    let guard = service.attach_preview_runtime(&engine, runtime.clone());
    let request = Request {
        schema_version: 1,
        expected_node_id: "node".into(),
        expected_runtime_id: runtime,
        request_id: uuid::Uuid::new_v4().to_string(),
        command: Command::Show,
    };
    (root, ctx, service, engine, guard, request)
}
fn allow(service: &crate::NodeService, management: bool, consent: bool) {
    use crate::permission_commands::{Feature, PermissionCommand, PermissionRequest};
    let mut request = PermissionRequest {
        schema_version: 1,
        expected_node_id: service.node_id.clone(),
        expected_runtime_id: service.live.lock().unwrap()["runtime_id"]
            .as_str()
            .unwrap()
            .into(),
        command: PermissionCommand::Show {},
    };
    let mut report = service.permission_command(request.clone(), false).unwrap();
    for (feature, enabled) in [
        (Feature::RemoteManagement, management),
        (Feature::RemoteRepairSetupPolicy, consent),
    ] {
        request.command = PermissionCommand::Set {
            feature,
            enabled,
            expected_digest: report["saved_digest"].as_str().unwrap().into(),
        };
        report = service.permission_command(request.clone(), false).unwrap();
    }
}
fn configure(request: &Request, inspected: &Value, mode: Mode) -> Request {
    Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        command: Command::Configure {
            previous: serde_json::from_value(inspected["version"].clone()).unwrap(),
            mode,
            allow_automatic_repair: mode == Mode::Auto,
            manifest: None,
        },
        ..request.clone()
    }
}

#[test]
fn read_only_default_no_files_or_network_and_strict_correlated_report() {
    let (root, ctx, service, _engine, _guard, request) = fixture("1.0.0-alpha.1");
    let before = std::fs::read_dir(root.path()).unwrap().count();
    let show = execute(&service, &ctx, &request, true).unwrap();
    assert!(confirmed(&show, &request, true));
    assert_eq!(show["policy"]["mode"], "off");
    assert_eq!(show["version"]["revision"], 0);
    assert_eq!(show["remote_policy_allowed"], false);
    assert_eq!(show["prerelease_automation_blocked"], true);
    assert_eq!(show["applicability"], "off");
    assert!(!ctx.policy().exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), before);
    assert!(!show.to_string().contains("PRIVATE"));
    for (field, value) in [
        ("request_id", json!(uuid::Uuid::new_v4().to_string())),
        ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
        ("remote", json!(false)),
        ("saved", json!(true)),
        ("repair_approved", json!(true)),
        ("repair_started_by_command", json!(true)),
        ("restart_requested", json!(true)),
        ("prerelease_automation_blocked", json!(false)),
        ("note", json!("PRIVATE-PROSE")),
        ("extra", json!(true)),
    ] {
        let mut bad = show.clone();
        bad[field] = value;
        assert!(!confirmed(&bad, &request, true), "{field}");
    }
    let mut bad = show.clone();
    bad["version"]["digest"] = json!("f".repeat(64));
    assert!(!confirmed(&bad, &request, true));
}

#[test]
fn remote_save_requires_both_local_consents_and_each_save_consumes_exact_cas() {
    let (_root, ctx, service, _engine, _guard, request) = fixture("1.0.0");
    let initial = service.read_config().unwrap();
    let show = execute(&service, &ctx, &request, true).unwrap();
    let prompt = configure(&request, &show, Mode::Prompt);
    for (management, consent) in [(false, false), (true, false), (false, true)] {
        allow(&service, management, consent);
        assert!(execute(&service, &ctx, &prompt, true).is_err());
        assert!(!ctx.policy().exists());
    }
    allow(&service, true, true);
    let saved = execute(&service, &ctx, &prompt, true).unwrap();
    assert!(confirmed(&saved, &prompt, true));
    assert_eq!(saved["policy"]["mode"], "prompt");
    assert_eq!(
        saved["policy"]["binding"]["public_source"],
        "https://publisher.invalid/exact.json"
    );
    assert!(!ctx.state().exists());
    assert!(!ctx.leases().exists());
    assert!(execute(&service, &ctx, &prompt, true).is_err());
    let next = configure(&request, &saved, Mode::Prompt);
    let again = execute(&service, &ctx, &next, true).unwrap();
    assert_ne!(again["policy"]["policy_id"], saved["policy"]["policy_id"]);
    assert_eq!(again["version"]["revision"], 2);
    let automatic = configure(&request, &again, Mode::Auto);
    let again = execute(&service, &ctx, &automatic, true).unwrap();
    assert!(confirmed(&again, &automatic, true));
    assert_eq!(again["policy"]["mode"], "auto");
    assert_eq!(again["version"]["revision"], 3);
    assert!(!ctx.state().exists());
    let mut digest_mismatch = configure(&request, &again, Mode::Off);
    if let Command::Configure { previous, .. } = &mut digest_mismatch.command {
        previous.digest = "a".repeat(64)
    }
    assert!(execute(&service, &ctx, &digest_mismatch, true).is_err());
    let off = configure(&request, &again, Mode::Off);
    allow(&service, true, false);
    assert!(execute(&service, &ctx, &off, true).is_err());
    allow(&service, true, true);
    let off = execute(&service, &ctx, &off, true).unwrap();
    assert_eq!(off["policy"]["mode"], "off");
    let final_config = service.read_config().unwrap();
    for field in ["permissions", "models", "psk", "custom", "node_id"] {
        assert_eq!(final_config[field], initial[field]);
    }
    assert_eq!(service.live.lock().unwrap()["pending_restart"], false);
}

#[test]
fn alpha_never_authorizes_automatic_repair_prompt_stays_blocked_and_off_ignores_broken_history() {
    let (_root, ctx, service, _engine, _guard, request) = fixture("1.0.0-alpha.1");
    allow(&service, true, true);
    let show = execute(&service, &ctx, &request, true).unwrap();
    assert!(
        execute(
            &service,
            &ctx,
            &configure(&request, &show, Mode::Auto),
            true
        )
        .is_err()
    );
    assert!(!ctx.policy().exists());
    let prompt = execute(
        &service,
        &ctx,
        &configure(&request, &show, Mode::Prompt),
        true,
    )
    .unwrap();
    assert_eq!(prompt["prerelease_automation_blocked"], true);
    std::fs::write(ctx.state(), b"broken PRIVATE history").unwrap();
    let inspected = execute(&service, &ctx, &request, true).unwrap();
    let off = execute(
        &service,
        &ctx,
        &configure(&request, &inspected, Mode::Off),
        true,
    )
    .unwrap();
    assert_eq!(off["policy"]["mode"], "off");
    assert_eq!(
        std::fs::read(ctx.state()).unwrap(),
        b"broken PRIVATE history"
    );
    std::fs::write(ctx.policy(), b"broken PRIVATE policy").unwrap();
    assert!(execute(&service, &ctx, &request, true).is_err());
    assert!(execute(&service, &ctx, &configure(&request, &off, Mode::Off), true).is_err());
    assert_eq!(
        std::fs::read(ctx.policy()).unwrap(),
        b"broken PRIVATE policy"
    );
}

#[test]
fn exact_node_runtime_retirement_and_public_source_validation() {
    let (_root, ctx, service, _engine, guard, request) = fixture("1.0.0");
    allow(&service, true, true);
    let show = execute(&service, &ctx, &request, true).unwrap();
    let command = configure(&request, &show, Mode::Prompt);
    for source in [
        "http://example.invalid/a",
        "https://user:password@example.invalid/a",
        "https://example.invalid/a?q=secret",
        "https://example.invalid/a#secret",
    ] {
        let mut bad = command.clone();
        if let Command::Configure { manifest, .. } = &mut bad.command {
            *manifest = Some(source.into())
        }
        assert!(bad.validate().is_err(), "{source}");
    }
    let mut other = command.clone();
    other.expected_node_id = "other".into();
    assert!(execute(&service, &ctx, &other, true).is_err());
    other = command.clone();
    other.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(execute(&service, &ctx, &other, true).is_err());
    drop(guard);
    assert!(execute(&service, &ctx, &command, true).is_err());
    assert!(!ctx.policy().exists());
}

#[tokio::test]
async fn reserved_status_cannot_save_and_busy_worker_is_not_queued() {
    let (_root, ctx, service, _engine, _guard, request) = fixture("1.0.0");
    allow(&service, true, true);
    let show = execute(&service, &ctx, &request, true).unwrap();
    let mutation = configure(&request, &show, Mode::Off);
    assert!(
        service
            .remote("repair_setup_policy_status", json!(mutation))
            .await
            .is_err()
    );
    let held = service.package_policy_operation.clone().lock_owned().await;
    assert!(service.repair_setup_policy(request, true).await.is_err());
    assert!(!ctx.policy().exists());
    drop(held);
}

#[test]
fn policy_publication_lock_refuses_conflict_without_false_revocation() {
    let (_root, ctx, service, _engine, _guard, request) = fixture("1.0.0");
    allow(&service, true, true);
    let show = execute(&service, &ctx, &request, true).unwrap();
    let lock = super::super::lease_file(&ctx.policy_lock()).unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    assert!(execute(&service, &ctx, &configure(&request, &show, Mode::Off), true).is_err());
    assert!(!ctx.policy().exists());
}

#[test]
fn inspection_distinguishes_replaced_build_without_rebinding_or_health_claim() {
    let (_root, ctx, service, _engine, _guard, request) = fixture("1.0.0");
    allow(&service, true, true);
    let show = execute(&service, &ctx, &request, true).unwrap();
    let prompt = execute(
        &service,
        &ctx,
        &configure(&request, &show, Mode::Prompt),
        true,
    )
    .unwrap();
    assert_eq!(prompt["applicability"], "invoked_build_matches");
    let before = std::fs::read(ctx.policy()).unwrap();
    std::fs::write(&ctx.executable, b"different invoked build").unwrap();
    let observed = execute(&service, &ctx, &request, true).unwrap();
    assert!(confirmed(&observed, &request, true));
    assert_eq!(observed["applicability"], "reauthorization_required");
    assert_eq!(observed["runtime_health_checked"], false);
    assert_eq!(std::fs::read(ctx.policy()).unwrap(), before);
    let mut forged = observed.clone();
    forged["applicability"] = json!("off");
    assert!(!confirmed(&forged, &request, true));
}

#[tokio::test]
async fn cancelled_waiter_does_not_release_owned_blocking_worker_slot() {
    let (_root, _ctx, service, _engine, _guard, _request) = fixture("1.0.0");
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let owner = service.clone();
    let task = tokio::spawn(async move {
        owner
            .operate_package_policy(move || {
                started.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(())
            })
            .await
    });
    ready.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(service.operate_package_policy(|| Ok(())).await.is_err());
    release.send(()).unwrap();
    // Await the actual owned worker's release, not a wall-clock sleep.
    let released = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.package_policy_operation.clone().lock_owned(),
    )
    .await
    .unwrap();
    drop(released);
    service.operate_package_policy(|| Ok(())).await.unwrap();
}
