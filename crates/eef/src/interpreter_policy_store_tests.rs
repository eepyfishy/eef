use super::*;
use eefn::interpreter_policy::{Limits, Scope};

fn approve(generation: u64) -> Command {
    Command::Approve {
        operation_id: uuid::Uuid::new_v4().to_string(),
        generation,
        scope: Scope {
            origin_nodes: vec!["input-node".into()],
            provider_nodes: vec!["model-node".into()],
        },
        limits: Limits::default(),
        valid_for_ms: None,
    }
}
fn call(store: &JobStore, command: Command, runtime: &str) -> Value {
    store
        .interpreter_policy_command(command, runtime, Some(&"a".repeat(64)), 100)
        .unwrap()
}
#[test]
fn receipt_reuse_is_read_only_after_reopen_supersession_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    Connection::open(&path).unwrap();
    let runtime = uuid::Uuid::new_v4().to_string();
    let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
    assert!(call(&store, Command::Show { id: None }, &runtime)["authorization"].is_null());
    assert!(
        store
            .interpreter_policy_command(approve(0), &runtime, None, 100)
            .is_err()
    );
    let command = approve(0);
    let saved = call(&store, command.clone(), &runtime);
    let record: Authorization = serde_json::from_value(saved["authorization"].clone()).unwrap();
    assert!(record.active(1, &record.issuer_id, 100));
    assert!(store.list().unwrap().is_empty());
    drop(store);
    let store = JobStore::open(&path, 20, 1024 * 1024).unwrap();
    let next_runtime = uuid::Uuid::new_v4().to_string();
    let before = store.usage().unwrap();
    let reused = store
        .interpreter_policy_command(command.clone(), &next_runtime, None, 1)
        .unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["authorization"], saved["authorization"]);
    assert_eq!(store.usage().unwrap(), before);
    call(&store, approve(1), &next_runtime);
    let mut advisory = command.clone();
    if let Command::Approve { generation, .. } = &mut advisory {
        *generation = 2;
    }
    let reused = call(&store, advisory, &next_runtime);
    assert_eq!(reused["authorization"], saved["authorization"]);
    let revoke = Command::Revoke {
        id: record.id.clone(),
        revision: 1,
        operation_id: uuid::Uuid::new_v4().to_string(),
    };
    call(&store, revoke.clone(), &next_runtime);
    let reused = call(&store, command, &next_runtime);
    assert_eq!(reused["authorization"]["revision"], 2);
    assert_eq!(reused["reused"], true);
    let before = store.usage().unwrap();
    call(&store, revoke, &next_runtime);
    assert_eq!(store.usage().unwrap(), before);
}
#[test]
fn concurrent_owner_approvals_cannot_both_use_the_same_generation() {
    let store = JobStore::in_memory().unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = store.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let command = approve(0);
            let runtime = uuid::Uuid::new_v4().to_string();
            barrier.wait();
            store
                .interpreter_policy_command(command, &runtime, Some(&"a".repeat(64)), 100)
                .is_ok()
        }));
    }
    barrier.wait();
    let accepted = workers
        .into_iter()
        .map(|worker| usize::from(worker.join().unwrap()))
        .sum::<usize>();
    assert_eq!(accepted, 1);
    assert_eq!(store.usage().unwrap()["interpreter_policy_count"], 1);
}
#[test]
fn exact_shared_quota_admission_and_lowered_quota_revocation_preserve_history() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = uuid::Uuid::new_v4().to_string();
    let command = approve(0);
    let large = JobStore::in_memory().unwrap();
    let saved = call(&large, command.clone(), &runtime);
    let cost = serde_json::to_vec(
        &serde_json::from_value::<Authorization>(saved["authorization"].clone()).unwrap(),
    )
    .unwrap()
    .len()
        + REVOCATION_RESERVE;
    for fits in [false, true] {
        let path = dir.path().join(if fits { "fits.db" } else { "full.db" });
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 1, cost - usize::from(!fits)).unwrap();
        let result =
            store.interpreter_policy_command(command.clone(), &runtime, Some(&"a".repeat(64)), 100);
        assert_eq!(result.is_ok(), fits);
        let usage = store.usage().unwrap();
        assert_eq!(usage["interpreter_policy_count"], usize::from(fits));
        if fits {
            assert_eq!(usage["available_bytes"], 0);
            assert_eq!(usage["over_limit"], false);
        }
    }
    let path = dir.path().join("fits.db");
    let store = JobStore::open(&path, 1, 1024).unwrap();
    assert_eq!(store.usage().unwrap()["over_limit"], true);
    let id = saved["authorization"]["id"].as_str().unwrap().to_owned();
    let revoke = Command::Revoke {
        id,
        revision: 1,
        operation_id: uuid::Uuid::new_v4().to_string(),
    };
    let revoked = call(&store, revoke.clone(), &runtime);
    assert_eq!(revoked["authorization"]["revision"], 2);
    let before = store.usage().unwrap();
    call(&store, revoke, &runtime);
    assert_eq!(store.usage().unwrap(), before);
    assert!(
        store
            .interpreter_policy_command(approve(1), &runtime, Some(&"a".repeat(64)), 100)
            .is_err()
    );
    assert_eq!(
        call(&store, command, &runtime)["authorization"]["revision"],
        2
    );
}
#[test]
fn policy_receipts_cannot_change_scope_or_cross_creation_and_revocation_purposes() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid::Uuid::new_v4().to_string();
    let command = approve(0);
    let saved = call(&store, command.clone(), &runtime);
    let mut changed = command.clone();
    if let Command::Approve { scope, .. } = &mut changed {
        scope.provider_nodes = vec!["other-node".into()];
    }
    assert!(
        store
            .interpreter_policy_command(changed, &runtime, Some(&"a".repeat(64)), 100)
            .is_err()
    );
    let first = saved["authorization"]["id"].as_str().unwrap().to_owned();
    let operation = uuid::Uuid::new_v4().to_string();
    call(
        &store,
        Command::Revoke {
            id: first,
            revision: 1,
            operation_id: operation.clone(),
        },
        &runtime,
    );
    let mut changed = approve(1);
    if let Command::Approve { operation_id, .. } = &mut changed {
        *operation_id = operation;
    }
    assert!(
        store
            .interpreter_policy_command(changed, &runtime, Some(&"a".repeat(64)), 100)
            .is_err()
    );
    let list = call(
        &store,
        Command::List {
            limit: 1,
            after_generation: None,
        },
        &runtime,
    );
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert!(list["next_after"].is_null());
    assert_eq!(store.usage().unwrap()["interpreter_policy_count"], 1);
}
#[test]
fn interpreter_policy_reservations_are_counted_by_existing_automation_approval_admission() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = uuid::Uuid::new_v4().to_string();
    let command = approve(0);
    let sample = JobStore::in_memory().unwrap();
    let policy = call(&sample, command.clone(), &runtime);
    let policy_cost =
        serde_json::to_vec(&policy["authorization"]).unwrap().len() + REVOCATION_RESERVE;
    for policy_first in [true, false] {
        let path = dir.path().join(if policy_first {
            "policy-first.db"
        } else {
            "automation-first.db"
        });
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 100, 1024 * 1024).unwrap();
        let (engine, _, _, approval) =
            super::super::automation_authorizations::tests::fixture(store.clone());
        let approval_cost = serde_json::to_vec(&approval).unwrap().len() + 1024;
        let save_automation = |store: &JobStore| {
            store.automation_policy_checkpoint(
                approval.creation.clone(),
                Some(approval.clone()),
                &approval.runtime_id,
                approval.approved_at_ms,
            )
        };
        if policy_first {
            call(&store, command.clone(), &runtime);
        } else {
            save_automation(&store).unwrap();
        }
        let used =
            1024 * 1024 - store.usage().unwrap()["available_bytes"].as_u64().unwrap() as usize;
        let exact_limit = used
            + if policy_first {
                approval_cost
            } else {
                policy_cost
            };
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 100, exact_limit - 1).unwrap();
        let before = store.usage().unwrap();
        if policy_first {
            assert!(
                save_automation(&store)
                    .unwrap_err()
                    .to_string()
                    .contains("storage is full")
            );
        } else {
            assert!(
                store
                    .interpreter_policy_command(
                        command.clone(),
                        &runtime,
                        Some(&"a".repeat(64)),
                        100
                    )
                    .is_err()
            );
        }
        assert_eq!(store.usage().unwrap(), before);
        assert_eq!(before["authorization_count"], usize::from(!policy_first));
        assert_eq!(
            before["interpreter_policy_count"],
            usize::from(policy_first)
        );
        drop(store);
        let store = JobStore::open(&path, 100, exact_limit).unwrap();
        if policy_first {
            save_automation(&store).unwrap();
        } else {
            call(&store, command.clone(), &runtime);
        }
        let after = store.usage().unwrap();
        assert_eq!(after["authorization_count"], 1);
        assert_eq!(after["interpreter_policy_count"], 1);
        assert_eq!(after["available_bytes"], 0);
        assert_eq!(after["over_limit"], false);
    }
}

