use super::*;
use crate::{event::EventBus, task::TaskEngine};
use eefn::{
    context::RequestContext,
    interpreter_policy::{Authorization, Command, Limits, Scope},
    interpreter_task::Snapshot,
};

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn policy(store: &JobStore, runtime: &str, limits: Limits) -> PolicyPin {
    let generation = interpreter_policies::records(&store.connection.lock().unwrap())
        .unwrap()
        .len() as u64;
    let value = store
        .interpreter_policy_command(
            Command::Approve {
                operation_id: uuid(),
                generation,
                scope: Scope {
                    origin_nodes: vec!["input".into(), "other-input".into()],
                    provider_nodes: vec!["provider".into(), "other-provider".into()],
                },
                limits,
                valid_for_ms: None,
            },
            runtime,
            Some(&"a".repeat(64)),
            eefn::protocol::now_ms(),
        )
        .unwrap();
    PolicyPin::from(
        &serde_json::from_value::<Authorization>(value["authorization"].clone()).unwrap(),
    )
}
fn limits() -> Limits {
    Limits {
        max_output_tokens: 128,
        max_execution_ms: 30_000,
        max_result_bytes: 4096,
        ..Default::default()
    }
}
fn permit() -> Permit {
    Permit {
        origin_node: "input".into(),
        origin_runtime: uuid(),
        origin_session: uuid(),
        provider_node: "provider".into(),
        provider_runtime: uuid(),
        provider_session: uuid(),
        issuer: "a".repeat(64),
        backend: "ollama".into(),
        model_id: "chosen".into(),
        permission_revision: 2,
        limits: Default::default(),
        capabilities: vec!["system.info".into()],
        fresh_until: Instant::now() + Duration::from_secs(300),
    }
}
fn input(store: &Arc<JobStore>, origin: &str) -> Task {
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    let context = RequestContext::new(uuid(), origin.into(), vec!["home".into()]).unwrap();
    let saved = crate::request_intent::save(
        &engine,
        json!({"schema_version":1,"original_text":"Check this node"}),
        &context,
    )
    .unwrap();
    Task::new(
        Snapshot {
            schema_version: 1,
            job_id: saved["id"].as_str().unwrap().into(),
            revision: saved["revision"].as_u64().unwrap(),
            operation_id: uuid(),
            request_context: context,
            allowed_capabilities: vec!["system.info".into()],
            source: Source::Initial {
                input: serde_json::from_value(saved["input"].clone()).unwrap(),
            },
        },
        128,
    )
    .unwrap()
}
fn reserve_new(
    store: &JobStore,
    task: Task,
    pin: &PolicyPin,
    permit: &Permit,
    runtime: &str,
) -> Reserved {
    match store
        .reserve_interpreter_allowance(task, pin, permit, runtime)
        .unwrap()
    {
        Reservation::New(r) => r,
        Reservation::Existing(_) => panic!("Expected a new continuation"),
    }
}
fn used(store: &JobStore) -> usize {
    let u = store.usage().unwrap();
    u["max_bytes"].as_u64().unwrap() as usize - u["available_bytes"].as_u64().unwrap() as usize
}

#[test]
fn reservation_and_input_claim_are_atomic_and_reuse_never_returns_a_continuation() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let reserved = reserve_new(&store, task.clone(), &pin, &p, &runtime);
    let claimed = store.get(&task.snapshot.job_id).unwrap();
    assert_eq!(claimed.revision, task.snapshot.revision + 1);
    assert_eq!(
        claimed.input_interpretation.as_ref().unwrap().claim,
        reserved.initial_claim().unwrap().unwrap()
    );
    assert!(claimed.tasks.is_empty() && claimed.approval.is_none() && claimed.execution.is_none());
    let before = store.usage().unwrap();
    let old = store
        .reserve_interpreter_allowance(task.clone(), &pin, &p, &uuid())
        .unwrap();
    assert!(matches!(old,Reservation::Existing(r) if r.id()==reserved.id()));
    assert_eq!(store.usage().unwrap(), before);
    let mut changed = task.clone();
    changed.max_output_tokens = 64;
    changed = Task::new(changed.snapshot, 64).unwrap();
    assert!(
        store
            .reserve_interpreter_allowance(changed, &pin, &p, &runtime)
            .is_err()
    );
    let record = store.interpreter_not_sent(reserved).unwrap();
    assert_eq!(record.phase, Phase::NotSent);
    assert!(!record.holds_slot());
    assert_eq!(record.checkpoint_reserve().unwrap(), 0);
    let plan = store.get(&task.snapshot.job_id).unwrap();
    assert_eq!(
        plan.input_interpretation.unwrap().state,
        eefn::request_interpretation::State::Unknown
    );
    let (_, accounts) = store.interpreter_allowance_records().unwrap();
    assert_eq!(
        accounts[&pin.id].charged.calls, 1,
        "Known unsent retirement must not refund aggregate approval"
    );
    assert!(matches!(
        store
            .reserve_interpreter_allowance(task, &pin, &p, &runtime)
            .unwrap(),
        Reservation::Existing(_)
    ));
}

