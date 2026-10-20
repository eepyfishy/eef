use super::*;

#[test]
fn early_peer_refusal_diagnostics_are_fixed_hints_not_payloads_or_readiness_proof() {
    let f = Fixture::new(4 * 1024 * 1024);
    let (grants, _) = f.prepare();
    let g = grants[1].clone();
    let denied = GrantReport::new(
        &g,
        ReceiptState::Rejected,
        serde_json::Value::Null,
        Some("local_policy_denied_or_changed"),
    );
    assert_eq!(
        early_destination_reason(&Ok(json!({"grant":denied})), &g),
        "local_policy_denied_or_changed"
    );
    let private = GrantReport::new(
        &g,
        ReceiptState::Rejected,
        serde_json::Value::Null,
        Some("PRIVATE_DIAGNOSTIC_MUST_NOT_ECHO"),
    );
    assert_eq!(
        early_destination_reason(&Ok(json!({"grant":private})), &g),
        "destination_report_unconfirmed_readiness"
    );
    assert_eq!(
        early_destination_reason(
            &Err(anyhow::anyhow!(
                "Peer generation or route changed before dispatch"
            )),
            &g
        ),
        "peer_generation_or_route_changed"
    );
    assert_eq!(
        early_destination_reason(
            &Err(anyhow::anyhow!("PRIVATE_TRANSPORT_DETAIL_MUST_NOT_ECHO")),
            &g
        ),
        "destination_transport_unconfirmed"
    );
    assert_eq!(
        early_destination_reason(
            &Ok(json!({"error_type":"PRIVATE_REPLY_MUST_NOT_ECHO","data":"secret"})),
            &g
        ),
        "destination_reply_unconfirmed"
    );
}
use crate::{
    event::EventBus,
    jobs::JobStore,
    request_planning::{PlannedBinding, PlanningRecord},
    task::{Goal, TaskEngine, TaskPlanner},
};
use eefn::{
    authentication::{AuthenticationPolicy, generate_credential},
    peer::{plan::Endpoint, scope::Limits},
};
use serde_json::Value;
use std::collections::BTreeMap;

struct Fixture {
    path: std::path::PathBuf,
    engine: Arc<TaskEngine>,
    server: eefn::NodeServer,
    plan: Plan,
    dir: tempfile::TempDir,
}