#[test]
fn damaged_policy_records_or_column_metadata_never_become_an_empty_enabled_policy() {
    for field in ["json", "generation", "receipt"] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid::Uuid::new_v4().to_string();
        call(&store, approve(0), &runtime);
        let connection = store.connection.lock().unwrap();
        match field {
            "json" => {
                connection
                    .execute(
                        "UPDATE interpreter_service_policies SET record_json='{}'",
                        [],
                    )
                    .unwrap();
            }
            "generation" => {
                connection
                    .execute("UPDATE interpreter_service_policies SET generation=2", [])
                    .unwrap();
            }
            "receipt" => {
                connection
                    .execute(
                        "UPDATE interpreter_service_policies SET policy_id=?1",
                        [uuid::Uuid::new_v4().to_string()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before: Vec<String> = connection
            .prepare("SELECT record_json FROM interpreter_service_policies")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        drop(connection);
        assert!(
            store
                .interpreter_policy_command(Command::Show { id: None }, &runtime, None, 100)
                .is_err()
        );
        assert!(
            store
                .interpreter_policy_command(approve(0), &runtime, Some(&"a".repeat(64)), 100)
                .is_err()
        );
        let after: Vec<String> = store
            .connection
            .lock()
            .unwrap()
            .prepare("SELECT record_json FROM interpreter_service_policies")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(before, after);
    }
}