#[test]
fn every_aggregate_limit_refuses_before_claim_or_charge_and_removal_does_not_refund() {
    for bound in ["calls", "tokens", "time", "result"] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let mut l = limits();
        match bound {
            "calls" => l.max_calls = 1,
            "tokens" => l.max_reserved_output_tokens = 128,
            "time" => l.max_reserved_execution_ms = 30_000,
            _ => l.max_reserved_result_bytes = 4096,
        }
        let pin = policy(&store, &runtime, l);
        let p = permit();
        let first = input(&store, "input");
        let reserved = reserve_new(&store, first.clone(), &pin, &p, &runtime);
        store.interpreter_not_sent(reserved).unwrap();
        store
            .change(&first.snapshot.job_id, "Owner cancelled input", |p| {
                p.status = PlanStatus::Cancelled;
                Ok(())
            })
            .unwrap();
        store.remove_finished(&first.snapshot.job_id).unwrap();
        let next = input(&store, "input");
        let before = store.usage().unwrap();
        let error = store
            .reserve_interpreter_allowance(next.clone(), &pin, &p, &runtime)
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains("aggregate allowance exhausted"),
            "{bound}: {error}"
        );
        assert!(
            store
                .get(&next.snapshot.job_id)
                .unwrap()
                .input_interpretation
                .is_none()
        );
        assert_eq!(store.usage().unwrap(), before);
        assert_eq!(
            store.interpreter_allowance_records().unwrap().1[&pin.id]
                .charged
                .calls,
            1
        );
    }
}

#[test]
fn concurrency_is_global_origin_and_provider_scoped_across_policy_generations() {
    for bound in ["global", "origin", "provider"] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let mut l = limits();
        l.max_in_flight = if bound == "global" { 1 } else { 2 };
        let pin = policy(&store, &runtime, l.clone());
        let p = permit();
        let first = input(&store, "input");
        let _reserved = reserve_new(&store, first, &pin, &p, &runtime);
        // New approval must not forget uncertain work admitted by a prior generation.
        let pin = policy(&store, &runtime, l);
        let mut p2 = permit();
        if bound != "origin" {
            p2.origin_node = "other-input".into()
        }
        if bound != "provider" {
            p2.provider_node = "other-provider".into()
        }
        let next = input(&store, &p2.origin_node);
        let before = store.usage().unwrap();
        assert!(
            store
                .reserve_interpreter_allowance(next.clone(), &pin, &p2, &runtime)
                .err()
                .unwrap()
                .to_string()
                .contains("concurrency allowance"),
            "{bound}"
        );
        assert_eq!(store.usage().unwrap(), before);
        assert!(
            store
                .get(&next.snapshot.job_id)
                .unwrap()
                .input_interpretation
                .is_none()
        );
    }
}

