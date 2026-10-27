use super::*;
use serde_json::json;
fn fixture() -> Report {
    let grant = crate::grants::tests::grant();
    Report {
        schema_version: 1,
        report_type: "job_process_stop_status".into(),
        success: true,
        runtime_id: uuid::Uuid::new_v4().to_string(),
        id: grant.job_id.clone(),
        revision: 3,
        origin_node: "owner".into(),
        stop_requested: true,
        issued_attempts: 2,
        not_inspected_issued_attempts: 1,
        grants: vec![GrantStatus {
            scope: Scope::from_grant(&grant).unwrap(),
            availability: Availability::Unavailable,
            intent_recorded: false,
            observation: None,
        }],
        read_only: true,
        execution_requested: false,
        stop_signal_sent: false,
        automatically_retried: false,
        reconciled: false,
        external_effects_stopped: false,
        finished_actions_undone: false,
        note: NOTE.into(),
    }
}
#[test]
fn coverage_and_plain_status_never_infer_a_job_wide_stop() {
    let report = fixture();
    let command = Command {
        id: report.id.clone(),
    };
    let value = serde_json::to_value(&report).unwrap();
    assert!(confirmed(
        &value,
        &command,
        Some("owner"),
        Some(&report.runtime_id)
    ));
    let text = render(&value, &command, Some("owner")).unwrap();
    assert!(text.contains("1 of 2 issued attempts not inspected"));
    assert!(text.contains("unconfirmed"));
    assert!(!text.to_lowercase().contains("job stopped"));
    assert!(!confirmed(&value, &command, Some("another-node"), None));
    assert!(!confirmed(
        &value,
        &command,
        None,
        Some(&uuid::Uuid::new_v4().to_string())
    ));
}
#[test]
fn no_target_is_not_negative_effect_or_termination_proof() {
    let mut report = fixture();
    report.grants.clear();
    report.not_inspected_issued_attempts = 2;
    let command = Command {
        id: report.id.clone(),
    };
    let text = render(&json!(report), &command, None).unwrap();
    assert!(text.contains("does not prove no work is running"));
    assert!(text.contains("2 of 2"));
}
#[test]
fn mutation_flags_future_fields_wrong_scopes_and_bad_coverage_are_rejected() {
    let report = fixture();
    let command = Command {
        id: report.id.clone(),
    };
    for (key, mutation) in [
        ("schema_version", json!(2)),
        ("read_only", json!(false)),
        ("execution_requested", json!(true)),
        ("stop_signal_sent", json!(true)),
        ("automatically_retried", json!(true)),
        ("reconciled", json!(true)),
        ("external_effects_stopped", json!(true)),
        ("finished_actions_undone", json!(true)),
        ("not_inspected_issued_attempts", json!(0)),
        ("unexpected", json!(true)),
    ] {
        let mut value = json!(report);
        value[key] = mutation;
        assert!(!confirmed(&value, &command, None, None), "{key}");
    }
    let mut duplicate = report.clone();
    duplicate.grants.push(duplicate.grants[0].clone());
    duplicate.not_inspected_issued_attempts = 0;
    assert!(duplicate.validate().is_err());
    let mut wrong = report.clone();
    wrong.grants[0].scope.job_id = "f".repeat(32);
    assert!(wrong.validate().is_err());
}
#[test]
fn a_positive_label_requires_an_actual_valid_observation_and_intent() {
    let mut report = fixture();
    report.grants[0].availability = Availability::OwnedProcessTreeEmpty;
    assert!(report.validate().is_err());
    report.grants[0].observation = Some(Observation {
        schema_version: 1,
        kind: "owned_process_tree_empty_after_stop_request".into(),
        mode: "windows_owned_job".into(),
        observed_at_ms: 1,
        stop_requested: true,
        owned_process_tree_empty: true,
        external_effects_stopped: false,
        finished_actions_undone: false,
    });
    assert!(report.validate().is_err());
    report.grants[0].intent_recorded = true;
    assert!(report.validate().is_ok());
    report.grants[0]
        .observation
        .as_mut()
        .unwrap()
        .external_effects_stopped = true;
    assert!(report.validate().is_err());
}
