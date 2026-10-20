use super::*;
use crate::permission_commands::{Feature, PermissionCommand, PermissionRequest};

struct Fixture {
    dir: tempfile::TempDir,
    core: Arc<NodeService>,
    engine: Arc<crate::NodeEngine>,
    runtime: String,
    _guard: crate::request_preview::PreviewGuard,
    model: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let model = dir.path().join("fixture.gguf");
        std::fs::write(
            &model,
            b"synthetic metadata fixture, not real model weights",
        )
        .unwrap();
        let mut policy = crate::NodePolicy::default();
        policy.http.enabled = true;
        policy.http.allowed_hosts = vec!["example.invalid".into()];
        let config = json!({"node_id":"owner","name":"Fixture node","psk":"private-fixture","endpoints":[],
        "permissions":policy,"future":{"keep":true},"models":{"provider":"llamacpp","llamacpp":{"slots":[
            {"model_id":"fixture","port":8082,"model_path":model,"gpu_layers":0,"context":4096}
        ]}}});
        crate::setup::write_json(&path, &config).unwrap();
        let core = NodeService::new(path, "owner".into());
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap()
                .with_service(core.clone()),
        );
        let runtime = uuid::Uuid::new_v4().to_string();
        core.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let guard = core.attach_preview_runtime(&engine, runtime.clone());
        Self {
            dir,
            core,
            engine,
            runtime,
            _guard: guard,
            model,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.core.config_path).unwrap()
    }
    fn permissions(&self, command: PermissionCommand) -> PermissionRequest {
        PermissionRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: self.runtime.clone(),
            command,
        }
    }
}

#[test]
fn retained_model_reference_missing_weights_cannot_block_permission_revocation() {
    let f = Fixture::new();
    let saved = f.core.read_config().unwrap()["models"].clone();
    let mut grant = crate::grants::tests::grant();
    grant.capability = "http.request".into();
    grant.action = "send".into();
    grant.params = json!({"url":"https://example.invalid/","method":"GET"});
    grant.validate().unwrap();
    f.engine.check_grant_policy(&grant).unwrap();
    std::fs::remove_file(&f.model).unwrap();
    let shown = f
        .core
        .permission_command(f.permissions(PermissionCommand::Show {}), false)
        .unwrap();
    let request = f.permissions(PermissionCommand::Set {
        feature: Feature::HttpRequests,
        enabled: false,
        expected_digest: shown["saved_digest"].as_str().unwrap().into(),
    });
    let report = f.core.permission_command(request.clone(), false).unwrap();
    assert!(crate::permission_commands::confirmed(&report, &request));
    let config = f.core.read_config().unwrap();
    assert_eq!(config["models"], saved);
    assert_eq!(config["permissions"]["http"]["enabled"], false);
    assert_eq!(config["future"]["keep"], true);
    assert_eq!(config["psk"], "private-fixture");
    assert!(
        f.engine.check_grant_policy(&grant).is_err(),
        "saved revocation must fence the previously applied scope without executing HTTP"
    );
    assert!(
        !f.model.exists(),
        "core control cannot recreate or load missing weights"
    );
}

#[test]
fn retained_model_reference_new_relative_file_uses_the_same_config_base_as_startup() {
    let f = Fixture::new();
    let name = format!("{}.gguf", uuid::Uuid::new_v4());
    std::fs::write(
        f.dir.path().join(&name),
        b"synthetic file, not a working model",
    )
    .unwrap();
    let mut config = f.core.read_config().unwrap();
    config["models"]["llamacpp"]["slots"][0]["model_path"] = json!(name);
    f.core.save_config(config).unwrap();
}

#[test]
fn retained_model_reference_new_projector_must_exist_before_any_settings_write() {
    let f = Fixture::new();
    let mut config = f.core.read_config().unwrap();
    config["models"]["llamacpp"]["slots"][0]["mmproj_path"] =
        json!(f.dir.path().join("missing-projector.gguf"));
    let before = f.bytes();
    assert!(f.core.save_config(config).is_err());
    assert_eq!(before, f.bytes());
}