#[path = "peer_driver_tests.rs"]
mod driver_tests;
#[path = "peer_native_tests.rs"]
mod native_tests;
impl Fixture {
    fn new(max_bytes: usize) -> Self {
        Self::with_manifest(max_bytes, 100, "b".repeat(64))
    }
    fn with_manifest(max_bytes: usize, payload_bytes: u64, payload_sha256: String) -> Self {
        Self::with_content(max_bytes, payload_bytes, Some(payload_sha256), None)
    }
    fn with_content(
        max_bytes: usize,
        payload_bytes: u64,
        payload_sha256: Option<String>,
        audio: Option<eefn::peer::audio::Audio>,
    ) -> Self {
        let audio_mode = audio.is_some();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 50, max_bytes).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store);
        let identity = eefn::tls::CoordinatorIdentity::generate().unwrap();
        let issuer = identity.fingerprint().unwrap();
        let server = eefn::NodeServer::with_authentication(
            "unused-legacy",
            "127.0.0.1",
            0,
            AuthenticationPolicy {
                allow_legacy: false,
                tls: Some(identity),
                credentials: BTreeMap::from([
                    ("source".into(), Some(generate_credential())),
                    ("destination".into(), Some(generate_credential())),
                ]),
            },
        )
        .unwrap();
        let pair = Pair {
            schema_version: if audio_mode { 2 } else { 1 },
            source: Endpoint {
                node_id: "source".into(),
                resource_id: "source::transport".into(),
            },
            destination: Endpoint {
                node_id: "destination".into(),
                resource_id: "destination::transport".into(),
            },
            payload_bytes,
            payload_sha256,
            audio,
            limits: Limits {
                max_wire_bytes: 1024 * 1024,
                max_bytes_per_second: 1024 * 1024,
                max_frame_bytes: if audio_mode { 512 } else { 64 },
                max_frames: if audio_mode { 256 } else { 8 },
                max_inflight_frames: 1,
            },
        };
        let input = eefn::request_planning::PlanInput {
            schema_version: 1,
            template: "peer_transfer".into(),
            params: json!({"transfer":pair}),
            constraints: json!({}),
        };
        let concrete = eefn::request_submission::ConcreteInput {
            schema_version: 1,
            original_text: "Transfer approved bytes between the two resources".into(),
            title: "Paired transport".into(),
            input: input.clone(),
        };
        let proposal = concrete.proposal().unwrap();
        let context = eefn::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let opened = crate::request_drafts::open(
            &engine,
            json!({"schema_version":1,"original_text":concrete.original_text,"proposal":proposal}),
            &context,
            &if audio_mode {
                vec![
                    "peer.transfer".into(),
                    "audio.capture".into(),
                    "audio.play".into(),
                ]
            } else {
                vec!["peer.transfer".into()]
            },
        )
        .unwrap();
        let id = opened["id"].as_str().unwrap();
        let saved = engine.store.get(id).unwrap();
        let mut compiled = TaskPlanner
            .plan(Goal {
                request_context: Some(context),
                description: saved.goal.description.clone(),
                template: input.template.clone(),
                params: input.params.clone(),
                constraints: input.constraints.clone(),
            })
            .unwrap();
        compiled.plan_id = saved.plan_id.clone();
        compiled.request_review = saved.request_review.clone();
        compiled.revision = saved.revision + 1;
        compiled.status = PlanStatus::AwaitingApproval;
        for t in &mut compiled.tasks {
            t.retry_count = 0;
            t.retry_delay_ms = 0;
        }
        let mut record = PlanningRecord {
            schema_version: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
            source_revision: saved.revision,
            proposal_index: 0,
            compiled_at_ms: eefn::protocol::now_ms(),
            input,
            preparation: None,
            plan_sha256: String::new(),
            bindings: compiled
                .tasks
                .iter()
                .map(|t| PlannedBinding {
                    task_id: t.task_id.clone(),
                    node_id: t.constraints["node_id"].as_str().unwrap().into(),
                    resource_id: Some(t.constraints["resource_id"].as_str().unwrap().into()),
                    model_id: None,
                    backend: None,
                })
                .collect(),
        };
        record.plan_sha256 = crate::request_planning::digest(&compiled, &record).unwrap();
        compiled.planning = Some(record);
        crate::jobs::validate_retained_review(&compiled).unwrap();
        let p = engine
            .store
            .change(id, "Fixture exact paired compilation", |p| {
                p.goal = compiled.goal;
                p.tasks = compiled.tasks;
                p.planning = compiled.planning;
                p.status = compiled.status;
                Ok(())
            })
            .unwrap();
        let mut command = crate::request_approval::tests::command(&p);
        if let crate::request_planning::PlanningCommand::Approve { limits, .. } = &mut command {
            limits.max_execution_ms = 10000;
            limits.max_result_bytes = 4096;
        }
        let mut approval = crate::request_approval::tests::record(&command);
        approval.issuer_id = issuer;
        approval.approval_sha256 = crate::request_approval::digest(&approval).unwrap();
        let p = crate::request_approval::tests::save(&engine, &p, &command, approval).unwrap();
        let execute = eefn::request_execution::ExecuteCommand {
            id: p.plan_id.clone(),
            expected_revision: p.revision,
            operation_id: uuid::Uuid::new_v4().to_string(),
            approval_sha256: p.approval.as_ref().unwrap().approval_sha256.clone(),
        };
        let plan = engine
            .store
            .change(id, "Fixture explicit execution request", |p| {
                crate::request_execution::start(p, &execute, eefn::protocol::now_ms())
            })
            .unwrap();
        Self {
            dir,
            path,
            engine,
            server,
            plan,
        }
    }
    fn prepare(&self) -> ([ExecutionGrant; 2], Issuance) {
        let now = eefn::protocol::now_ms();
        let grants = [0, 1].map(|i| {
            let mut g =
                crate::request_approval::scope(&self.plan, i, self.plan.approval.as_ref().unwrap())
                    .unwrap();
            g.grant_id = uuid::Uuid::new_v4().to_string();
            g.session_id = uuid::Uuid::new_v4().to_string();
            g.issued_at_ms = now;
            g.expires_at_ms = (now + g.limits.max_execution_ms)
                .min(self.plan.approval.as_ref().unwrap().expires_at_ms);
            g
        });
        let generations = [
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        ];
        let issuance = Issuance::prepare(
            &self.plan,
            &grants,
            &uuid::Uuid::new_v4().to_string(),
            [&generations[0], &generations[1]],
            "127.0.0.1:12345".parse().unwrap(),
            &self.server,
        )
        .unwrap();
        (grants, issuance)
    }
    fn save(&self, grants: [ExecutionGrant; 2], issuance: Issuance) -> Result<Plan> {
        self.engine.store.change(
            &self.plan.plan_id,
            "Both peer grants before dispatch",
            |p| issue(p, grants, issuance),
        )
    }
    fn observer(&self, g: &ExecutionGrant) -> crate::request_execution::AttemptJournal {
        crate::request_execution::AttemptJournal {
            store: self.engine.store.clone(),
            job_id: self.plan.plan_id.clone(),
            grant_id: g.grant_id.clone(),
        }
    }
    fn report(g: &ExecutionGrant, r: &Issuance, i: usize) -> GrantReport {
        let (_, d) = eefn::peer::admission::Definition::from_grant(g).unwrap();
        GrantReport::new(
            g,
            ReceiptState::Completed,
            json!(eefn::peer::admission::TransferReceipt {
                schema_version: 1,
                pair_session_id: r.scope.session_id.clone(),
                scope_sha256: r.scope.digest().unwrap(),
                role: if i == 0 {
                    Role::Source
                } else {
                    Role::Destination
                },
                payload_bytes: d.payload_bytes,
                payload_sha256: d.payload_sha256.unwrap(),
                data_frames: 2,
                payload_retained: false,
                sink_effects_confirmed: false,
                producer_reported: false,
                sink_reported: false
            }),
            None,
        )
    }
}