#[test]
fn simultaneous_reservations_have_one_durable_owner() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let one = input(&store, "input");
    let two = input(&store, "input");
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = vec![];
    for task in [one, two] {
        let (store, barrier, runtime, pin) =
            (store.clone(), barrier.clone(), runtime.clone(), pin.clone());
        workers.push(std::thread::spawn(move || {
            let p = permit();
            barrier.wait();
            store
                .reserve_interpreter_allowance(task, &pin, &p, &runtime)
                .is_ok()
        }));
    }
    barrier.wait();
    assert_eq!(
        workers
            .into_iter()
            .map(|w| usize::from(w.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let (records, accounts) = store.interpreter_allowance_records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(accounts[&pin.id].charged.calls, 1);
    assert_eq!(
        store
            .list()
            .unwrap()
            .iter()
            .filter(|p| p.input_interpretation.is_some())
            .count(),
        1
    );
}

#[test]
fn changed_source_disclosure_policy_or_participants_cannot_gain_delivery_authority() {
    for edit in [
        "source",
        "disclosure",
        "session",
        "epoch",
        "model",
        "limits",
        "freshness",
        "runtime",
        "policy",
        "deadline",
        "stopped",
    ] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let pin = policy(&store, &runtime, limits());
        let mut p = permit();
        let task = input(&store, "input");
        if edit == "source" || edit == "disclosure" {
            let mut changed = task.snapshot.clone();
            if edit == "source" {
                changed.revision += 1
            } else {
                changed.allowed_capabilities.clear()
            }
            let before = store.usage().unwrap();
            assert!(
                store
                    .reserve_interpreter_allowance(
                        Task::new(changed, 128).unwrap(),
                        &pin,
                        &p,
                        &runtime
                    )
                    .is_err()
            );
            assert_eq!(store.usage().unwrap(), before);
            continue;
        }
        let mut reserved = reserve_new(&store, task.clone(), &pin, &p, &runtime);
        let mut current_runtime = runtime.clone();
        match edit {
            "session" => p.provider_session = uuid(),
            "epoch" => p.permission_revision += 1,
            "model" => p.model_id = "replacement".into(),
            "limits" => p.limits.max_result_bytes = 1024,
            "freshness" => p.fresh_until = Instant::now(),
            "runtime" => current_runtime = uuid(),
            "policy" => {
                policy(&store, &runtime, limits());
            }
            "deadline" => reserved.deadline = Instant::now(),
            "stopped" => {
                store
                    .change(&task.snapshot.job_id, "Owner stopped input", |p| {
                        p.status = PlanStatus::Cancelled;
                        Ok(())
                    })
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = store.usage().unwrap();
        assert!(
            store
                .interpreter_delivery_intent(&reserved, &p, &current_runtime, &pin)
                .is_err(),
            "{edit}"
        );
        assert_eq!(store.usage().unwrap(), before);
        store.interpreter_not_sent(reserved).unwrap();
    }
}

#[test]
fn delivery_and_receipts_keep_original_deadline_exact_binding_and_reserved_checkpoint_space() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let reserved = reserve_new(&store, task, &pin, &p, &runtime);
    let deadline = reserved.deadline;
    let before = used(&store);
    let premature = GrantReport::new(
        &reserved.record.binding.grant,
        ReceiptState::Accepted,
        json!(null),
        None,
    );
    assert!(store.interpreter_report(reserved.id(), premature).is_err());
    let dispatch = store
        .interpreter_delivery_intent(&reserved, &p, &runtime, &pin)
        .unwrap();
    assert_eq!(dispatch.deadline(), deadline);
    assert_eq!(dispatch.grant().grant_id, reserved.id());
    assert!(
        store
            .interpreter_delivery_intent(&reserved, &p, &runtime, &pin)
            .is_err()
    );
    let unaccepted = GrantReport::new(
        dispatch.grant(),
        ReceiptState::Completed,
        json!(null),
        Some("result_too_large"),
    );
    assert!(
        store.interpreter_report(reserved.id(), unaccepted).is_err(),
        "Completion needs the exact saved acceptance"
    );
    let accepted = GrantReport::new(dispatch.grant(), ReceiptState::Accepted, json!(null), None);
    let saved = store
        .interpreter_report(reserved.id(), accepted.clone())
        .unwrap();
    assert_eq!(saved.phase, Phase::Accepted);
    assert_eq!(used(&store), before);
    assert_eq!(
        store
            .interpreter_report(reserved.id(), accepted)
            .unwrap()
            .revision,
        saved.revision
    );
    let rejected = GrantReport::new(
        dispatch.grant(),
        ReceiptState::Rejected,
        json!(null),
        Some("not_accepted"),
    );
    assert!(store.interpreter_report(reserved.id(), rejected).is_err());
    let mut forged = GrantReport::new(
        dispatch.grant(),
        ReceiptState::Completed,
        json!(null),
        Some("result_too_large"),
    );
    forged.session_id = uuid();
    assert!(store.interpreter_report(reserved.id(), forged).is_err());
    let done = GrantReport::new(
        dispatch.grant(),
        ReceiptState::Completed,
        json!(null),
        Some("result_too_large"),
    );
    let final_record = store
        .interpreter_report(reserved.id(), done.clone())
        .unwrap();
    assert_eq!(final_record.phase, Phase::Final);
    assert!(!final_record.holds_slot());
    assert!(used(&store) < before);
    assert_eq!(
        store
            .interpreter_report(reserved.id(), done)
            .unwrap()
            .revision,
        final_record.revision
    );
    assert!(store.interpreter_not_sent(reserved).is_err());
    assert_eq!(
        store.interpreter_allowance_records().unwrap().1[&pin.id]
            .charged
            .calls,
        1
    );
}

#[test]
fn unknown_completion_retains_concurrency_and_never_claims_shared_inference_stopped() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let r = reserve_new(&store, task, &pin, &p, &runtime);
    let d = store
        .interpreter_delivery_intent(&r, &p, &runtime, &pin)
        .unwrap();
    let unknown = GrantReport::new(
        d.grant(),
        ReceiptState::Unknown,
        json!(null),
        Some("completion_unconfirmed"),
    );
    let record = store.interpreter_report(r.id(), unknown).unwrap();
    assert_eq!(record.phase, Phase::Final);
    assert!(record.holds_slot());
    assert!(
        record.checkpoint_reserve().unwrap() > 0,
        "Uncertain recovery must retain its original checkpoint space"
    );
    let next = input(&store, "input");
    assert!(
        store
            .reserve_interpreter_allowance(next, &pin, &p, &runtime)
            .is_err()
    );
}

#[test]
fn reopen_never_reconstructs_a_continuation_and_recovery_works_below_quota() {
    for phase in ["reserved", "delivery", "accepted", "accepted-stopped"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 100, 1024 * 1024).unwrap();
        let runtime = uuid();
        let pin = policy(&store, &runtime, limits());
        let p = permit();
        let task = input(&store, "input");
        let r = reserve_new(&store, task.clone(), &pin, &p, &runtime);
        if phase != "reserved" {
            let d = store
                .interpreter_delivery_intent(&r, &p, &runtime, &pin)
                .unwrap();
            if phase == "accepted" || phase == "accepted-stopped" {
                store
                    .interpreter_report(
                        r.id(),
                        GrantReport::new(d.grant(), ReceiptState::Accepted, json!(null), None),
                    )
                    .unwrap();
            }
            if phase == "accepted-stopped" {
                store
                    .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
                    .unwrap()
                    .unwrap();
            }
        }
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(store.usage().unwrap()["over_limit"], true);
        let (records, accounts) = store.interpreter_allowance_records().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(accounts[&pin.id].charged.calls, 1);
        assert_eq!(records[0].stop.is_some(), phase == "accepted-stopped");
        assert_eq!(
            records[0].phase,
            if phase == "reserved" {
                Phase::NotSent
            } else {
                Phase::Interrupted
            }
        );
        assert_eq!(
            store
                .get(&task.snapshot.job_id)
                .unwrap()
                .input_interpretation
                .unwrap()
                .state,
            eefn::request_interpretation::State::Unknown
        );
        assert!(matches!(
            store
                .reserve_interpreter_allowance(task, &pin, &p, &uuid())
                .unwrap(),
            Reservation::Existing(_)
        ));
        assert!(
            store
                .interpreter_delivery_intent(&r, &p, &runtime, &pin)
                .is_err()
        );
        if phase != "reserved" {
            assert!(
                store
                    .interpreter_report(
                        r.id(),
                        GrantReport::new(
                            &r.record.binding.grant,
                            ReceiptState::Completed,
                            json!(null),
                            Some("result_too_large"),
                        ),
                    )
                    .is_err(),
                "Interrupted history needs separately authenticated explicit recovery, not a live observer"
            );
        }
        drop(store);
        assert!(JobStore::open(&path, 1, 1024).is_ok());
    }
}

