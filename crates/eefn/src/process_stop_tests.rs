use super::*;
use crate::grants::{ExecutionGrant, GrantLedger, GrantReport, ReceiptState};
use serde_json::json;

fn observation() -> Observation {
    Observation {
        schema_version: 1,
        kind: "owned_process_tree_empty_after_stop_request".into(),
        mode: "windows_owned_job".into(),
        observed_at_ms: 1,
        stop_requested: true,
        owned_process_tree_empty: true,
        external_effects_stopped: false,
        finished_actions_undone: false,
    }
}
fn grant() -> ExecutionGrant {
    let mut grant = crate::grants::tests::grant();
    grant.capability = "process.exec".into();
    grant.params = json!({"program":"owner-selected-tool"});
    grant
}
fn report(grant: &ExecutionGrant) -> GrantReport {
    GrantReport::new(
        grant,
        ReceiptState::Completed,
        json!({"success":false,"process_stop_observation":observation()}),
        Some("process_exit_unsuccessful"),
    )
}
fn stop_report(grant: &ExecutionGrant) -> crate::grants::stop::Report {
    let directory = tempfile::tempdir().unwrap();
    let ledger = GrantLedger::open(
        &directory.path().join("ledger"),
        Default::default(),
        "owner",
    )
    .unwrap();
    ledger
        .request_stop(grant, &grant.issuer_id, &grant.session_id, "owner")
        .unwrap()
}
#[test]
fn positive_requires_both_exact_records_and_never_upgrades_v1_effect_guarantees() {
    let grant = grant();
    let stop = stop_report(&grant);
    let result = report(&grant);
    assert!(result.confirms(&grant));
    assert_eq!(confirmed(&grant, &stop, &result), Some(observation()));
    assert!(!stop.execution_stopped);
    assert!(!stop.external_effects_stopped);
    assert!(!observation().external_effects_stopped);
    assert!(!observation().finished_actions_undone);
}
#[test]
fn changed_grant_stop_or_result_scope_never_confirms() {
    let grant = grant();
    let stop = stop_report(&grant);
    let result = report(&grant);
    let mut other = grant.clone();
    other.params["args"] = json!(["another-command"]);
    assert!(confirmed(&other, &stop, &result).is_none());
    let mut changed = stop.clone();
    changed.grant_id = uuid::Uuid::new_v4().to_string();
    assert!(confirmed(&grant, &changed, &result).is_none());
    let mut changed = result.clone();
    changed.session_id = uuid::Uuid::new_v4().to_string();
    assert!(confirmed(&grant, &stop, &changed).is_none());
}
#[test]
fn unsupported_capabilities_and_missing_unknown_or_unpersisted_results_are_not_proof() {
    let grant = grant();
    let stop = stop_report(&grant);
    for changed in [
        GrantReport::new(
            &grant,
            ReceiptState::Unknown,
            Value::Null,
            Some("grant_stop_requested_effects_unknown"),
        ),
        GrantReport::new(
            &grant,
            ReceiptState::Completed,
            json!({"success":false,"stdout":serde_json::to_string(&observation()).unwrap()}),
            Some("process_exit_unsuccessful"),
        ),
        {
            let mut result = report(&grant);
            result.persisted = false;
            result
        },
        {
            let mut result = report(&grant);
            result.success = true;
            result
        },
    ] {
        assert!(confirmed(&grant, &stop, &changed).is_none());
    }
    let mut ping = grant.clone();
    ping.capability = "system.ping".into();
    ping.params = json!({});
    assert!(confirmed(&ping, &stop_report(&ping), &report(&ping)).is_none());
}
#[test]
fn malformed_future_or_outside_effect_claims_are_rejected() {
    let grant = grant();
    let stop = stop_report(&grant);
    for (key, value) in [
        ("schema_version", json!(2)),
        ("mode", json!("root_child_only")),
        ("owned_process_tree_empty", json!(false)),
        ("stop_requested", json!(false)),
        ("external_effects_stopped", json!(true)),
        ("finished_actions_undone", json!(true)),
        ("observed_at_ms", json!(0)),
        ("unexpected", json!(true)),
    ] {
        let mut result = report(&grant);
        result.data["process_stop_observation"][key] = value;
        assert!(confirmed(&grant, &stop, &result).is_none(), "{key}");
    }
}
#[tokio::test]
async fn private_control_close_is_not_a_stop_but_an_already_signalled_close_is() {
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let mut control = Control(receiver);
    drop(sender);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), control.requested())
            .await
            .is_err()
    );
    let (sender, receiver) = tokio::sync::watch::channel(false);
    sender.send_replace(true);
    drop(sender);
    let mut control = Control(receiver);
    tokio::time::timeout(std::time::Duration::from_secs(1), control.requested())
        .await
        .unwrap();
}
#[test]
fn only_direct_windows_process_grants_get_a_private_control() {
    let mut grant = grant();
    assert_eq!(control(&grant).is_some(), cfg!(windows));
    for cap in ["pipeline.run", "llm.infer", "peer.transfer", "system.ping"] {
        grant.capability = cap.into();
        assert!(control(&grant).is_none());
    }
}