#[test]
fn exact_pair_definition_compiles_two_fixed_nonsequential_tasks_without_scheduling_authority() {
    let f = Fixture::new(2 * 1024 * 1024);
    assert_eq!(f.plan.tasks.len(), 2);
    assert!(
        f.plan
            .tasks
            .iter()
            .all(|t| t.dependencies.is_empty() && t.retry_count == 0 && t.attempts == 0)
    );
    assert!(f.plan.execution.as_ref().unwrap().peer.is_none());
    assert!(f.plan.execution.as_ref().unwrap().attempts.is_empty());
    for field in ["destination_address", "approved", "material"] {
        let mut input = f.plan.planning.as_ref().unwrap().input.clone();
        input.params["transfer"][field] = json!(true);
        assert!(input.validate().is_err());
    }
    let mut changed = f.plan.clone();
    changed.tasks[1].constraints["node_id"] = json!("other");
    assert!(crate::jobs::validate_retained_review(&changed).is_err());
    let mut changed = f.plan.clone();
    let predecessor = changed.tasks[0].task_id.clone();
    changed.tasks[1].dependencies.push(predecessor);
    assert!(crate::jobs::validate_retained_review(&changed).is_err());
}

#[test]
fn both_issuances_are_atomic_and_one_sided_or_corrupt_delivery_never_replaces_history() {
    let f = Fixture::new(2 * 1024 * 1024);
    let (grants, record) = f.prepare();
    let before = json!(f.engine.store.get(&f.plan.plan_id).unwrap());
    let mut changed = record.clone();
    changed.material.destination.signature.clear();
    assert!(f.save(grants.clone(), changed).is_err());
    assert_eq!(json!(f.engine.store.get(&f.plan.plan_id).unwrap()), before);
    let issued = f.save(grants.clone(), record.clone()).unwrap();
    assert_eq!(issued.execution.as_ref().unwrap().attempts.len(), 2);
    assert!(
        issued
            .tasks
            .iter()
            .all(|t| t.status == TaskStatus::Running && t.attempts == 1)
    );
    assert!(f.save(grants.clone(), record).is_err());
    for fault in [
        "partial",
        "missing",
        "scope",
        "ciphertext",
        "terminal_parent",
    ] {
        let mut changed = issued.clone();
        let e = changed.execution.as_mut().unwrap();
        match fault {
            "partial" => {
                e.attempts.pop();
            }
            "missing" => e.peer = None,
            "scope" => e.peer.as_mut().unwrap().scope.source.grant_sha256 = "c".repeat(64),
            "ciphertext" => e.peer.as_mut().unwrap().material.source.token.push('x'),
            _ => changed.status = PlanStatus::Completed,
        }
        assert!(
            crate::jobs::validate_retained_review(&changed).is_err(),
            "{fault}"
        );
    }
    assert_eq!(
        json!(f.engine.store.get(&f.plan.plan_id).unwrap()),
        json!(issued)
    );
}

