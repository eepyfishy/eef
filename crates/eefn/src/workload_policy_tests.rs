use super::*;
use futures_util::FutureExt;
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
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        crate::setup::write_json(&path, &json!({"node_id":"node","psk":"private","endpoints":[],
            "future":{"keep":true},"permissions":{"media":{"microphone":false}},"management":{"allow_remote":true}})).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
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
            .workload_policy_command(self.request(Command::Show {}), false)
            .unwrap()
    }
    fn set(&self, policy: Policy) -> Request {
        let shown = self.show();
        self.request(Command::Set {
            policy,
            expected_revision: shown["revision"].as_u64().unwrap(),
            expected_epoch: shown["runtime_epoch"].as_u64().unwrap(),
        })
    }
    fn change(&self, policy: Policy) -> Value {
        self.service
            .workload_policy_command(self.set(policy), false)
            .unwrap()
    }
    fn enabled(&self) -> Value {
        self.change(Policy {
            enabled: true,
            ..Default::default()
        })
    }
    fn config(&self) -> Value {
        self.service.read_config().unwrap()
    }
}

#[test]
fn legacy_policy_is_off_without_generic_save_inventing_configuration_or_revision() {
    let old = json!({"future":true});
    let mut next = old.clone();
    preserve(&old, &mut next, false).unwrap();
    assert_eq!(next, old);
    assert!(!from_config(&old).unwrap().enabled);
    assert_eq!(revision(&old).unwrap(), 0);
    for bad in [
        json!({"workload_policy":null}),
        json!({"workload_policy":{"enabled":"yes"}}),
        json!({"workload_policy":{"enabled":true,"unexpected":1}}),
        json!({"workload_policy":{"max_lease_ms":0}}),
        json!({"workload_policy_revision":-1}),
    ] {
        assert!(from_config(&bad).is_err() || revision(&bad).is_err());
    }
}

#[test]
fn typed_policy_command_persists_bounds_without_permission_consent_or_restart_authority() {
    let f = Fixture::new();
    let before = f.config();
    assert!(!f.show()["policy"]["enabled"].as_bool().unwrap());
    let policy = Policy {
        enabled: true,
        max_session_ms: 60000,
        max_lease_ms: 10000,
    };
    let request = f.set(policy.clone());
    let report = f
        .service
        .workload_policy_command(request.clone(), false)
        .unwrap();
    assert!(confirmed(&report, &request));
    assert_eq!(from_config(&f.config()).unwrap(), policy);
    assert_eq!(report["revision"], 1);
    assert_eq!(report["runtime_blocked"], false);
    assert_eq!(f.config()["future"], before["future"]);
    assert_eq!(f.config()["permissions"], before["permissions"]);
    assert_eq!(f.config()["listening"], Value::Null);
    assert_eq!(report["capture_started"], false);
    assert_eq!(report["execution_authorized"], false);
    assert!(!f.service.restart.notified().now_or_never().is_some());
    assert!(render(&report).unwrap().contains("60000 ms"));
}

#[test]
fn stale_policy_runtime_revision_epoch_and_unknown_commands_are_refused_without_save() {
    let f = Fixture::new();
    let stale = f.set(Policy {
        enabled: true,
        ..Default::default()
    });
    let mut wrong = stale.clone();
    wrong.expected_runtime_id = uuid::Uuid::new_v4().to_string();
    assert!(f.service.workload_policy_command(wrong, false).is_err());
    f.enabled();
    let before = f.config();
    assert!(f.service.workload_policy_command(stale, false).is_err());
    let mut old_epoch = f.set(Policy::default());
    let Command::Set { expected_epoch, .. } = &mut old_epoch.command else {
        unreachable!()
    };
    *expected_epoch += 1;
    assert!(f.service.workload_policy_command(old_epoch, false).is_err());
    assert_eq!(before, f.config());
    assert!(serde_json::from_value::<Command>(json!({"operation":"show","enabled":true})).is_err());
    assert!(
        serde_json::from_value::<Request>(json!({"schema_version":1,"expected_node_id":"node",
        "expected_runtime_id":f.runtime,"command":{"operation":"show"},"unexpected":1}))
        .is_err()
    );
}

#[tokio::test]
async fn remote_policy_changes_require_owner_management_and_reserved_status_is_read_only() {
    let f = Fixture::new();
    let mut config = f.config();
    config["management"]["allow_remote"] = json!(false);
    f.service.save_config(config).unwrap();
    let request = f.set(Policy {
        enabled: true,
        ..Default::default()
    });
    assert!(
        f.service
            .workload_policy_command(request.clone(), true)
            .is_err()
    );
    assert!(
        f.service
            .remote("workload_policy_status", json!(request))
            .await
            .is_err()
    );
    assert!(
        f.service
            .remote("workload_policy_status", json!(f.request(Command::Show {})))
            .await
            .is_ok()
    );
    let mut config = f.config();
    config["management"]["allow_remote"] = json!(true);
    f.service.save_config(config).unwrap();
    let request = f.set(Policy {
        enabled: true,
        ..Default::default()
    });
    let result = f
        .service
        .remote("workload_policy", json!(request))
        .await
        .unwrap();
    assert_eq!(result["policy"]["enabled"], true);
    assert_eq!(f.config()["permissions"]["media"]["microphone"], false);
}

