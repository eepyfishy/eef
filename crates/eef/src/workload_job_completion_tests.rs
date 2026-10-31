// Coordinator journal tests use explicit synthetic authenticated-node receipts.
// Node native tests separately require real owned source/process evidence.
fn completed_receipt(d: &Definition, ack: &eefn::grants::workload::Receipt, sequence: u64) -> eefn::grants::workload::Receipt {
    let Source::MicrophoneEnergy { parameters, adapter_sha256, .. } = &d.source;
    let mut receipt = ack.receipt.clone(); receipt.state = "completed".into();
    receipt.updated_at_ms += 1;
    eefn::grants::workload::Receipt { schema_version: 3, definition: d.clone(),
        completion: Some(eefn::grants::workload::CompletionReport { schema_version: 1,
            grant_sha256: d.scope().unwrap().grant_sha256, adapter_sha256: adapter_sha256.clone(),
            source_observations: u64::from(parameters.max_blocks),
            source_frames: u64::from(parameters.max_blocks) * u64::from(parameters.block_frames),
            event_count: 1, event_bytes: 200, event_prefix_sha256: "f".repeat(64), lease_sequence: sequence,
            recorded_at_ms: receipt.updated_at_ms, owned_process_exit_observed: true, physical_effects_stopped: false }),
        receipt, execution_authorized: false, restart_authorized: false }
}
fn completed_fixture(path: &std::path::Path) -> (Arc<JobStore>, Issued, eefn::grants::workload::Receipt) {
    let store = new_store(path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    let mut issued = issue(&store, &execute(&plan), &uuid::Uuid::new_v4().to_string(), &policy()).unwrap();
    lease(&mut issued, 1).unwrap();
    let ack = synthetic_ack(issued.definition());
    issued.acknowledge(ack.clone()).unwrap();
    let receipt = completed_receipt(issued.definition(), &ack, 1);
    receipt.validate().unwrap();
    (store, issued, receipt)
}
fn finish(issued: &mut Issued, receipt: eefn::grants::workload::Receipt) -> Result<Plan> {
    let d = issued.definition().clone();
    issued.complete(receipt, &d.issuer_id, &d.session_id, &d.executor_node)
}

#[test]
fn completed_workload_is_durable_idempotent_terminal_and_never_restores_an_issuer() {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("completed.db");
    let (store, mut issued, receipt) = completed_fixture(&path);
    lease(&mut issued, 2).unwrap();
    let done = finish(&mut issued, receipt.clone()).unwrap();
    assert_eq!(done.status, PlanStatus::Completed);
    assert_eq!(done.tasks[0].status, TaskStatus::Completed);
    assert_eq!(done.execution.as_ref().unwrap().schema_version, 3);
    let w = done.execution.as_ref().unwrap().workload.as_ref().unwrap();
    assert_eq!(w.state, AttemptState::Completed);
    let report = w.completion.as_ref().unwrap();
    assert_eq!(report.last_issued_lease_sequence, 2);
    assert_eq!(report.receipt.accepted_at_ms, 42, "Node clock is not compared to coordinator wall time");
    assert!(report.report.owned_process_exit_observed && !report.report.physical_effects_stopped);
    assert_eq!(crate::request_execution::reserved_bytes(&done), 0);
    assert_eq!(finish(&mut issued, receipt.clone()).unwrap().revision, done.revision);
    assert!(lease(&mut issued, 3).is_err());
    let mut interrupted = done.clone(); crate::request_execution::interrupt(&mut interrupted);
    assert_eq!(json!(interrupted), json!(done));
    drop(issued); drop(store);
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    assert_eq!(json!(reopened.get(&done.plan_id).unwrap()), json!(done));
    assert!(issue(&reopened, &execute(&done), &uuid::Uuid::new_v4().to_string(), &policy()).is_err());
}

#[test]
fn completion_requires_consumed_ack_and_a_lease_sequence_that_was_actually_issued() {
    let engine = TaskEngine::new(EventBus::new(20)); let (plan, _) = approved(&engine);
    let mut issued = issue(&engine.store, &execute(&plan), &uuid::Uuid::new_v4().to_string(), &policy()).unwrap();
    let ack = synthetic_ack(issued.definition());
    let receipt = completed_receipt(issued.definition(), &ack, 1);
    let before = json!(engine.store.get(&plan.plan_id).unwrap());
    assert!(finish(&mut issued, receipt.clone()).is_err());
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), before);
    lease(&mut issued, 1).unwrap(); issued.acknowledge(ack).unwrap();
    let accepted = engine.store.get(&plan.plan_id).unwrap();
    assert!(crate::request_execution::reserved_bytes(&accepted) >= COMPLETION_RESERVE);
    let mut future = receipt.clone(); future.completion.as_mut().unwrap().lease_sequence = 2;
    assert!(finish(&mut issued, future).is_err());
    assert_eq!(engine.store.get(&plan.plan_id).unwrap().revision, accepted.revision);
    assert_eq!(finish(&mut issued, receipt).unwrap().status, PlanStatus::Completed);
}

