#[test]
fn workload_receipt_commands_validate_origin_and_exact_issuance_without_a_job_write() {
    use crate::workload_job_inspection::{report, retained};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("receipt.db");
    let (store, mut issued, receipt) = completed_fixture(&path);
    let d = issued.definition().clone();
    let plan = store.get(&d.job_id).unwrap();
    let command = eefn::request_receipt::ReceiptCommand {
        id: d.job_id.clone(),
        grant_id: d.grant_id.clone(),
    };
    let runtime = uuid::Uuid::new_v4().to_string();
    let origin = d.request_context.origin_node();
    let before = json!(plan);
    let disk = std::fs::read(&path).unwrap();
    assert!(retained(&plan, &command, Some(origin)).is_ok());
    assert!(retained(&plan, &command, Some("other-origin")).is_err());
    let mut wrong = command.clone();
    wrong.grant_id = uuid::Uuid::new_v4().to_string();
    assert!(retained(&plan, &wrong, None).is_err());
    wrong = command.clone();
    wrong.id = uuid::Uuid::new_v4().simple().to_string();
    assert!(retained(&plan, &wrong, None).is_err());
    for receipt in [synthetic_ack(&d), receipt.clone()] {
        let value = report(&plan, &command, Some(origin), &runtime, receipt).unwrap();
        assert!(eefn::request_receipt::confirmed(
            &value,
            &command,
            Some(origin),
            Some(&runtime)
        ));
        assert!(!eefn::request_receipt::confirmed(
            &value,
            &command,
            Some("other-origin"),
            None
        ));
        assert!(!eefn::request_receipt::confirmed(
            &value,
            &command,
            None,
            Some(&uuid::Uuid::new_v4().to_string())
        ));
        assert_eq!(value["coordinator_attempt_state"], "accepted");
        assert_eq!(value["reconciled"], false);
    }
    let mut changed = receipt.clone();
    changed.definition.priority += 1;
    changed.receipt.grant_sha256 = changed.definition.scope().unwrap().grant_sha256;
    changed.completion.as_mut().unwrap().grant_sha256 = changed.receipt.grant_sha256.clone();
    changed.validate().unwrap();
    assert!(report(&plan, &command, None, &runtime, changed).is_err());
    assert_eq!(json!(store.get(&d.job_id).unwrap()), before);
    assert_eq!(std::fs::read(&path).unwrap(), disk);
    let done = finish(&mut issued, receipt.clone()).unwrap();
    drop(issued);
    drop(store);
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    let history = reopened.get(&d.job_id).unwrap();
    let disk = std::fs::read(&path).unwrap();
    let value = report(&history, &command, None, &runtime, receipt).unwrap();
    assert_eq!(value["coordinator_attempt_state"], "completed");
    assert_eq!(history.revision, done.revision);
    assert_eq!(std::fs::read(&path).unwrap(), disk);
}

#[test]
fn typed_workload_receipts_reject_authority_scope_corruption_and_ordinary_reinterpretation() {
    let dir = tempfile::tempdir().unwrap();
    let (store, issued, receipt) = completed_fixture(&dir.path().join("corrupt.db"));
    let d = issued.definition();
    let plan = store.get(&d.job_id).unwrap();
    let command = eefn::request_receipt::ReceiptCommand {
        id: d.job_id.clone(),
        grant_id: d.grant_id.clone(),
    };
    let runtime = uuid::Uuid::new_v4().to_string();
    let value =
        crate::workload_job_inspection::report(&plan, &command, None, &runtime, receipt).unwrap();
    for (field, bad) in [
        ("schema_version", json!(1)),
        ("report_type", json!("coordinator_request_receipt")),
        ("read_only", json!(false)),
        ("execution_requested", json!(true)),
        ("automatically_retried", json!(true)),
        ("reconciled", json!(true)),
        ("revision", json!(0)),
        ("note", json!("resume")),
    ] {
        let mut bad_value = value.clone();
        bad_value[field] = bad;
        assert!(
            !eefn::request_receipt::confirmed(&bad_value, &command, None, Some(&runtime)),
            "{field}"
        );
    }
    for pointer in [
        "/receipt/execution_authorized",
        "/receipt/restart_authorized",
        "/receipt/completion/physical_effects_stopped",
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = json!(true);
        assert!(
            !eefn::request_receipt::confirmed(&bad, &command, None, None),
            "{pointer}"
        );
    }
    let mut extra = value.clone();
    extra["renew"] = json!(true);
    assert!(!eefn::request_receipt::confirmed(
        &extra, &command, None, None
    ));
    let request = uuid::Uuid::new_v4().to_string();
    let node = d.request_context.origin_node();
    let wrapped = json!({"schema_version":1,"report_type":"request_receipt","node_id":node,"request_id":request,"id":command.id,"grant_id":command.grant_id,
        "success":true,"execution_requested":false,"automatically_retried":false,"reconciled":false,"note":"Read-only evidence","result":value});
    assert!(eefn::request_receipt::confirmed_node(
        &wrapped, &command, node, &request
    ));
    assert!(!eefn::request_receipt::confirmed_node(
        &wrapped,
        &command,
        "other-node",
        &request
    ));
    assert!(!eefn::request_receipt::confirmed_node(
        &wrapped,
        &command,
        node,
        &uuid::Uuid::new_v4().to_string()
    ));
}