#[test]
fn failed_pair_checkpoint_keeps_both_tasks_unissued_without_partial_grants() {
    let f = Fixture::new(30000);
    let (g, r) = f.prepare();
    let before = json!(f.engine.store.get(&f.plan.plan_id).unwrap());
    assert!(f.save(g, r).is_err());
    assert_eq!(json!(f.engine.store.get(&f.plan.plan_id).unwrap()), before);
    assert!(
        f.engine
            .store
            .get(&f.plan.plan_id)
            .unwrap()
            .execution
            .unwrap()
            .attempts
            .is_empty()
    );
}

#[test]
fn parent_requires_both_matching_results_and_cross_session_reports_leave_no_checkpoint() {
    let f = Fixture::new(2 * 1024 * 1024);
    let (g, r) = f.prepare();
    f.save(g.clone(), r.clone()).unwrap();
    for grant in &g {
        f.observer(grant)
            .record(&GrantReport::new(
                grant,
                ReceiptState::Accepted,
                Value::Null,
                None,
            ))
            .unwrap();
    }
    let mut wrong = Fixture::report(&g[0], &r, 0);
    wrong.data["pair_session_id"] = json!(uuid::Uuid::new_v4().to_string());
    let before = json!(f.engine.store.get(&f.plan.plan_id).unwrap());
    assert!(
        wrong.confirms(&g[0]),
        "individual counters alone cannot bind a pair"
    );
    assert!(f.observer(&g[0]).record(&wrong).is_err());
    assert_eq!(json!(f.engine.store.get(&f.plan.plan_id).unwrap()), before);
    f.observer(&g[0])
        .record(&Fixture::report(&g[0], &r, 0))
        .unwrap();
    let p = f.engine.store.get(&f.plan.plan_id).unwrap();
    assert_eq!(p.status, PlanStatus::Running);
    assert_eq!(p.tasks[0].status, TaskStatus::Completed);
    assert_eq!(p.tasks[1].status, TaskStatus::Running);
    f.observer(&g[1])
        .record(&Fixture::report(&g[1], &r, 1))
        .unwrap();
    assert_eq!(
        f.engine.store.get(&f.plan.plan_id).unwrap().status,
        PlanStatus::Completed
    );
}

#[test]
fn rejection_and_uncertainty_wait_for_the_counterpart_and_never_infer_success() {
    let f = Fixture::new(2 * 1024 * 1024);
    let (g, r) = f.prepare();
    f.save(g.clone(), r).unwrap();
    f.observer(&g[0])
        .record(&GrantReport::new(
            &g[0],
            ReceiptState::Rejected,
            Value::Null,
            Some("local_policy_denied"),
        ))
        .unwrap();
    assert_eq!(
        f.engine.store.get(&f.plan.plan_id).unwrap().status,
        PlanStatus::Running
    );
    f.observer(&g[1])
        .record(&GrantReport::new(
            &g[1],
            ReceiptState::Accepted,
            Value::Null,
            None,
        ))
        .unwrap();
    f.observer(&g[1])
        .record(&GrantReport::new(
            &g[1],
            ReceiptState::Unknown,
            Value::Null,
            Some("connection_lost"),
        ))
        .unwrap();
    let p = f.engine.store.get(&f.plan.plan_id).unwrap();
    assert_eq!(p.status, PlanStatus::Interrupted);
    assert_eq!(p.tasks[0].status, TaskStatus::Failed);
    assert_eq!(p.tasks[1].status, TaskStatus::Interrupted);
    assert_eq!(crate::jobs::view(&p, true)["can_abandon_tracking"], false);
}