#[test]
fn exact_count_and_byte_quota_admission_is_all_or_nothing() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let before = used(&store);
    let _r = reserve_new(&store, task, &pin, &p, &runtime);
    let extra = used(&store) - before;
    for bound in ["bytes", "count"] {
        for fits in [false, true] {
            let mut store = JobStore::in_memory().unwrap();
            let runtime = uuid();
            let pin = policy(&store, &runtime, limits());
            let p = permit();
            let task = input(&store, "input");
            let exact = used(&store) + extra;
            let mutable = Arc::get_mut(&mut store)
                .expect("No background or engine owns this synchronous fixture");
            if bound == "bytes" {
                mutable.max_bytes = exact - usize::from(!fits)
            } else {
                mutable.max_count = 4 - usize::from(!fits)
            }
            let before = store.usage().unwrap();
            let result = store.reserve_interpreter_allowance(task.clone(), &pin, &p, &runtime);
            assert_eq!(result.is_ok(), fits, "{bound}, fits={fits}");
            if fits {
                let usage = store.usage().unwrap();
                assert_eq!(usage["interpreter_allowance_attempt_count"], 1);
                assert_eq!(usage["interpreter_allowance_account_count"], 1);
                if bound == "bytes" {
                    assert_eq!(usage["available_bytes"], 0)
                }
            } else {
                assert_eq!(store.usage().unwrap(), before);
                assert!(
                    store
                        .get(&task.snapshot.job_id)
                        .unwrap()
                        .input_interpretation
                        .is_none()
                )
            }
        }
    }
}