#[test]
fn wrong_peer_scope_or_consumption_timestamp_cannot_change_a_completed_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut issued, receipt) = completed_fixture(&dir.path().join("scope.db"));
    let d = issued.definition().clone(); let before = json!(store.get(&d.job_id).unwrap());
    for (issuer, session, node) in [
        ("b".repeat(64), d.session_id.clone(), d.executor_node.clone()),
        (d.issuer_id.clone(), uuid::Uuid::new_v4().to_string(), d.executor_node.clone()),
        (d.issuer_id.clone(), d.session_id.clone(), "another-node".into())] {
        assert!(issued.complete(receipt.clone(), &issuer, &session, &node).is_err());
        assert_eq!(json!(store.get(&d.job_id).unwrap()), before);
    }
    let mut other = d.clone(); other.plan_sha256 = "c".repeat(64);
    assert!(finish(&mut issued, completed_receipt(&other, &synthetic_ack(&other), 1)).is_err());
    let mut changed_ack = synthetic_ack(&d); changed_ack.receipt.accepted_at_ms = 99; changed_ack.receipt.updated_at_ms = 99;
    let mismatch = completed_receipt(&d, &changed_ack, 1); mismatch.validate().unwrap();
    assert!(finish(&mut issued, mismatch).is_err());
    assert_eq!(json!(store.get(&d.job_id).unwrap()), before);
    let done = finish(&mut issued, receipt.clone()).unwrap();
    assert!(issued.complete(receipt, &d.issuer_id, &d.session_id, "another-node").is_err());
    assert_eq!(store.get(&d.job_id).unwrap().revision, done.revision);
}

#[test]
fn completed_evidence_is_immutable_and_unknown_tracking_cannot_be_refined_by_a_stale_issuer() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut issued, receipt) = completed_fixture(&dir.path().join("terminal.db"));
    let done = finish(&mut issued, receipt.clone()).unwrap();
    for index in 0..3 {
        assert!(store.change(&done.plan_id, "synthetic forbidden terminal edit", |plan| {
            let w = plan.execution.as_mut().unwrap().workload.as_mut().unwrap();
            match index { 0 => w.completion = None,
                1 => w.completion.as_mut().unwrap().report.event_prefix_sha256 = "b".repeat(64),
                _ => { w.state = AttemptState::Unknown; plan.status = PlanStatus::Interrupted; plan.tasks[0].status = TaskStatus::Interrupted; } }
            Ok(())
        }).is_err());
        assert_eq!(json!(store.get(&done.plan_id).unwrap()), json!(done));
    }
    let (other, mut retired, receipt) = completed_fixture(&dir.path().join("unknown.db"));
    retired.issued_wall = retired.issued_wall.saturating_add(100_000);
    assert!(lease(&mut retired, 2).is_err());
    let unknown = other.get(&receipt.definition.job_id).unwrap();
    assert_eq!(unknown.status, PlanStatus::Interrupted);
    retired.issued_wall = eefn::protocol::now_ms();
    assert!(finish(&mut retired, receipt).is_err());
    assert_eq!(json!(other.get(&unknown.plan_id).unwrap()), json!(unknown));
}

#[test]
fn failed_completion_storage_retires_the_issuer_and_never_retries_the_completion() {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("failure.db");
    let (store, mut issued, receipt) = completed_fixture(&path);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_completed BEFORE UPDATE ON durable_jobs WHEN json_extract(NEW.plan_json,'$.status')='completed' BEGIN SELECT RAISE(ABORT,'synthetic completion failure'); END;").unwrap();
    assert!(finish(&mut issued, receipt.clone()).is_err());
    let saved = store.get(&receipt.definition.job_id).unwrap();
    assert_eq!(saved.status, PlanStatus::Interrupted);
    let w = saved.execution.as_ref().unwrap().workload.as_ref().unwrap();
    assert_eq!(w.state, AttemptState::Unknown); assert!(w.completion.is_none());
    assert!(issued.retired);
    db.execute_batch("DROP TRIGGER refuse_completed").unwrap();
    assert!(finish(&mut issued, receipt).is_err()); assert!(lease(&mut issued, 2).is_err());
    drop(issued);
    assert_eq!(store.get(&saved.plan_id).unwrap().revision, saved.revision);
}

#[test]
fn corrupt_coordinator_completion_is_refused_on_reopen_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..9 {
        let path = dir.path().join(format!("corrupt-{index}.db"));
        let (store, mut issued, receipt) = completed_fixture(&path);
        let mut done = finish(&mut issued, receipt).unwrap();
        let e = done.execution.as_mut().unwrap(); let w = e.workload.as_mut().unwrap();
        match index { 0 => e.schema_version = 2, 1 => w.completion = None,
            2 => w.completion.as_mut().unwrap().last_issued_lease_sequence = 0,
            3 => w.completion.as_mut().unwrap().receipt.accepted_at_ms += 1,
            4 => w.completion.as_mut().unwrap().report.owned_process_exit_observed = false,
            5 => w.completion.as_mut().unwrap().report.physical_effects_stopped = true,
            6 => w.completion.as_mut().unwrap().recorded_at_ms = 0,
            7 => w.completion.as_mut().unwrap().recorded_at_ms = u64::MAX,
            _ => { done.status = PlanStatus::Running; done.tasks[0].status = TaskStatus::Running; } }
        drop(issued); drop(store);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("UPDATE durable_jobs SET plan_json=?1", [serde_json::to_string(&done).unwrap()]).unwrap(); drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(JobStore::open(&path, 20, 8 * 1024 * 1024).is_err(), "Corruption {index}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "No repair for {index}");
    }
}
