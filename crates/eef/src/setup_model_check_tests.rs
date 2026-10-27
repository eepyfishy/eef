use super::*;
use serde_json::json;

pub(crate) fn definition() -> Definition {
    Definition {
        schema_version: 1,
        node_id: "node".into(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        model: ModelRef {
            backend: eefn::model_selection::Backend::Llamacpp,
            model_id: "chosen".into(),
        },
        max_wait_ms: 5000,
    }
}
fn request() -> Request {
    let definition = definition();
    Request {
        schema_version: 1,
        expected_runtime_id: uuid::Uuid::new_v4().to_string(),
        node_id: definition.node_id,
        operation_id: definition.operation_id,
        command: Command::Arm {
            model: definition.model,
            max_wait_ms: definition.max_wait_ms,
            approve_inference: true,
        },
    }
}
fn report(request: &Request) -> Value {
    let record = Record {
        schema_version: 1,
        definition: request.definition().unwrap(),
        coordinator_runtime_id: request.expected_runtime_id.clone(),
        created_at_ms: 1,
        state: State::Waiting,
        outcome: None,
    };
    serde_json::to_value(Report {
        schema_version: 1,
        report_type: "coordinator_setup_model_check".into(),
        coordinator_runtime_id: request.expected_runtime_id.clone(),
        node_id: request.node_id.clone(),
        operation_id: request.operation_id.clone(),
        command: request.command.clone(),
        success: true,
        record: Some(record),
        effective_state: Some(State::Waiting),
        worker_active: true,
        waiting_reason: Some("waiting_for_setup".into()),
        consent_created: true,
        waiting_cancelled: false,
        reused: false,
        automatically_retried: false,
        resumed: false,
        model_selection_changed: false,
        model_quality_approved: false,
        current_setup_verified: false,
    })
    .unwrap()
}
#[test]
fn consent_and_targets_are_exact_and_wait_is_bounded() {
    let valid = request();
    valid.validate().unwrap();
    for (field, value) in [
        ("approve_inference", json!(false)),
        ("max_wait_ms", json!(0)),
        ("max_wait_ms", json!(MAX_WAIT_MS + 1)),
        ("prompt", json!("PRIVATE replacement")),
    ] {
        let mut changed = serde_json::to_value(&valid).unwrap();
        changed["command"][field] = value;
        if let Ok(parsed) = serde_json::from_value::<Request>(changed) {
            assert!(parsed.validate().is_err(), "{field}");
        }
    }
    for value in [json!("auto"), json!("PRIVATE backend")] {
        let mut changed = serde_json::to_value(&valid).unwrap();
        changed["command"]["model"]["backend"] = value;
        assert!(serde_json::from_value::<Request>(changed).is_err());
    }
    let mut changed = valid;
    changed.operation_id = "not-a-uuid".into();
    assert!(changed.validate().is_err());
}
#[test]
fn strict_reports_reject_forged_readiness_scope_and_private_peer_text() {
    let request = request();
    let valid = report(&request);
    assert!(confirmed(&valid, &request));
    for (field, value) in [
        ("success", json!(false)),
        ("effective_state", json!("finished")),
        ("automatically_retried", json!(true)),
        ("resumed", json!(true)),
        ("model_selection_changed", json!(true)),
        ("model_quality_approved", json!(true)),
        ("current_setup_verified", json!(true)),
        ("waiting_cancelled", json!(true)),
        ("reused", json!(true)),
        ("waiting_reason", json!("PRIVATE peer error")),
        ("PRIVATE", json!("secret")),
    ] {
        let mut changed = valid.clone();
        changed[field] = value;
        assert!(!confirmed(&changed, &request), "{field}");
    }
    let mut changed = valid.clone();
    changed["record"]["definition"]["model"]["model_id"] = json!("other");
    assert!(!confirmed(&changed, &request));
    let mut changed = valid;
    changed["record"] = Value::Null;
    changed["worker_active"] = json!(false);
    changed["waiting_reason"] = Value::Null;
    changed["effective_state"] = Value::Null;
    changed["consent_created"] = json!(false);
    assert!(!confirmed(&changed, &request));
    assert!(render(&changed).contains("unconfirmed"));
}
#[test]
fn missing_or_changed_worker_ownership_is_never_readiness_or_resume() {
    let request = request();
    let value = report(&request);
    let mut record: Record = serde_json::from_value(value["record"].clone()).unwrap();
    assert_eq!(
        effective_state(&record, &request.expected_runtime_id, false),
        State::Interrupted
    );
    record.state = State::Launching;
    assert_eq!(
        effective_state(&record, &request.expected_runtime_id, false),
        State::Unknown
    );
    assert_eq!(
        effective_state(&record, &uuid::Uuid::new_v4().to_string(), true),
        State::Unknown
    );
    record.state = State::Finished;
    assert!(record.validate().is_err());
    record.outcome = Some(Outcome {
        state: crate::model_verification::State::Running,
        node_runtime_id: None,
        job_id: None,
        grant_id: None,
        historical: false,
        outcome_unknown: false,
        inference_returned_text: false,
    });
    assert!(record.validate().is_err());
    record.state = State::Unknown;
    record.outcome = None;
    record.validate().unwrap();
}
#[test]
fn inspection_and_cancel_cannot_smuggle_consent_or_model_arguments() {
    let mut request = request();
    request.command = Command::Inspect {};
    request.validate().unwrap();
    for command in [Command::Inspect {}, Command::CancelWait {}] {
        request.command = command;
        let mut value = serde_json::to_value(&request).unwrap();
        value["command"]["approve_inference"] = json!(true);
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}
#[test]
fn admission_settings_default_without_opt_in_and_refuse_unbounded_or_unknown_fields() {
    assert_eq!(validate_settings(None).unwrap(), 2);
    assert_eq!(
        validate_settings(Some(&json!({"max_active":8}))).unwrap(),
        8
    );
    for value in [
        json!(null),
        json!([]),
        json!({"max_active":0}),
        json!({"max_active":9}),
        json!({"max_active":1.5}),
        json!({"enabled":true}),
    ] {
        assert!(validate_settings(Some(&value)).is_err());
    }
}

#[test]
fn plain_status_distinguishes_returned_text_no_text_and_failure_without_json() {
    let request = request();
    let mut value = report(&request);
    value["record"]["state"] = json!("finished");
    value["effective_state"] = json!("finished");
    value["worker_active"] = json!(false);
    value["waiting_reason"] = Value::Null;
    value["record"]["outcome"] = json!({"state":"returned_text","node_runtime_id":uuid::Uuid::new_v4().to_string(),"job_id":uuid::Uuid::new_v4().simple().to_string(),"grant_id":uuid::Uuid::new_v4().to_string(),"historical":true,"outcome_unknown":false,"inference_returned_text":true});
    assert!(confirmed(&value, &request));
    assert!(render(&value).contains("model answered"));
    value["record"]["outcome"]["state"] = json!("no_text");
    value["record"]["outcome"]["inference_returned_text"] = json!(false);
    assert!(confirmed(&value, &request));
    assert!(render(&value).contains("without model text"));
    value["record"]["outcome"]["state"] = json!("failed");
    assert!(confirmed(&value, &request));
    assert!(render(&value).contains("check failed"));
}

#[test]
fn config_load_and_save_reject_invalid_admission_settings_without_rewriting_owner_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("eef.yaml");
    let original = b"custom_owner_value: preserve\n";
    std::fs::write(&path, original).unwrap();
    let config = crate::config::Config::load(Some(&path)).unwrap();
    for settings in [
        json!([]),
        json!({"max_active":0}),
        json!({"max_active":9}),
        json!({"enabled":true}),
    ] {
        let mut changed = config.as_value();
        changed["setup_model_checks"] = settings;
        assert!(config.save_for_restart(&changed).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    let mut changed = config.as_value();
    changed["setup_model_checks"] = json!({"max_active":1});
    config.save_for_restart(&changed).unwrap();
    let saved = crate::config::Config::load(Some(&path)).unwrap();
    assert_eq!(saved.get("custom_owner_value"), Some(&json!("preserve")));
    assert_eq!(
        validate_settings(saved.get("setup_model_checks")).unwrap(),
        1
    );
    std::fs::write(&path, b"setup_model_checks: []\n").unwrap();
    assert!(crate::config::Config::load(Some(&path)).is_err());
}

#[test]
fn outcome_uses_compact_job_ids_and_canonical_runtime_and_grant_uuids() {
    let valid = Outcome {
        state: crate::model_verification::State::ReturnedText,
        node_runtime_id: Some(uuid::Uuid::new_v4().to_string()),
        job_id: Some(uuid::Uuid::new_v4().simple().to_string()),
        grant_id: Some(uuid::Uuid::new_v4().to_string()),
        historical: true,
        outcome_unknown: false,
        inference_returned_text: true,
    };
    valid.validate().unwrap();
    let mut changed = valid.clone();
    changed.job_id = Some(uuid::Uuid::new_v4().to_string());
    assert!(changed.validate().is_err());
    let mut changed = valid.clone();
    changed.job_id = Some("../PRIVATE".into());
    assert!(changed.validate().is_err());
    let mut changed = valid.clone();
    changed.node_runtime_id = Some(uuid::Uuid::new_v4().simple().to_string());
    assert!(changed.validate().is_err());
    let mut changed = valid;
    changed.grant_id = Some(uuid::Uuid::new_v4().simple().to_string());
    assert!(changed.validate().is_err());
}