#[test]
fn damaged_history_accounts_columns_or_escrow_fail_closed_without_reset() {
    for corruption in [
        "record",
        "binding",
        "sequence",
        "escrow",
        "delete_attempt",
        "delete_account",
        "account",
    ] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let pin = policy(&store, &runtime, limits());
        let p = permit();
        let task = input(&store, "input");
        let _r = reserve_new(&store, task, &pin, &p, &runtime);
        let connection = store.connection.lock().unwrap();
        match corruption {
            "record"=>connection.execute("UPDATE interpreter_allowance_attempts SET record_json='{}'",[]),
            "binding"=>connection.execute("UPDATE interpreter_allowance_attempts SET record_json=json_set(record_json,'$.binding.origin_runtime_id',?1)",[uuid()]),
            "sequence"=>connection.execute("UPDATE interpreter_allowance_attempts SET sequence=2",[]),
            "escrow"=>connection.execute("UPDATE interpreter_allowance_attempts SET reserved_bytes=0",[]),
            "delete_attempt"=>connection.execute("DELETE FROM interpreter_allowance_attempts",[]),
            "delete_account"=>connection.execute("DELETE FROM interpreter_allowance_accounts",[]),
            _=>connection.execute("UPDATE interpreter_allowance_accounts SET record_json=json_set(record_json,'$.charged.calls',0)",[]),
        }.unwrap();
        let snapshot_before=connection.query_row("SELECT (SELECT count(*) FROM interpreter_allowance_attempts),(SELECT count(*) FROM interpreter_allowance_accounts)",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap();
        drop(connection);
        assert!(
            store.interpreter_allowance_records().is_err(),
            "{corruption}"
        );
        assert!(store.validate_automations().is_err());
        let connection = store.connection.lock().unwrap();
        let snapshot_after=connection.query_row("SELECT (SELECT count(*) FROM interpreter_allowance_attempts),(SELECT count(*) FROM interpreter_allowance_accounts)",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap();
        assert_eq!(snapshot_before, snapshot_after);
    }
}

#[test]
fn all_three_stages_reserve_only_exact_retained_sources_without_review_promotion() {
    for name in ["initial", "clarification", "method"] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let pin = policy(&store, &runtime, limits());
        let p = permit();
        let task = if name == "initial" {
            input(&store, "input")
        } else {
            let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
            let context = RequestContext::new(uuid(), "input".into(), vec!["home".into()]).unwrap();
            let question = name == "clarification";
            let opened=crate::request_drafts::open(&engine,json!({"schema_version":1,"original_text":"Inspect node status","proposal":{
                "schema_version":1,"intent":"information","goal":"Inspect node status","context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot","needs_clarification":question,"clarification":if question{json!("Which node?")}else{json!(null)}
            }}),&context,&p.capabilities).unwrap();
            let saved = if question {
                let answer_context =
                    RequestContext::new(uuid(), "input".into(), vec!["home".into()]).unwrap();
                crate::request_drafts::command(&engine,"requests.draft_answer",json!({"id":opened["id"],"expected_revision":opened["revision"],"text":"This node"}),&answer_context,&p.capabilities).unwrap()
            } else {
                opened
            };
            let draft = Box::new(serde_json::from_value(saved["draft"].clone()).unwrap());
            Task::new(
                Snapshot {
                    schema_version: 1,
                    job_id: saved["id"].as_str().unwrap().into(),
                    revision: saved["revision"].as_u64().unwrap(),
                    operation_id: uuid(),
                    request_context: context,
                    allowed_capabilities: p.capabilities.clone(),
                    source: if question {
                        Source::Clarification { draft }
                    } else {
                        Source::Method { draft }
                    },
                },
                128,
            )
            .unwrap()
        };
        let r = reserve_new(&store, task.clone(), &pin, &p, &runtime);
        assert_eq!(r.record.stage().unwrap(), name);
        let d = store
            .interpreter_delivery_intent(&r, &p, &runtime, &pin)
            .unwrap();
        assert_eq!(d.grant().request_context, task.snapshot.request_context);
        assert_eq!(d.grant().params["interpreter_task"], json!(task));
        let plan = store.get(&task.snapshot.job_id).unwrap();
        assert!(
            plan.tasks.is_empty()
                && plan.approval.is_none()
                && plan.planning.is_none()
                && plan.execution.is_none()
        );
        assert_eq!(plan.input_interpretation.is_some(), name == "initial");
    }
}

#[test]
fn operation_receipt_and_per_call_bounds_cannot_change_or_partially_claim_another_source() {
    for bound in ["operation", "task_bytes", "tokens"] {
        let store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let mut l = limits();
        if bound == "task_bytes" {
            l.max_task_bytes = 1
        } else if bound == "tokens" {
            l.max_output_tokens = 64
        }
        let pin = policy(&store, &runtime, l);
        let p = permit();
        let mut task = input(&store, "input");
        if bound == "operation" {
            let first = reserve_new(&store, task.clone(), &pin, &p, &runtime);
            store.interpreter_not_sent(first).unwrap();
            let operation = task.snapshot.operation_id.clone();
            let mut snapshot = input(&store, "input").snapshot;
            snapshot.operation_id = operation;
            task = Task::new(snapshot, 128).unwrap();
        }
        let before = store.usage().unwrap();
        assert!(
            store
                .reserve_interpreter_allowance(task.clone(), &pin, &p, &runtime)
                .is_err(),
            "{bound}"
        );
        assert_eq!(store.usage().unwrap(), before);
        assert!(
            store
                .get(&task.snapshot.job_id)
                .unwrap()
                .input_interpretation
                .is_none()
        );
    }
}

