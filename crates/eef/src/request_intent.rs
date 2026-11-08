//! Raw node input in the existing journal, with no invented semantic proposal.
use crate::task::{Goal, Plan, PlanStatus, TaskEngine};
use anyhow::{Context, Result, ensure};
use eefn::{
    context::RequestContext,
    request_intent::{Input, Saved, TEMPLATE},
};
use serde_json::{Value, json};

pub(crate) fn validate(plan: &Plan) -> Result<Input> {
    eefn::request_drafts::validate_id(&plan.plan_id)?;
    let input: Input = serde_json::from_value(plan.goal.params.clone())?;
    input.validate()?;
    let context = plan
        .goal
        .request_context
        .as_ref()
        .context("Saved input origin missing")?;
    eefn::job_commands::validate_operation_id(context.request_id())?;
    ensure!(
        plan.goal.template == TEMPLATE
            && plan.goal.description == input.description()
            && plan.goal.constraints == json!({})
            && plan.request_review.is_none()
            && plan.planning.is_none()
            && plan.approval.is_none()
            && plan.execution.is_none()
            && plan.tasks.is_empty()
            && plan.results.is_empty()
            && matches!(plan.status, PlanStatus::Review | PlanStatus::Cancelled),
        "Saved input cannot contain interpretation, task, approval or execution state"
    );
    Ok(input)
}
pub(crate) fn save(engine: &TaskEngine, payload: Value, context: &RequestContext) -> Result<Value> {
    let input: Input = serde_json::from_value(payload)?;
    input.validate()?;
    let admission = engine.store_input_intent(Plan {
        execution: None,
        approval: None,
        planning: None,
        request_review: None,
        plan_id: uuid::Uuid::new_v4().simple().to_string(),
        goal: Goal {
            request_context: Some(context.clone()),
            description: input.description(),
            template: TEMPLATE.into(),
            params: json!(input),
            constraints: json!({}),
        },
        tasks: vec![],
        status: PlanStatus::Review,
        results: Default::default(),
        revision: 0,
        updated_at_ms: 0,
        history: vec![],
    })?;
    let plan = admission.plan;
    let input = validate(&plan)?;
    let context = plan
        .goal
        .request_context
        .context("Saved input origin missing")?;
    let report = Saved {
        schema_version: 1,
        report_type: "coordinator_request_intent".into(),
        success: true,
        id: plan.plan_id,
        revision: plan.revision,
        status: match plan.status {
            PlanStatus::Review => "review",
            _ => "cancelled",
        }
        .into(),
        operation_id: context.request_id().into(),
        source_sha256: input.digest(&context)?,
        request_context: context,
        input,
        reused: admission.reused,
        persisted: true,
        interpretation_requested: false,
        execution_authorized: false,
        dispatched: false,
    };
    report.validate()?;
    Ok(json!(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::EventBus, jobs::JobStore};

    fn context() -> RequestContext {
        RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "input-node".into(),
            vec!["home".into(), "bedroom".into()],
        )
        .unwrap()
    }
    fn payload() -> Value {
        json!({"schema_version":1,"original_text":"What is happening downstairs?"})
    }
    fn saved(engine: &TaskEngine, origin: &RequestContext) -> Saved {
        serde_json::from_value(save(engine, payload(), origin).unwrap()).unwrap()
    }

    #[test]
    fn raw_input_survives_reopen_without_a_proposal_or_executor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let origin = context();
        let first = {
            let engine = TaskEngine::with_store(
                EventBus::new(10),
                JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap(),
            );
            assert!(!engine.has_executor());
            saved(&engine, &origin)
        };
        let store = JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
        let plan = store.get(&first.id).unwrap();
        let view = crate::jobs::view(&plan, true);
        assert_eq!(view["review_state"], "awaiting_interpretation");
        assert_eq!(view["planning_state"], "not_interpreted");
        assert_eq!(view["proposal_count"], 0);
        assert_eq!(view["can_resume"], false);
        assert_eq!(view["input_intent"], payload());
        assert!(
            crate::jobs::view(&plan, false)
                .get("input_intent")
                .is_none()
        );
        assert_eq!(plan.goal.request_context.as_ref(), Some(&origin));
        assert!(
            plan.request_review.is_none()
                && plan.planning.is_none()
                && plan.approval.is_none()
                && plan.execution.is_none()
        );
        assert!(plan.tasks.is_empty() && plan.results.is_empty());
        assert_eq!(plan.revision, first.revision);
        assert_eq!(plan.status, PlanStatus::Review);
        let engine = TaskEngine::with_store(EventBus::new(10), store);
        let again = saved(&engine, &origin);
        assert!(again.reused);
        assert_eq!(again.id, first.id);
        assert_eq!(again.source_sha256, first.source_sha256);
        assert_eq!(again.revision, first.revision);
    }

    #[test]
    fn receipt_reuse_preserves_origin_area_and_refuses_changed_text() {
        let engine = TaskEngine::new(EventBus::new(10));
        let origin = context();
        let first = saved(&engine, &origin);
        let moved = RequestContext::new(
            origin.request_id().into(),
            origin.origin_node().into(),
            vec!["elsewhere".into()],
        )
        .unwrap();
        let again = saved(&engine, &moved);
        assert!(again.reused);
        assert_eq!(again.request_context, origin);
        assert_eq!(again.source_sha256, first.source_sha256);
        assert!(
            save(
                &engine,
                json!({"schema_version":1,"original_text":"Changed request"}),
                &origin
            )
            .is_err()
        );
        assert_eq!(engine.list_plans().unwrap().len(), 1);
        let other =
            RequestContext::new(origin.request_id().into(), "another-node".into(), vec![]).unwrap();
        assert_ne!(saved(&engine, &other).id, first.id);
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_legacy_execution_cannot_start_input() {
        struct Executor;
        #[async_trait::async_trait]
        impl crate::task::TaskExecutor for Executor {
            async fn execute(&self, _: &crate::task::Task) -> Result<Value> {
                panic!("Raw input must never execute")
            }
        }
        let store = JobStore::in_memory().unwrap();
        let engine = TaskEngine::legacy_with_store_for_test(EventBus::new(10), store.clone());
        engine.set_executor(std::sync::Arc::new(Executor));
        let origin = context();
        let first = saved(&engine, &origin);
        assert!(engine.pause(&first.id).is_err());
        assert!(engine.resume_job(&first.id).is_err());
        let mut forged = store.get(&first.id).unwrap();
        forged.status = PlanStatus::Pending;
        assert!(engine.run_plan(forged).await.is_err());
        assert_eq!(
            engine.cancel(&first.id).unwrap().status,
            PlanStatus::Cancelled
        );
        let again = saved(&engine, &origin);
        assert!(again.reused);
        assert_eq!(again.status, "cancelled");
        assert_eq!(again.id, first.id);
        assert!(engine.resume_job(&first.id).is_err());
        assert!(store.get(&first.id).unwrap().tasks.is_empty());
    }

    #[test]
    fn journal_quota_refusal_and_invalid_input_leave_retained_state_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 1, 2 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
        let origin = context();
        let first = saved(&engine, &origin);
        let before = serde_json::to_value(store.get(&first.id).unwrap()).unwrap();
        assert!(save(&engine, payload(), &context()).is_err());
        for bad in [
            json!({"schema_version":2,"original_text":"text"}),
            json!({"schema_version":1,"original_text":"text","execute":true}),
            json!({"schema_version":1,"original_text":" "}),
            json!({"schema_version":1,"original_text":"x".repeat(32769)}),
        ] {
            assert!(save(&engine, bad, &origin).is_err());
        }
        assert_eq!(
            serde_json::to_value(store.get(&first.id).unwrap()).unwrap(),
            before
        );
        assert!(
            saved(&engine, &origin).reused,
            "Quota does not block inspection of a retained receipt"
        );
        engine.cancel(&first.id).unwrap();
        assert_eq!(saved(&engine, &origin).status, "cancelled");
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(10), store);
        assert!(
            saved(&engine, &origin).reused,
            "Lowered quota must preserve retained input"
        );
    }

    #[test]
    fn raw_rows_reject_interpreted_or_executable_state() {
        let store = JobStore::in_memory().unwrap();
        let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
        let first = saved(&engine, &context());
        let original = serde_json::to_value(store.get(&first.id).unwrap()).unwrap();
        for (key, value) in [
            ("status", json!("pending")),
            ("results", json!({"fake":"result"})),
            ("plan_id", json!("invalid")),
            (
                "goal",
                json!({"template":TEMPLATE,"description":"Invented interpretation","params":payload(),"constraints":{}}),
            ),
        ] {
            let mut changed = original.clone();
            changed[key] = value;
            let plan: Plan = serde_json::from_value(changed).unwrap();
            assert!(validate(&plan).is_err());
        }
        assert!(
            store
                .change(&first.id, "Cannot bypass raw input", |p| {
                    p.status = PlanStatus::Running;
                    Ok(())
                })
                .is_err()
        );
        for mutation in 0..3 {
            assert!(
                store
                    .change(&first.id, "Cannot rewrite saved input", |p| {
                        if mutation == 0 {
                            let changed = Input {
                                schema_version: 1,
                                original_text: "Changed after admission".into(),
                            };
                            p.goal.description = changed.description();
                            p.goal.params = json!(changed);
                        } else if mutation == 1 {
                            p.goal.request_context = Some(context());
                        } else {
                            p.plan_id = uuid::Uuid::new_v4().simple().to_string();
                        }
                        Ok(())
                    })
                    .is_err()
            );
        }
        assert_eq!(
            serde_json::to_value(store.get(&first.id).unwrap()).unwrap(),
            original
        );
    }
}
