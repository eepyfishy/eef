use super::*;

fn command() -> Command {
    Command::Approve {
        operation_id: uuid::Uuid::new_v4().to_string(),
        generation: 0,
        scope: Scope {
            origin_nodes: vec!["input-node".into()],
            provider_nodes: vec!["model-node".into()],
        },
        limits: Limits::default(),
        valid_for_ms: Some(60_000),
    }
}
fn record() -> Authorization {
    let creation = command();
    let mut record = Authorization {
        schema_version: 1,
        id: creation.receipt().unwrap().into(),
        generation: 1,
        creation,
        runtime_id: uuid::Uuid::new_v4().to_string(),
        issuer_id: "a".repeat(64),
        approved_at_ms: 100,
        expires_at_ms: Some(60_100),
        authorization_sha256: String::new(),
        revision: 1,
        revocation: None,
    };
    record.authorization_sha256 = record.digest().unwrap();
    record.validate().unwrap();
    record
}
#[test]
fn explicit_scopes_require_private_nodes_and_bound_all_allowances_without_defaults_on_decode() {
    let command = command();
    command.validate().unwrap();
    let Command::Approve { scope, limits, .. } = &command else {
        unreachable!()
    };
    assert!(
        scope
            .permits("input-node", "model-node", true, true)
            .is_ok()
    );
    for (origin, provider, a, b) in [
        ("other", "model-node", true, true),
        ("input-node", "other", true, true),
        ("input-node", "model-node", false, true),
        ("input-node", "model-node", true, false),
    ] {
        assert!(scope.permits(origin, provider, a, b).is_err());
    }
    for field in [
        "max_output_tokens",
        "max_execution_ms",
        "max_calls",
        "max_in_flight_per_origin",
        "max_reserved_result_bytes",
    ] {
        let mut bad = json!(limits);
        bad[field] = json!(0);
        assert!(
            serde_json::from_value::<Limits>(bad)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    assert!(serde_json::from_value::<Limits>(json!({})).is_err());
    for pointer in [
        "/execution_authorized",
        "/scope/all_nodes",
        "/limits/unlimited",
    ] {
        let mut bad = json!(command);
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        bad.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(true));
        assert!(serde_json::from_value::<Command>(bad).is_err());
    }
    let mut empty = scope.clone();
    empty.provider_nodes.clear();
    assert!(empty.validate().is_err());
    let mut duplicate = scope.clone();
    duplicate.origin_nodes.push("input-node".into());
    assert!(duplicate.validate().is_err());
}
#[test]
fn immutable_policy_digest_generation_and_revocation_never_imply_execution_or_renewal() {
    let record = record();
    assert!(record.active(1, &"a".repeat(64), 100));
    assert!(!record.active(2, &record.issuer_id, 100));
    assert!(!record.active(1, &"b".repeat(64), 100));
    assert!(!record.active(1, &record.issuer_id, 99));
    assert!(!record.active(1, &record.issuer_id, 60_100));
    let mut changed = record.clone();
    if let Command::Approve { scope, .. } = &mut changed.creation {
        scope.provider_nodes.push("another-node".into());
    }
    assert!(changed.validate().is_err());
    let mut revoked = record.clone();
    revoked.revision = 2;
    revoked.revocation = Some(Revocation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        source_revision: 1,
        runtime_id: uuid::Uuid::new_v4().to_string(),
        at_ms: 1,
    });
    revoked.validate().unwrap();
    assert_eq!(revoked.authorization_sha256, record.authorization_sha256);
    assert!(!revoked.active(1, &record.issuer_id, 100));
    let mut repeat = record.creation.clone();
    if let Command::Approve { generation, .. } = &mut repeat {
        *generation = 1;
    }
    assert!(record.creation.same_approval(&repeat));
}
#[test]
fn policy_confirmation_refuses_forged_availability_and_wrong_authority_or_receipts() {
    let record = record();
    let command = record.creation.clone();
    let runtime = record.runtime_id.clone();
    let report = json!(Report {
        schema_version: 1,
        report_type: "coordinator_interpreter_policy".into(),
        success: true,
        runtime_id: runtime.clone(),
        command: command.clone(),
        current_generation: 1,
        authorization: Some(record),
        items: vec![],
        next_after: None,
        reused: false,
        changed: true,
        grants_issued: false,
        model_inference_requested: false,
        automatically_retried: false,
        allowance_reservation_available: false,
        automatic_delegation_available: false
    });
    assert!(confirmed(&report, &command, &runtime));
    for field in [
        "grants_issued",
        "model_inference_requested",
        "automatically_retried",
        "allowance_reservation_available",
        "automatic_delegation_available",
        "reused",
    ] {
        let mut bad = report.clone();
        bad[field] = json!(true);
        assert!(!confirmed(&bad, &command, &runtime));
    }
    let mut wrong = report.clone();
    wrong["authorization"]["generation"] = json!(2);
    assert!(!confirmed(&wrong, &command, &runtime));
    let mut wrong = report;
    wrong["runtime_id"] = json!(uuid::Uuid::new_v4().to_string());
    assert!(!confirmed(&wrong, &command, &runtime));
}