#[test]
fn usage_is_bounded_private_read_only_and_available_after_revocation() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    for _ in 0..2 {
        let r = reserve_new(&store, input(&store, "input"), &pin, &p, &runtime);
        store.interpreter_not_sent(r).unwrap();
    }
    store
        .interpreter_policy_command(
            Command::Revoke {
                id: pin.id.clone(),
                revision: 1,
                operation_id: uuid(),
            },
            &runtime,
            None,
            eefn::protocol::now_ms(),
        )
        .unwrap();
    let options = crate::interpreter_usage::Options {
        id: Some(pin.id.clone()),
        limit: 1,
        after_sequence: 0,
    };
    let before = store.usage().unwrap();
    let first = store.interpreter_usage(&options, &runtime).unwrap();
    assert!(crate::interpreter_usage::confirmed(
        &first, &options, &runtime
    ));
    assert_eq!(first["policy"]["revoked"], true);
    assert_eq!(first["charged"]["calls"], 2);
    assert_eq!(first["next_after"], 1);
    for private in [
        "Check this node",
        "interpreter_task",
        "params",
        "request_context",
        "report",
        "grant",
    ] {
        assert!(!first.as_object().unwrap().contains_key(private));
        assert!(!first["items"][0].as_object().unwrap().contains_key(private));
    }
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("Check this node")
    );
    let second_options = crate::interpreter_usage::Options {
        after_sequence: 1,
        ..options.clone()
    };
    let second = store.interpreter_usage(&second_options, &runtime).unwrap();
    assert_eq!(second["items"][0]["sequence"], 2);
    assert!(second["next_after"].is_null());
    for change in ["flags", "count", "sequence", "phase", "extra", "runtime"] {
        let mut bad = first.clone();
        match change {
            "flags" => bad["grants_issued"] = json!(true),
            "count" => bad["charged"]["calls"] = json!(0),
            "sequence" => bad["items"][0]["sequence"] = json!(2),
            "phase" => bad["items"][0]["result_state"] = json!("accepted"),
            "extra" => bad["grant"] = json!({}),
            _ => bad["runtime_id"] = json!(uuid()),
        }
        assert!(
            !crate::interpreter_usage::confirmed(&bad, &options, &runtime),
            "{change}"
        );
    }
    assert!(
        crate::interpreter_usage::render(&first)
            .unwrap()
            .contains("conservative")
    );
    assert_eq!(store.usage().unwrap(), before);
}

#[test]
fn allowance_escrow_and_automation_approval_share_the_same_admission_budget() {
    for allowance_first in [false, true] {
        let mut store = JobStore::in_memory().unwrap();
        let runtime = uuid();
        let pin = policy(&store, &runtime, limits());
        let p = permit();
        let task = input(&store, "input");
        let (engine, _, _, approval) =
            super::super::automation_authorizations::tests::fixture(store.clone());
        let approval_cost = serde_json::to_vec(&approval).unwrap().len() + 1024;
        let save_approval = |s: &JobStore| {
            s.automation_policy_checkpoint(
                approval.creation.clone(),
                Some(approval.clone()),
                &approval.runtime_id,
                approval.approved_at_ms,
            )
        };
        let reserved = if allowance_first {
            Some(reserve_new(&store, task.clone(), &pin, &p, &runtime))
        } else {
            save_approval(&store).unwrap();
            None
        };
        drop(engine);
        let before = used(&store);
        // A full journal must reject either kind of new admission; retiring an
        // existing reservation remains possible without refunding its charges.
        Arc::get_mut(&mut store).unwrap().max_bytes = before;
        let usage_before = store.usage().unwrap();
        if allowance_first {
            assert!(save_approval(&store).is_err());
        } else {
            assert!(
                store
                    .reserve_interpreter_allowance(task.clone(), &pin, &p, &runtime)
                    .is_err()
            );
        }
        assert_eq!(store.usage().unwrap(), usage_before);
        if let Some(r) = reserved {
            store.interpreter_not_sent(r).unwrap();
            Arc::get_mut(&mut store).unwrap().max_bytes = used(&store) + approval_cost;
            save_approval(&store).unwrap();
        }
        assert_eq!(store.usage().unwrap()["authorization_count"], 1);
    }
}