#[tokio::test]
async fn disable_cancels_owned_future_and_off_on_cannot_revive_an_unpolled_permit() {
    struct Owned(Arc<AtomicBool>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let f = Fixture::new();
    f.enabled();
    let gate = &f.service.workload_gate;
    let old = gate.permit().unwrap();
    let permit = gate.permit().unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let owned = Owned(dropped.clone());
    let (started, ready) = tokio::sync::oneshot::channel();
    let mut running = Box::pin(permit.supervise(async move {
        let _owned = owned;
        let _ = started.send(());
        std::future::pending::<Result<()>>().await
    }));
    tokio::select! { biased; result=&mut running=>panic!("Ended before disable: {result:?}"), _=ready=>{} }
    f.change(Policy::default());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), running)
            .await
            .unwrap()
            .is_err()
    );
    assert!(dropped.load(Ordering::SeqCst));
    f.enabled();
    let polled = Arc::new(AtomicBool::new(false));
    let observed = polled.clone();
    assert!(
        old.supervise(async move {
            observed.store(true, Ordering::SeqCst);
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

#[test]
fn generic_saves_preserve_policy_and_revision_but_cannot_enable_or_widen_it() {
    let f = Fixture::new();
    let mut raw = f.config();
    raw["workload_policy"] = json!(Policy {
        enabled: true,
        ..Default::default()
    });
    assert!(f.service.save_config(raw).is_err());
    let enabled = f.enabled();
    let mut raw = f.config();
    raw.as_object_mut().unwrap().remove("workload_policy");
    raw["workload_policy_revision"] = json!(0);
    f.service.save_config(raw).unwrap();
    assert_eq!(f.config()["workload_policy"], enabled["policy"]);
    assert_eq!(f.config()["workload_policy_revision"], enabled["revision"]);
    let mut raw = f.config();
    raw["workload_policy"]["max_session_ms"] = json!(7200000);
    assert!(f.service.save_config(raw).is_err());
    let mut raw = f.config();
    raw["workload_policy"]["enabled"] = json!(false);
    f.service.save_config(raw).unwrap();
    assert!(f.service.workload_gate.permit().is_err());
}

#[tokio::test]
async fn failed_save_revokes_before_storage_and_generic_restore_cannot_clear_the_latch() {
    let f = Fixture::new();
    f.enabled();
    let old = f.service.workload_gate.permit().unwrap();
    let before = f.config();
    let backup = f.dir.path().join("node.json.bak");
    std::fs::remove_file(&backup).unwrap();
    std::fs::create_dir(&backup).unwrap();
    let mut lowered = from_config(&before).unwrap();
    lowered.max_session_ms /= 2;
    assert!(
        f.service
            .workload_policy_command(f.set(lowered), false)
            .is_err()
    );
    assert_eq!(f.config(), before);
    assert_eq!(f.show()["runtime_blocked"], true);
    assert!(old.supervise(async { Ok(()) }).await.is_err());
    std::fs::remove_dir(&backup).unwrap();
    f.service.save_config(before).unwrap();
    assert!(f.service.workload_gate.permit().is_err());
    f.enabled();
    assert!(f.service.workload_gate.permit().is_ok());
}

#[tokio::test]
async fn changing_bounds_invalidates_original_policy_while_a_fresh_permit_uses_saved_limits() {
    let f = Fixture::new();
    f.enabled();
    let old = f.service.workload_gate.permit().unwrap();
    let changed = f.change(Policy {
        enabled: true,
        max_session_ms: 60000,
        max_lease_ms: 10000,
    });
    assert_eq!(changed["revision"], 2);
    assert!(old.supervise(async { Ok(()) }).await.is_err());
    assert!(
        f.service
            .workload_gate
            .permit()
            .unwrap()
            .supervise(async { Ok(7) })
            .await
            .is_ok()
    );
    assert_eq!(from_config(&f.config()).unwrap().max_session_ms, 60000);
    assert_eq!(f.config()["permissions"]["media"]["microphone"], false);
}

#[test]
fn invalid_bounds_and_revision_exhaustion_leave_existing_policy_intact() {
    let f = Fixture::new();
    let before = f.config();
    for policy in [
        Policy {
            enabled: true,
            max_session_ms: 0,
            max_lease_ms: 1,
        },
        Policy {
            enabled: true,
            max_session_ms: 100,
            max_lease_ms: 101,
        },
        Policy {
            enabled: true,
            max_session_ms: crate::workload_leases::MAX_SESSION_MS + 1,
            max_lease_ms: 1,
        },
    ] {
        assert!(
            f.service
                .workload_policy_command(f.set(policy), false)
                .is_err()
        );
    }
    assert_eq!(f.config(), before);
    let old = json!({"workload_policy_revision":i64::MAX});
    let mut next = json!({"workload_policy":{"enabled":true}});
    assert!(preserve(&old, &mut next, true).is_err());
}
