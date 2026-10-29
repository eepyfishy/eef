use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Fixture {
    dir: tempfile::TempDir,
    service: Arc<crate::NodeService>,
    runtime: String,
    _engine: Arc<crate::NodeEngine>,
    _guard: crate::request_preview::PreviewGuard,
}
impl Fixture {
    fn new(microphone: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path,&json!({"node_id":"node","psk":"private","endpoints":[],"future":{"keep":true},"permissions":{"media":{"microphone":microphone}},"management":{"allow_remote":true}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let mut policy = crate::NodePolicy::default();
        policy.media.microphone = microphone;
        let engine = Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_policy(policy)
                .unwrap(),
        );
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        Self {
            dir,
            service,
            runtime,
            _engine: engine,
            _guard: guard,
        }
    }
    fn request(&self, command: Command) -> Request {
        Request {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: self.runtime.clone(),
            command,
        }
    }
    fn show(&self) -> Value {
        self.service
            .listening_command(self.request(Command::Show {}), false)
            .unwrap()
    }
    fn set(&self, enabled: bool) -> Request {
        let report = self.show();
        self.request(Command::Set {
            enabled,
            expected_revision: report["revision"].as_u64().unwrap(),
            expected_epoch: report["runtime_epoch"].as_u64().unwrap(),
        })
    }
    fn change(&self, enabled: bool) -> Value {
        self.service
            .listening_command(self.set(enabled), false)
            .unwrap()
    }
    fn config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.dir.path().join("node.json")).unwrap()).unwrap()
    }
}