#[test]
fn grant_wall_and_monotonic_deadlines_cannot_outlive_policy_expiry() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let p = permit();
    let now = eefn::protocol::now_ms();
    let value = store
        .interpreter_policy_command(
            Command::Approve {
                operation_id: uuid(),
                generation: 0,
                scope: Scope {
                    origin_nodes: vec!["input".into()],
                    provider_nodes: vec!["provider".into()],
                },
                limits: limits(),
                valid_for_ms: Some(30_000),
            },
            &runtime,
            Some(&p.issuer),
            now - 20_000,
        )
        .unwrap();
    let policy: Authorization = serde_json::from_value(value["authorization"].clone()).unwrap();
    let pin = PolicyPin::from(&policy);
    let task = input(&store, "input");
    let r = reserve_new(&store, task, &pin, &p, &runtime);
    let g = &r.record.binding.grant;
    assert_eq!(Some(g.expires_at_ms), policy.expires_at_ms);
    assert!(g.limits.max_execution_ms <= 10_000);
    assert_eq!(g.expires_at_ms - g.issued_at_ms, g.limits.max_execution_ms);
    assert!(r.deadline <= Instant::now() + Duration::from_millis(g.limits.max_execution_ms));
    let d = store
        .interpreter_delivery_intent(&r, &p, &runtime, &pin)
        .unwrap();
    assert_eq!(d.deadline(), r.deadline);
    let account = &store.interpreter_allowance_records().unwrap().1[&pin.id];
    assert_eq!(account.charged.execution_ms, g.limits.max_execution_ms);
}

fn stop_report(g: &ExecutionGrant) -> eefn::grants::stop::Report {
    eefn::grants::stop::Report {
        schema_version: 1,
        report_type: "grant_stop_receipt".into(),
        grant_id: g.grant_id.clone(),
        grant_sha256: eefn::grants::definition_digest(&json!(g)).unwrap(),
        issuer_id: g.issuer_id.clone(),
        session_id: g.session_id.clone(),
        executor_node: g.executor_node.clone(),
        job_id: g.job_id.clone(),
        task_id: g.task_id.clone(),
        attempt: g.attempt,
        recorded_at_ms: eefn::protocol::now_ms().max(1),
        receipt_preexisted: true,
        reused: false,
        cancellation_signalled: true,
        persisted: true,
        stop_requested: true,
        execution_stopped: false,
        external_effects_stopped: false,
        execution_authorized: false,
        automatically_retried: false,
    }
}

#[test]
fn unsent_interpreter_stop_is_atomic_without_a_network_send_or_budget_refund() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let r = reserve_new(&store, task.clone(), &pin, &p, &runtime);
    assert!(
        store
            .interpreter_stop_intent(r.id(), &"f".repeat(64))
            .is_err()
    );
    assert!(
        store
            .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .interpreter_delivery_intent(&r, &p, &runtime, &pin)
            .is_err()
    );
    let (records, accounts) = store.interpreter_allowance_records().unwrap();
    assert_eq!(records[0].phase, Phase::NotSent);
    assert!(records[0].stop.is_none());
    assert_eq!(accounts[&pin.id].charged.calls, 1);
    assert_eq!(
        store
            .get(&task.snapshot.job_id)
            .unwrap()
            .input_interpretation
            .unwrap()
            .state,
        eefn::request_interpretation::State::Unknown
    );
}

#[test]
fn interpreter_stop_intent_precedes_one_send_and_receipts_never_prove_effects_stopped() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let r = reserve_new(&store, task, &pin, &p, &runtime);
    let d = store
        .interpreter_delivery_intent(&r, &p, &runtime, &pin)
        .unwrap();
    assert!(
        store
            .interpreter_stop_observed(r.id(), &r.record.binding_sha256, stop_report(d.grant()))
            .is_err()
    );
    let to_send = store
        .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
        .unwrap()
        .unwrap();
    assert_eq!(json!(to_send), json!(d.grant()));
    let checkpoint = store.interpreter_allowance_records().unwrap().0.remove(0);
    assert!(checkpoint.stop.as_ref().unwrap().outcome_unknown);
    let before = checkpoint.allocation().unwrap();
    assert!(
        store
            .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
            .unwrap()
            .is_none()
    );
    let mut forged = stop_report(d.grant());
    forged.execution_stopped = true;
    assert!(
        store
            .interpreter_stop_observed(r.id(), &r.record.binding_sha256, forged)
            .is_err()
    );
    let report = stop_report(d.grant());
    let saved = store
        .interpreter_stop_observed(r.id(), &r.record.binding_sha256, report.clone())
        .unwrap();
    assert_eq!(saved.allocation().unwrap(), before);
    assert_eq!(
        saved.stop.as_ref().unwrap().report.as_ref().unwrap(),
        &report
    );
    assert!(saved.holds_slot());
    let mut erased = saved.clone();
    erased.stop = None;
    erased.revision = 2;
    erased.validate().unwrap();
    assert!(
        update_record(&store.connection.lock().unwrap(), &saved, &erased).is_err(),
        "Ordinary checkpoints cannot erase stop intent or its receipt"
    );
    assert_eq!(
        store
            .interpreter_stop_observed(r.id(), &r.record.binding_sha256, report)
            .unwrap()
            .revision,
        saved.revision
    );
    let retired = store
        .retire_interpreter_continuation(r.id(), &r.record.binding_sha256)
        .unwrap();
    assert_eq!(retired.phase, Phase::Interrupted);
    assert_eq!(retired.stop, saved.stop);
    assert!(retired.holds_slot());
    let again = store
        .retire_interpreter_continuation(r.id(), &r.record.binding_sha256)
        .unwrap();
    assert_eq!(again.revision, retired.revision);
    assert!(
        store
            .interpreter_delivery_intent(&r, &p, &runtime, &pin)
            .is_err()
    );
}