#[test]
fn restart_preserves_both_exact_grants_as_uncertain_without_renewal_or_replay() {
    let f = Fixture::new(2 * 1024 * 1024);
    let (g, r) = f.prepare();
    f.save(g.clone(), r.clone()).unwrap();
    f.observer(&g[1])
        .record(&GrantReport::new(
            &g[1],
            ReceiptState::Accepted,
            Value::Null,
            None,
        ))
        .unwrap();
    let Fixture {
        dir,
        path,
        engine,
        server,
        plan,
    } = f;
    drop(engine);
    drop(server);
    let reopened = JobStore::open(&path, 50, 2 * 1024 * 1024).unwrap();
    let p = reopened.get(&plan.plan_id).unwrap();
    assert_eq!(p.status, PlanStatus::Interrupted);
    assert!(
        p.execution
            .as_ref()
            .unwrap()
            .attempts
            .iter()
            .all(|a| a.state == AttemptState::Unknown)
    );
    assert_eq!(json!(p.execution.as_ref().unwrap().peer), json!(r));
    let before = json!(p);
    assert!(
        reopened
            .change(&plan.plan_id, "No repeated pair", |p| issue(p, g, r))
            .is_err()
    );
    assert_eq!(json!(reopened.get(&plan.plan_id).unwrap()), before);
    assert_eq!(crate::jobs::view(&p, true)["can_resume"], false);
    drop(reopened);
    drop(dir);
}

#[test]
fn audio_review_declares_both_media_operations_and_mismatched_results_are_not_joint_success() {
    let audio = eefn::peer::audio::Audio {
        duration_ms: 500,
        sample_rate: 16000,
        channels: 1,
        source_resource_id: "source::microphone".into(),
        destination_resource_id: "destination::speaker".into(),
    };
    let f = Fixture::with_content(2 * 1024 * 1024, audio.maximum_bytes(), None, Some(audio));
    let caps = &f.plan.request_review.as_ref().unwrap().proposals[0]
        .proposal
        .suggested_capabilities;
    assert!(
        caps.contains(&"peer.transfer".into())
            && caps.contains(&"audio.capture".into())
            && caps.contains(&"audio.play".into())
    );
    let (grants, issued) = f.prepare();
    f.save(grants.clone(), issued.clone()).unwrap();
    for g in &grants {
        f.observer(g)
            .record(&GrantReport::new(
                g,
                ReceiptState::Accepted,
                Value::Null,
                None,
            ))
            .unwrap();
    }
    let report = |i: usize, hash: &str| {
        GrantReport::new(
            &grants[i],
            ReceiptState::Completed,
            json!(eefn::peer::admission::TransferReceipt {
                schema_version: 2,
                pair_session_id: issued.scope.session_id.clone(),
                scope_sha256: issued.scope.digest().unwrap(),
                role: if i == 0 {
                    Role::Source
                } else {
                    Role::Destination
                },
                payload_bytes: 16044,
                payload_sha256: hash.repeat(64),
                data_frames: 1 + 16044u64.div_ceil(512) as u32,
                payload_retained: false,
                sink_effects_confirmed: false,
                producer_reported: i == 0,
                sink_reported: i == 1
            }),
            None,
        )
    };
    let source = report(0, "a");
    assert!(source.confirms(&grants[0]));
    f.observer(&grants[0]).record(&source).unwrap();
    let before = json!(f.engine.store.get(&f.plan.plan_id).unwrap());
    let mismatch = report(1, "b");
    assert!(mismatch.confirms(&grants[1]));
    assert!(f.observer(&grants[1]).record(&mismatch).is_err());
    assert_eq!(json!(f.engine.store.get(&f.plan.plan_id).unwrap()), before);
    f.observer(&grants[1]).record(&report(1, "a")).unwrap();
    assert_eq!(
        f.engine.store.get(&f.plan.plan_id).unwrap().status,
        PlanStatus::Completed
    );
}