#[test]
fn legacy_config_defaults_off_without_inserting_preferences_or_revisions() {
    let old = json!({"future":true});
    let mut next = old.clone();
    preserve_revision(&old, &mut next, false).unwrap();
    assert_eq!(old, next);
    assert!(!Consent::from_config(&old).unwrap().enabled);
    assert_eq!(revision(&old).unwrap(), 0);
    for bad in [
        json!({"listening":null}),
        json!({"listening":{"enabled":"yes"}}),
        json!({"listening":{"enabled":true,"unexpected":1}}),
        json!({"listening_revision":-1}),
    ] {
        assert!(Consent::from_config(&bad).is_err() || revision(&bad).is_err());
    }
}
#[test]
fn status_command_rejects_unknown_fields_instead_of_ignoring_them() {
    assert_eq!(
        serde_json::from_value::<Command>(json!({"operation":"show"})).unwrap(),
        Command::Show {}
    );
    for value in [
        json!({"operation":"show","enabled":true}),
        json!({"operation":"show","unexpected":1}),
        json!({"operation":"set","enabled":true,"expected_revision":0}),
        json!({"operation":"other"}),
    ] {
        assert!(serde_json::from_value::<Command>(value).is_err());
    }
}
#[test]
fn generic_saves_and_restores_cannot_reenable_denied_consent_or_roll_back_revision() {
    let old = json!({"listening":{"enabled":false},"listening_revision":4});
    let mut restoring = json!({"listening":{"enabled":true},"listening_revision":1});
    assert!(preserve_revision(&old, &mut restoring, false).is_err());
    preserve_revision(&old, &mut restoring, true).unwrap();
    assert_eq!(restoring["listening_revision"], 5);
    let mut deny = json!({"listening":{"enabled":false},"listening_revision":0});
    preserve_revision(&restoring, &mut deny, false).unwrap();
    assert_eq!(deny["listening_revision"], 6);
    assert!(preserve_revision(&deny, &mut restoring, false).is_err());
    let mut omitted = json!({"future":1});
    preserve_revision(&deny, &mut omitted, false).unwrap();
    assert_eq!(omitted["listening_revision"], 6);
    assert_eq!(omitted["listening"], json!({"enabled":false}));
}
#[test]
fn local_consent_preserves_permissions_and_never_claims_capture_or_restart() {
    let f = Fixture::new(false);
    let before = f.config();
    let request = f.set(true);
    let report = f.service.listening_command(request.clone(), false).unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(report["revision"], 1);
    assert_eq!(report["consent"]["enabled"], true);
    assert_eq!(report["capture_integration_available"], false);
    assert_eq!(report["capture_started"], false);
    assert_eq!(report["applied_microphone_permission"], false);
    assert_eq!(report["execution_authorized"], false);
    assert_eq!(f.config()["permissions"], before["permissions"]);
    assert_eq!(f.config()["future"], before["future"]);
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
    assert!(f.service.listening_command(request, false).is_err());
}
#[test]
fn remote_even_approved_management_and_stale_runtime_cannot_mutate_consent() {
    let f = Fixture::new(true);
    let request = f.set(true);
    let before = f.config();
    assert!(f.service.listening_command(request.clone(), true).is_err());
    assert_eq!(f.config(), before);
    let mut wrong = request.clone();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.listening_command(wrong, false).is_err());
    let mut wrong = request;
    wrong.expected_node_id = "different".into();
    assert!(f.service.listening_command(wrong, false).is_err());
    assert_eq!(f.config(), before);
    let request = f.request(Command::Show {});
    assert!(confirmed(
        &f.service.listening_command(request.clone(), true).unwrap(),
        &request
    ));
}
#[test]
fn failed_durable_disable_keeps_runtime_revoked_and_rejects_stale_enable() {
    let f = Fixture::new(true);
    f.change(true);
    let stale = f.set(true);
    let before = f.config();
    // Enabling already made an ordinary backup. Replace only that fixture-owned
    // file with a directory to inject the next backup-write failure.
    std::fs::remove_file(f.dir.path().join("node.json.bak")).unwrap();
    std::fs::create_dir(f.dir.path().join("node.json.bak")).unwrap();
    assert!(f.service.listening_command(f.set(false), false).is_err());
    assert_eq!(f.config(), before);
    assert_eq!(f.show()["runtime_blocked"], true);
    assert!(f.service.listening_command(stale, false).is_err());
    // A fresh, explicit local decision may clear the in-memory denial while the
    // retained durable consent is unchanged. No failed mutation is replayed.
    let result = f.change(true);
    assert_eq!(result["changed"], false);
    assert_eq!(result["runtime_blocked"], false);
}
#[tokio::test]
async fn off_then_on_cannot_revive_an_old_supervised_epoch() {
    let gate = Gate::default();
    let permit = gate.permit().unwrap();
    gate.deny();
    gate.allow().unwrap();
    let polled = Arc::new(AtomicBool::new(false));
    let mark = polled.clone();
    assert!(
        permit
            .supervise(async move {
                mark.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
    );
    assert!(!polled.load(Ordering::SeqCst));
    gate.permit()
        .unwrap()
        .supervise(async { Ok(()) })
        .await
        .unwrap();
}
struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn disable_drops_owned_future_before_returning_to_final_storage() {
    let f = Fixture::new(true);
    f.change(true);
    let dropped = Arc::new(AtomicBool::new(false));
    let mark = dropped.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let operation = async move {
        let _owned = Dropped(mark);
        started.send(()).unwrap();
        std::future::pending::<Result<()>>().await
    };
    let running = f.service.supervise_listening(
        &f.runtime,
        tokio::time::Instant::now() + std::time::Duration::from_secs(3),
        operation,
    );
    tokio::pin!(running);
    tokio::select! {_=ready=>{},_=&mut running=>panic!("Owned operation must be pending")};
    let off = f.change(false);
    assert_eq!(off["runtime_blocked"], true);
    assert!(running.await.is_err());
    assert!(
        dropped.load(Ordering::SeqCst),
        "Drop precedes the caller's final persistence"
    );
    assert_eq!(f.service.diagnostics()["success"], true);
    assert_eq!(f.service.live.lock().unwrap()["pending_restart"], false);
}
#[tokio::test]
async fn missing_microphone_permission_or_expired_deadline_prevents_polling() {
    let f = Fixture::new(false);
    f.change(true);
    let polled = Arc::new(AtomicBool::new(false));
    let mark = polled.clone();
    assert!(
        f.service
            .supervise_listening(
                &f.runtime,
                tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                async move {
                    mark.store(true, Ordering::SeqCst);
                    Ok(())
                }
            )
            .await
            .is_err()
    );
    assert!(!polled.load(Ordering::SeqCst));
    let f = Fixture::new(true);
    f.change(true);
    let mark = polled.clone();
    assert!(
        f.service
            .supervise_listening(&f.runtime, tokio::time::Instant::now(), async move {
                mark.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .is_err()
    );
    assert!(!polled.load(Ordering::SeqCst));
}
#[test]
fn forged_success_fields_and_revision_exhaustion_are_refused() {
    let f = Fixture::new(true);
    let request = f.set(true);
    let report = f.service.listening_command(request.clone(), false).unwrap();
    for (key, value) in [
        ("capture_started", json!(true)),
        ("capture_integration_available", json!(true)),
        ("execution_authorized", json!(true)),
        ("runtime_epoch", json!(100)),
        ("note", json!("arbitrary")),
        ("unknown", json!(false)),
    ] {
        let mut forged = report.clone();
        forged[key] = value;
        assert!(!confirmed(&forged, &request), "{key}");
    }
    let mut missing = f.show();
    missing["consent"] = json!({});
    assert!(
        !confirmed(&missing, &f.request(Command::Show {})),
        "Missing consent is unknown, not disabled"
    );
    let old = json!({"listening":{"enabled":false},"listening_revision":i64::MAX});
    let mut next = json!({"listening":{"enabled":true}});
    assert!(preserve_revision(&old, &mut next, true).is_err());
}
#[test]
fn backend_generic_save_cannot_bypass_consent_gate() {
    let f = Fixture::new(true);
    let mut next = f.config();
    next["listening"] = json!({"enabled":true});
    assert!(f.service.save_configuration(next).is_err());
    f.change(true);
    let mut next = f.config();
    next["listening"] = json!({"enabled":false});
    f.service.save_configuration(next).unwrap();
    assert_eq!(f.show()["consent"]["enabled"], false);
}