#[test]
fn late_interpreter_stop_receipt_fits_original_headroom_after_final_completion() {
    let mut store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let r = reserve_new(&store, task, &pin, &p, &runtime);
    let d = store
        .interpreter_delivery_intent(&r, &p, &runtime, &pin)
        .unwrap();
    store
        .interpreter_report(
            r.id(),
            GrantReport::new(d.grant(), ReceiptState::Accepted, json!(null), None),
        )
        .unwrap();
    store
        .interpreter_stop_intent(r.id(), &r.record.binding_sha256)
        .unwrap()
        .unwrap();
    let final_record = store
        .interpreter_report(
            r.id(),
            GrantReport::new(
                d.grant(),
                ReceiptState::Completed,
                json!(null),
                Some("result_too_large"),
            ),
        )
        .unwrap();
    assert!(!final_record.holds_slot());
    assert!(final_record.checkpoint_reserve().unwrap() > 0);
    Arc::get_mut(&mut store).unwrap().max_bytes = 1024;
    let before = store.usage().unwrap();
    assert_eq!(before["over_limit"], true);
    let stopped = store
        .interpreter_stop_observed(r.id(), &r.record.binding_sha256, stop_report(d.grant()))
        .unwrap();
    assert_eq!(stopped.checkpoint_reserve().unwrap(), 0);
    assert!(!stopped.stop.unwrap().report.unwrap().execution_stopped);
    assert!(
        store.usage().unwrap()["interpreter_allowance_bytes"]
            .as_u64()
            .unwrap()
            < before["interpreter_allowance_bytes"].as_u64().unwrap()
    );
}

#[test]
fn active_unissued_input_can_be_cancelled_without_an_invalid_stopping_state() {
    let store = JobStore::in_memory().unwrap();
    let runtime = uuid();
    let pin = policy(&store, &runtime, limits());
    let p = permit();
    let task = input(&store, "input");
    let r = reserve_new(&store, task.clone(), &pin, &p, &runtime);
    let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
    let guard = engine.claim(&task.snapshot.job_id, || Ok(())).unwrap();
    let cancelled = engine.cancel(&task.snapshot.job_id).unwrap();
    assert_eq!(cancelled.status, PlanStatus::Cancelled);
    assert!(cancelled.execution.is_none() && cancelled.tasks.is_empty());
    let retired = store
        .retire_interpreter_continuation(r.id(), &r.record.binding_sha256)
        .unwrap();
    assert_eq!(retired.phase, Phase::NotSent);
    assert_eq!(
        store
            .get(&task.snapshot.job_id)
            .unwrap()
            .input_interpretation
            .unwrap()
            .state,
        eefn::request_interpretation::State::Unknown
    );
    drop(guard);
    assert_eq!(
        store.get(&task.snapshot.job_id).unwrap().status,
        PlanStatus::Cancelled
    );
}

#[test]
fn source_observation_wakes_on_saved_edits_and_removal_without_polling_or_read_side_effects() {
    let store = JobStore::in_memory().unwrap();
    let task = input(&store, "input");
    let mut changes = store.source_changes();
    store.get(&task.snapshot.job_id).unwrap();
    store.list().unwrap();
    store.usage().unwrap();
    assert!(!changes.has_changed().unwrap());
    let before = json!(store.get(&task.snapshot.job_id).unwrap());
    assert!(
        store
            .change(&task.snapshot.job_id, "Refused source edit", |p| {
                p.status = PlanStatus::Cancelled;
                anyhow::bail!("Refused")
            })
            .is_err()
    );
    assert!(!changes.has_changed().unwrap());
    assert_eq!(json!(store.get(&task.snapshot.job_id).unwrap()), before);
    store
        .change(&task.snapshot.job_id, "Owner cancelled source", |p| {
            p.status = PlanStatus::Cancelled;
            Ok(())
        })
        .unwrap();
    assert!(changes.has_changed().unwrap());
    let version = *changes.borrow_and_update();
    store.remove_finished(&task.snapshot.job_id).unwrap();
    assert!(changes.has_changed().unwrap());
    assert_ne!(*changes.borrow_and_update(), version);
}
