use super::*;
use crate::{event::EventBus, request_planning::PlanningCommand, task::TaskEngine};
use eefn::{
    request_drafts::{DraftRecord, ProposalRevision},
    request_planning::PlanInput,
    request_submission::ConcreteInput,
    workload_leases::definition::*,
};

fn template_input() -> PlanInput {
    PlanInput {
        schema_version: 1,
        template: TEMPLATE.into(),
        constraints: json!({}),
        params: json!({"workload":Template {
            schema_version:2,executor_node:"executor".into(),priority:0,
            source:Source::MicrophoneEnergy {resource_id:"executor::mic".into(),resource_sha256:"d".repeat(64),adapter_sha256:"e".repeat(64),
                parameters:EnergyParameters {sample_rate:16000,channels:1,block_frames:80,max_blocks:100,activation_level:1000,deactivation_level:500}},
            limits:Limits {max_session_ms:60000,max_lease_ms:10000,max_job_memory_mib:128,max_source_frames:8000},
            events:Events {mode:EventMode::ActivityTransitions,min_interval_ms:100,max_events:100,max_event_bytes:1024,max_total_bytes:65536,max_pending:4,overflow:Overflow::Stop},
        }}),
    }
}
fn review(engine: &TaskEngine) -> (Plan, PlanInput) {
    let input = template_input();
    let concrete = ConcreteInput {
        schema_version: 1,
        original_text: "Observe microphone energy for a bounded continuous session".into(),
        title: "Lightweight microphone energy".into(),
        input: input.clone(),
    };
    let context = eefn::context::RequestContext::new(
        uuid::Uuid::new_v4().to_string(),
        "origin".into(),
        vec!["home".into(), "bedroom".into()],
    )
    .unwrap();
    let draft = DraftRecord {
        schema_version: 1,
        original_text: concrete.original_text.clone(),
        concrete_input: Some(input.clone()),
        audio_source: None,
        proposals: vec![ProposalRevision {
            operation_id: context.request_id().into(),
            proposal: concrete.proposal().unwrap(),
            allowed_capabilities: eefn::request_planning::concrete_capabilities(&input).unwrap(),
            at_ms: eefn::protocol::now_ms(),
            resolution: None,
            preparation: None,
        }],
        answers: vec![],
    };
    (
        crate::request_drafts::store_review(engine, draft, &context)
            .unwrap()
            .plan,
        input,
    )
}
fn planned(engine: &TaskEngine) -> (Plan, PlanningCommand) {
    let (review, input) = review(engine);
    let command = PlanningCommand::Compile {
        id: review.plan_id,
        expected_revision: review.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    (compile(engine, &command).unwrap(), command)
}
fn approved(engine: &TaskEngine) -> (Plan, ApprovalCommand) {
    let (plan, _) = planned(engine);
    let command = ApprovalCommand {
        id: plan.plan_id.clone(),
        expected_revision: plan.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        plan_sha256: plan.planning.as_ref().unwrap().plan_sha256.clone(),
        valid_for_ms: 120000,
    };
    (
        approve(&engine.store, &command, &"a".repeat(64)).unwrap(),
        command,
    )
}
fn execute(plan: &Plan) -> ExecuteCommand {
    ExecuteCommand {
        id: plan.plan_id.clone(),
        expected_revision: plan.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        approval_sha256: plan.approval.as_ref().unwrap().approval_sha256.clone(),
    }
}
fn policy() -> Policy {
    Policy {
        enabled: true,
        ..Default::default()
    }
}
fn new_store(path: &std::path::Path, max_bytes: usize) -> Arc<JobStore> {
    drop(rusqlite::Connection::open(path).unwrap());
    JobStore::open(path, 20, max_bytes).unwrap()
}
fn query(issued: &Issued, sequence: u64) -> Query {
    Query {
        kind: "workload_lease_probe".into(),
        schema_version: 2,
        id: uuid::Uuid::new_v4().to_string(),
        contract: issued.definition().lease_contract().unwrap(),
        sequence,
    }
}
fn lease(issued: &mut Issued, sequence: u64) -> Result<Reply> {
    let d = issued.definition().clone();
    issued.lease(
        &query(issued, sequence),
        &d.issuer_id,
        &d.session_id,
        &d.executor_node,
    )
}
fn synthetic_ack(d: &Definition) -> eefn::grants::workload::Receipt {
    let scope = d.scope().unwrap();
    eefn::grants::workload::Receipt {
        schema_version: 2,
        definition: d.clone(),
        execution_authorized: false,
        restart_authorized: false,
        receipt: eefn::request_receipt::ConsumedReceipt {
            schema_version: 1,
            node_id: d.executor_node.clone(),
            issuer_id: d.issuer_id.clone(),
            grant_id: d.grant_id.clone(),
            grant_sha256: scope.grant_sha256,
            job_id: d.job_id.clone(),
            task_id: d.task_id.clone(),
            attempt: 1,
            state: "accepted".into(),
            accepted_at_ms: 42,
            updated_at_ms: 42,
            pipeline_progress: None,
        },
    }
}

#[test]
fn existing_job_store_persists_review_approval_definition_and_ack_before_renewals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (approved, approval) = approved(&engine);
    assert_eq!(approved.approval.as_ref().unwrap().schema_version, 2);
    assert_eq!(
        json!(approve(&store, &approval, &"a".repeat(64)).unwrap()),
        json!(approved),
        "retry is read-only, not renewal"
    );
    let command = execute(&approved);
    let mut issued = issue(
        &store,
        &command,
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    let d = issued.definition().clone();
    let db = rusqlite::Connection::open(&path).unwrap();
    let raw: String = db
        .query_row(
            "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
            [&approved.plan_id],
            |r| r.get(0),
        )
        .unwrap();
    let durable: Plan = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        durable
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .definition,
        d
    );
    assert!(
        lease(&mut issued, 2).is_err(),
        "No renewal before consumed ACK"
    );
    lease(&mut issued, 1).unwrap();
    let ack = synthetic_ack(&d);
    issued.acknowledge(ack.clone()).unwrap();
    let after = store.get(&approved.plan_id).unwrap();
    assert_eq!(
        after
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .state,
        AttemptState::Accepted
    );
    issued.acknowledge(ack).unwrap();
    assert_eq!(
        store.get(&approved.plan_id).unwrap().revision,
        after.revision
    );
    lease(&mut issued, 2).unwrap();
    drop(issued);
    let unknown = store.get(&approved.plan_id).unwrap();
    assert_eq!(unknown.status, PlanStatus::Interrupted);
    assert_eq!(
        unknown
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .state,
        AttemptState::Unknown
    );
    assert!(
        issue(
            &store,
            &command,
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    drop(engine);
    drop(store);
    let reopened = JobStore::open(&path, 20, 8 * 1024 * 1024).unwrap();
    assert_eq!(
        json!(reopened.get(&approved.plan_id).unwrap()),
        json!(unknown)
    );
}

#[test]
fn normal_approval_and_execution_cannot_reinterpret_a_workload_as_v1_authority() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
    assert!(crate::request_approval::scope(&plan, 0, plan.approval.as_ref().unwrap()).is_err());
    let mut candidate = plan.clone();
    assert!(
        crate::request_execution::start(&mut candidate, &execute(&plan), eefn::protocol::now_ms())
            .is_err()
    );
    let command = execute(&plan);
    assert!(
        issue(
            &engine.store,
            &command,
            &uuid::Uuid::new_v4().to_string(),
            &Policy::default()
        )
        .is_err()
    );
    assert!(engine.store.get(&plan.plan_id).unwrap().execution.is_none());
}

#[test]
fn restored_running_job_becomes_unknown_and_never_recreates_an_issuer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.db");
    let snapshot = dir.path().join("crash-snapshot.db");
    let store = new_store(&path, 8 * 1024 * 1024);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    let command = execute(&plan);
    let issued = issue(
        &store,
        &command,
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    // Copy only this small, committed temporary SQLite file to model a process
    // exiting before token Drop. This is not a supported replica/cluster path.
    std::fs::copy(&path, &snapshot).unwrap();
    let restored = JobStore::open(&snapshot, 20, 8 * 1024 * 1024).unwrap();
    let record = restored.get(&plan.plan_id).unwrap();
    assert_eq!(record.status, PlanStatus::Interrupted);
    assert_eq!(
        record
            .execution
            .as_ref()
            .unwrap()
            .workload
            .as_ref()
            .unwrap()
            .state,
        AttemptState::Unknown
    );
    assert!(
        issue(
            &restored,
            &command,
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    drop(issued);
}

#[test]
fn retired_or_changed_issuance_cannot_renew_and_generic_edits_cannot_rebind_it() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
    let mut issued = issue(
        &engine.store,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    let before = engine.store.get(&plan.plan_id).unwrap();
    assert!(
        engine
            .store
            .change(&plan.plan_id, "try rebind session", |p| {
                p.execution
                    .as_mut()
                    .unwrap()
                    .workload
                    .as_mut()
                    .unwrap()
                    .definition
                    .session_id = uuid::Uuid::new_v4().to_string();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(
        json!(engine.store.get(&plan.plan_id).unwrap()),
        json!(before)
    );
    engine.store.interrupt(&plan.plan_id).unwrap();
    assert!(lease(&mut issued, 1).is_err());
    assert!(
        engine
            .store
            .change(&plan.plan_id, "try revive", |p| {
                p.status = PlanStatus::Running;
                p.tasks[0].status = TaskStatus::Running;
                p.execution
                    .as_mut()
                    .unwrap()
                    .workload
                    .as_mut()
                    .unwrap()
                    .state = AttemptState::Issued;
                Ok(())
            })
            .is_err()
    );
}

#[test]
fn wrong_ack_scope_or_expired_issuer_cannot_activate_renewals() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, _) = approved(&engine);
    let mut issued = issue(
        &engine.store,
        &execute(&plan),
        &uuid::Uuid::new_v4().to_string(),
        &policy(),
    )
    .unwrap();
    let mut wrong = synthetic_ack(issued.definition());
    wrong.receipt.node_id = "other".into();
    lease(&mut issued, 1).unwrap();
    assert!(issued.acknowledge(wrong).is_err());
    assert!(lease(&mut issued, 2).is_err());
    assert_eq!(
        engine.store.get(&plan.plan_id).unwrap().status,
        PlanStatus::Running,
        "A wrong peer ACK does not retire otherwise live local authority"
    );
    issued
        .acknowledge(synthetic_ack(issued.definition()))
        .unwrap();
    lease(&mut issued, 2).unwrap();
    issued.deadline = Instant::now();
    assert!(
        issued
            .acknowledge(synthetic_ack(issued.definition()))
            .is_err()
    );
    assert!(lease(&mut issued, 3).is_err());
}

#[test]
fn observed_clock_failure_permanently_retires_lease_and_ack_authority() {
    for acknowledge_entry in [false, true] {
        for consumed in [false, true] {
            let engine = TaskEngine::new(EventBus::new(20));
            let (plan, _) = approved(&engine);
            let mut issued = issue(
                &engine.store,
                &execute(&plan),
                &uuid::Uuid::new_v4().to_string(),
                &policy(),
            )
            .unwrap();
            let ack = synthetic_ack(issued.definition());
            if consumed || acknowledge_entry {
                lease(&mut issued, 1).unwrap();
            }
            if consumed {
                issued.acknowledge(ack.clone()).unwrap();
            }
            let sequence = if consumed || acknowledge_entry { 2 } else { 1 };
            let original_wall = issued.issued_wall;
            // Simulate discontinuity in the issuer's wall/monotonic correlation.
            // Never change the OS clock or depend on a sleep/host scheduling.
            issued.issued_wall = original_wall.saturating_sub(10_000);
            if acknowledge_entry {
                assert!(issued.acknowledge(ack.clone()).is_err());
            } else {
                assert!(lease(&mut issued, sequence).is_err());
            }
            issued.issued_wall = original_wall;
            if acknowledge_entry {
                assert!(
                    issued.acknowledge(ack.clone()).is_err(),
                    "Clock correction cannot restore an issuer's ACK authority"
                );
            } else {
                assert!(
                    lease(&mut issued, sequence).is_err(),
                    "Clock correction cannot restore an issuer's lease authority"
                );
            }
            assert!(lease(&mut issued, sequence).is_err());
            assert!(issued.acknowledge(ack).is_err());
            let retired = engine.store.get(&plan.plan_id).unwrap();
            assert_eq!(retired.status, PlanStatus::Interrupted);
            let workload = retired
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap();
            assert_eq!(workload.state, AttemptState::Unknown);
            assert_eq!(workload.acknowledgement.is_some(), consumed);
            drop(issued);
            assert_eq!(
                engine.store.get(&plan.plan_id).unwrap().revision,
                retired.revision,
                "Retirement attempts one uncertainty checkpoint, not repeated writes"
            );
        }
    }
}

#[test]
fn quota_failure_keeps_the_approved_plan_without_issuance_or_lease_authority() {
    let dir = tempfile::tempdir().unwrap();
    let store = new_store(&dir.path().join("jobs.db"), 65536);
    let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
    let (plan, _) = approved(&engine);
    assert!(
        issue(
            &store,
            &execute(&plan),
            &uuid::Uuid::new_v4().to_string(),
            &policy()
        )
        .is_err()
    );
    assert_eq!(json!(store.get(&plan.plan_id).unwrap()), json!(plan));
}

#[tokio::test]
async fn source_planning_cannot_checkpoint_after_engine_shutdown() {
    let engine = TaskEngine::new(EventBus::new(20));
    let (plan, input) = review(&engine);
    let command = PlanningCommand::Compile {
        id: plan.plan_id.clone(),
        expected_revision: plan.revision,
        operation_id: uuid::Uuid::new_v4().to_string(),
        input,
    };
    engine.shutdown().await;
    assert!(compile(&engine, &command).is_err());
    assert_eq!(json!(engine.store.get(&plan.plan_id).unwrap()), json!(plan));
}
