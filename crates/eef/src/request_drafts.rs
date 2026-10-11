//! Durable origin-owned review in the existing journal; never dispatches work.
use crate::task::{Goal, Plan, PlanStatus, TaskEngine};
use anyhow::{Context, Result, bail};
use eefn::{
    context::RequestContext,
    interpretation::Proposal,
    request_drafts::{ClarificationAnswer, DraftCommand, DraftRecord, ProposalRevision},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Opening {
    schema_version: u32,
    original_text: String,
    proposal: Proposal,
}

fn report(plan: &Plan, operation_id: &str, reused: bool) -> Result<Value> {
    let draft = plan
        .request_review
        .as_ref()
        .context("Not a durable request review")?;
    draft.validate()?;
    Ok(
        json!({"schema_version":1,"report_type":"coordinator_request_draft","id":plan.plan_id,
        "revision":plan.revision,"status":plan.status,"operation_id":operation_id,
        "request_context":plan.goal.request_context,"draft":draft,"review_state":draft.state(),
        "reused":reused,"execution_authorized":false,"dispatched":false,"persisted":true,
        "note":"Read-only request inspection; this command issued no grant or execution. The retained record may include separately requested execution and its receipts."}),
    )
}
pub fn open(
    engine: &TaskEngine,
    payload: Value,
    context: &RequestContext,
    capabilities: &[String],
) -> Result<Value> {
    eefn::job_commands::validate_operation_id(context.request_id())?;
    crate::request_review::review(payload.clone(), context, capabilities)?;
    let input: Opening = serde_json::from_value(payload)?;
    if input.schema_version != 1 {
        bail!("Unsupported request schema")
    }
    let draft = DraftRecord {
        schema_version: 1,
        original_text: input.original_text,
        concrete_input: None,
        proposals: vec![ProposalRevision {
            operation_id: context.request_id().into(),
            proposal: input.proposal,
            allowed_capabilities: capabilities.to_vec(),
            at_ms: eefn::protocol::now_ms(),
            resolution: None,
            preparation: None,
        }],
        answers: vec![],
    };
    let admission = store_review(engine, draft, context)?;
    report(&admission.plan, context.request_id(), admission.reused)
}

pub(crate) fn store_review(
    engine: &TaskEngine,
    draft: DraftRecord,
    context: &RequestContext,
) -> Result<crate::jobs::JobAdmission> {
    draft.validate()?;
    let description = draft.proposals[0].proposal.goal.clone();
    let plan = Plan {
        execution: None,
        approval: None,
        planning: None,
        request_review: Some(draft),
        plan_id: uuid::Uuid::new_v4().simple().to_string(),
        goal: Goal {
            request_context: Some(context.clone()),
            description,
            template: "request_review".into(),
            params: json!({}),
            constraints: json!({}),
        },
        tasks: vec![],
        status: PlanStatus::Review,
        results: Default::default(),
        revision: 0,
        updated_at_ms: 0,
        history: vec![],
    };
    engine.store_review_request(plan)
}
pub fn command(
    engine: &TaskEngine,
    kind: &str,
    mut payload: Value,
    context: &RequestContext,
    capabilities: &[String],
) -> Result<Value> {
    eefn::job_commands::validate_operation_id(context.request_id())?;
    if !payload.is_object()
        || payload.get("operation").is_some()
        || serde_json::to_vec(&payload)?.len() > 64 * 1024
    {
        bail!("Invalid draft command payload")
    }
    payload["operation"] = json!(match kind {
        "requests.draft_get" => "get",
        "requests.draft_answer" => "answer",
        "requests.draft_revise" => "revise",
        "requests.draft_resolve" => "resolve",
        _ => bail!("Unsupported draft command"),
    });
    let command: DraftCommand = serde_json::from_value(payload)?;
    command.submission()?;
    let owns = |plan: &Plan| {
        plan.goal
            .request_context
            .as_ref()
            .is_some_and(|c| c.origin_node() == context.origin_node())
    };
    let mut reused = false;
    let saved = if matches!(command, DraftCommand::Get { .. }) {
        let plan = engine.store.get(command.id())?;
        if !owns(&plan) {
            bail!("Draft not found for this node")
        }
        plan
    } else {
        engine
            .store
            .edit(command.id(), "Origin updated request review", |plan| {
                if !owns(plan) {
                    bail!("Draft not found for this node")
                }
                let draft = plan
                    .request_review
                    .as_mut()
                    .context("Not a durable request review")?;
                draft.validate()?;
                match &command {
                    DraftCommand::Answer {
                        expected_revision,
                        text,
                        ..
                    } => {
                        if let Some(answer) = draft
                            .answers
                            .iter()
                            .find(|a| a.operation_id == context.request_id())
                        {
                            if answer.text != *text || answer.source.as_ref().is_some_and(|s|s.revision!=*expected_revision) {
                                bail!("Answer receipt conflicts with retained review")
                            }
                            reused = true;
                            return Ok(false);
                        }
                        if draft
                            .proposals
                            .iter()
                            .any(|p| p.operation_id == context.request_id())
                        {
                            bail!("Receipt already belongs to a proposal")
                        }
                        if plan.status != PlanStatus::Review || plan.revision != *expected_revision
                        {
                            bail!("Draft state or revision changed; inspect it before retrying")
                        }
                        let question = draft
                            .current()
                            .clarification
                            .clone()
                            .context("This proposal has no clarification question")?;
                        let source=eefn::request_drafts::AnswerSource::from_draft(*expected_revision,draft)?;
                        draft.answers.push(ClarificationAnswer {
                            operation_id: context.request_id().into(),
                            proposal_index: draft.proposals.len() - 1,
                            question,
                            text: text.clone(),
                            at_ms: eefn::protocol::now_ms(),
                            source:Some(source),
                        });
                    }
                    DraftCommand::Revise {
                        expected_revision,
                        original_text,
                        proposal,
                        ..
                    } | DraftCommand::Resolve {expected_revision, original_text, proposal, ..} => {
                        let resolution = match &command {DraftCommand::Resolve {evidence,..} => Some(evidence.clone()), _ => None};
                        if original_text != &draft.original_text {
                            bail!("Original request text is immutable")
                        }
                        if let Some(revision) = draft
                            .proposals
                            .iter()
                            .find(|p| p.operation_id == context.request_id())
                        {
                            if revision.proposal != *proposal || revision.resolution != resolution || revision.preparation.is_some() {
                                bail!("Proposal receipt conflicts with retained review")
                            }
                            reused = true;
                            return Ok(false);
                        }
                        if draft
                            .answers
                            .iter()
                            .any(|a| a.operation_id == context.request_id())
                        {
                            bail!("Receipt already belongs to an answer")
                        }
                        if plan.status != PlanStatus::Review || plan.revision != *expected_revision
                        {
                            bail!("Draft state or revision changed; inspect it before retrying")
                        }
                        if let Some(evidence) = &resolution {
                            if !evidence.confirms_source(draft) {
                                bail!("Clarification resolution source changed or has no recorded answer")
                            }
                        }
                        eefn::interpretation::validate(
                            original_text,
                            &serde_json::to_vec(proposal)?,
                            capabilities,
                        )?;
                        draft.proposals.push(ProposalRevision {
                            operation_id: context.request_id().into(),
                            proposal: proposal.clone(),
                            allowed_capabilities: capabilities.to_vec(),
                            at_ms: eefn::protocol::now_ms(),
                            resolution,
                            preparation: None,
                        });
                    }
                    DraftCommand::Get { .. } => unreachable!(),
                }
                draft.validate()?;
                Ok(true)
            })?
    };
    report(&saved, context.request_id(), reused)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(node: &str) -> RequestContext {
        RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            node.into(),
            vec!["Room".into()],
        )
        .unwrap()
    }
    fn input() -> Value {
        json!({"schema_version":1,"original_text":"  Inspect that node\n","proposal":{
        "schema_version":1,"intent":"information","goal":"Inspect node status","context_hints":[],"constraints":[],
        "suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot",
        "needs_clarification":true,"clarification":"Which node?"}})
    }
    fn caps() -> Vec<String> {
        vec!["system.info".into()]
    }
    fn engine() -> std::sync::Arc<TaskEngine> {
        TaskEngine::new(crate::event::EventBus::new(20))
    }
    fn draft(engine: &TaskEngine, ctx: &RequestContext) -> Value {
        open(engine, input(), ctx, &caps()).unwrap()
    }
    #[tokio::test]
    async fn resolution_pins_full_answer_history_atomically_and_is_non_executable_after_reopen() {
        use eefn::request_resolution::ResolutionEvidence;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(crate::event::EventBus::new(20), store.clone());
        let opening = context("owner");
        let created = draft(&engine, &opening);
        let id = created["id"].as_str().unwrap();
        let answered = command(
            &engine,
            "requests.draft_answer",
            json!({"id":id,"expected_revision":1,"text":"Bedroom"}),
            &context("owner"),
            &caps(),
        )
        .unwrap();
        let source: DraftRecord = serde_json::from_value(answered["draft"].clone()).unwrap();
        let evidence = ResolutionEvidence {
            schema_version: 1,
            source_revision: 2,
            source_sha256: ResolutionEvidence::source_digest(&source).unwrap(),
            source_proposal_count: 1,
            source_answer_count: 1,
            backend: "ollama".into(),
            model_id: "fixture".into(),
        };
        let mut proposal = source.current().clone();
        proposal.needs_clarification = false;
        proposal.clarification = None;
        let payload = json!({"id":id,"expected_revision":2,"original_text":source.original_text,"proposal":proposal,"evidence":evidence});
        let before = json!(store.get(id).unwrap());
        for (who, bad) in [
            (context("other"), payload.clone()),
            (context("owner"), {
                let mut bad = payload.clone();
                bad["evidence"]["source_sha256"] = json!("a".repeat(64));
                bad
            }),
            (context("owner"), {
                let mut bad = payload.clone();
                bad["expected_revision"] = json!(1);
                bad
            }),
            (context("owner"), {
                let mut bad = payload.clone();
                bad["original_text"] = json!("changed");
                bad
            }),
            (context("owner"), {
                let mut bad = payload.clone();
                bad["permissions"] = json!(true);
                bad
            }),
        ] {
            assert!(command(&engine, "requests.draft_resolve", bad, &who, &caps()).is_err());
            assert_eq!(json!(store.get(id).unwrap()), before);
        }
        let ctx = context("owner");
        let saved = command(
            &engine,
            "requests.draft_resolve",
            payload.clone(),
            &ctx,
            &caps(),
        )
        .unwrap();
        assert_eq!(saved["revision"], 3);
        assert_eq!(saved["status"], "review");
        assert_eq!(saved["review_state"], "review_required");
        assert_eq!(saved["draft"]["answers"], answered["draft"]["answers"]);
        assert_eq!(saved["request_context"], created["request_context"]);
        assert_eq!(
            saved["draft"]["proposals"][1]["resolution"],
            json!(evidence)
        );
        let plan = store.get(id).unwrap();
        assert!(plan.tasks.is_empty());
        assert!(engine.run_plan(plan.clone()).await.is_err());
        assert_eq!(
            command(
                &engine,
                "requests.draft_resolve",
                payload.clone(),
                &ctx,
                &[]
            )
            .unwrap()["reused"],
            true
        );
        assert_eq!(json!(store.get(id).unwrap()), json!(plan));
        let mut conflicting = payload.clone();
        conflicting["evidence"]["model_id"] = json!("other");
        assert!(
            command(
                &engine,
                "requests.draft_resolve",
                conflicting,
                &ctx,
                &caps()
            )
            .is_err()
        );
        let mut manual = payload;
        manual.as_object_mut().unwrap().remove("evidence");
        assert!(command(&engine, "requests.draft_revise", manual, &ctx, &caps()).is_err());
        for field in ["digest", "answer", "future_revision"] {
            let mut bad = plan.clone();
            let draft = bad.request_review.as_mut().unwrap();
            match field {
                "digest" => {
                    draft.proposals[1]
                        .resolution
                        .as_mut()
                        .unwrap()
                        .source_sha256 = "a".repeat(64)
                }
                "answer" => draft.answers[0].text.push_str("changed"),
                "future_revision" => {
                    draft.proposals[1]
                        .resolution
                        .as_mut()
                        .unwrap()
                        .source_revision = 3
                }
                _ => unreachable!(),
            }
            assert!(
                crate::jobs::validate_retained_review(&bad).is_err(),
                "{field}"
            );
        }
        drop(engine);
        drop(store);
        let reopened = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(json!(reopened.get(id).unwrap()), json!(plan));
    }
    #[tokio::test]
    async fn durable_review_is_non_executable_origin_owned_and_retained_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(crate::event::EventBus::new(20), store.clone());
        let ctx = context("owner");
        let created = draft(&engine, &ctx);
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["status"], "review");
        assert_eq!(created["revision"], 1);
        assert!(eefn::request_drafts::confirmed_report(
            &created,
            "owner",
            ctx.request_id(),
            None
        ));
        let saved = store.get(id).unwrap();
        assert!(saved.tasks.is_empty());
        assert!(engine.run_plan(saved.clone()).await.is_err());
        assert!(engine.enqueue(saved.clone()).is_err());
        assert!(engine.enqueue_request(saved.clone()).is_err());
        assert!(engine.pause(id).is_err());
        assert!(engine.resume_job(id).is_err());
        let public = crate::jobs::view(&saved, true);
        assert_eq!(public["review_only"], true);
        assert_eq!(public["can_resume"], false);
        assert!(public.get("request_review").is_none());
        assert!(public.get("original_text").is_none());
        let stranger = context("other");
        assert!(
            command(
                &engine,
                "requests.draft_get",
                json!({"id":id}),
                &stranger,
                &caps()
            )
            .is_err()
        );
        engine.shutdown().await;
        assert_eq!(store.get(id).unwrap().status, PlanStatus::Review);
        let retained = json!(store.get(id).unwrap());
        drop(engine);
        drop(store);
        let reopened = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(json!(reopened.get(id).unwrap()), retained);
    }
    #[test]
    fn answers_are_atomic_revision_checked_and_receipt_deduplicated() {
        let engine = engine();
        let opening = context("owner");
        let created = draft(&engine, &opening);
        let id = created["id"].as_str().unwrap();
        let answer = context("owner");
        let payload = json!({"id":id,"expected_revision":1,"text":"The bedroom node"});
        let updated = command(
            &engine,
            "requests.draft_answer",
            payload.clone(),
            &answer,
            &caps(),
        )
        .unwrap();
        assert_eq!(updated["revision"], 2);
        assert_eq!(updated["review_state"], "answer_recorded");
        assert_eq!(updated["draft"]["answers"][0]["question"], "Which node?");
        assert_eq!(updated["request_context"], created["request_context"]);
        assert_eq!(updated["draft"]["original_text"], input()["original_text"]);
        assert_eq!(updated["draft"]["answers"][0]["source"]["revision"], 1);
        let original: eefn::request_drafts::DraftRecord =
            serde_json::from_value(created["draft"].clone()).unwrap();
        let source: eefn::request_drafts::AnswerSource =
            serde_json::from_value(updated["draft"]["answers"][0]["source"].clone()).unwrap();
        assert!(source.confirms(&original));
        let before = json!(engine.store.get(id).unwrap());
        let repeated = command(
            &engine,
            "requests.draft_answer",
            payload.clone(),
            &answer,
            &caps(),
        )
        .unwrap();
        assert_eq!(repeated["reused"], true);
        assert_eq!(json!(engine.store.get(id).unwrap()), before);
        for (who, body) in [
            (context("other"), payload.clone()),
            (context("owner"), payload.clone()),
            (
                answer.clone(),
                json!({"id":id,"expected_revision":1,"text":"Changed answer"}),
            ),
            (
                answer.clone(),
                json!({"id":id,"expected_revision":2,"text":"The bedroom node"}),
            ),
            (
                context("owner"),
                json!({"id":id,"expected_revision":2,"text":"fine","permissions":true}),
            ),
        ] {
            assert!(command(&engine, "requests.draft_answer", body, &who, &caps()).is_err());
            assert_eq!(json!(engine.store.get(id).unwrap()), before);
        }
        let mut revised = input()["proposal"].clone();
        revised["needs_clarification"] = json!(false);
        revised["clarification"] = Value::Null;
        let revision = context("owner");
        let payload = json!({"id":id,"expected_revision":2,"original_text":input()["original_text"],"proposal":revised});
        let updated = command(
            &engine,
            "requests.draft_revise",
            payload.clone(),
            &revision,
            &caps(),
        )
        .unwrap();
        assert_eq!(updated["revision"], 3);
        assert_eq!(updated["review_state"], "review_required");
        assert_eq!(updated["draft"]["answers"].as_array().unwrap().len(), 1);
        assert_eq!(updated["draft"]["proposals"].as_array().unwrap().len(), 2);
        let before = json!(engine.store.get(id).unwrap());
        assert_eq!(
            command(
                &engine,
                "requests.draft_revise",
                payload.clone(),
                &revision,
                &caps()
            )
            .unwrap()["reused"],
            true
        );
        let mut changed = payload;
        changed["original_text"] = json!("different original");
        changed["expected_revision"] = json!(3);
        assert!(
            command(
                &engine,
                "requests.draft_revise",
                changed,
                &context("owner"),
                &caps()
            )
            .is_err()
        );
        assert_eq!(json!(engine.store.get(id).unwrap()), before);
        let reopened = open(&engine, input(), &opening, &caps()).unwrap();
        assert_eq!(reopened["reused"], true);
        assert_eq!(reopened["revision"], 3);
        assert_eq!(json!(engine.store.get(id).unwrap()), before);
        engine.cancel(id).unwrap();
        assert!(
            command(
                &engine,
                "requests.draft_answer",
                json!({"id":id,"expected_revision":4,"text":"new"}),
                &context("owner"),
                &caps()
            )
            .is_err()
        );
        engine.store.remove_finished(id).unwrap();
        assert!(engine.store.get(id).is_err());
    }
    #[test]
    fn concurrent_open_and_answer_have_one_committed_winner() {
        let engine = engine();
        let ctx = context("owner");
        let barrier = std::sync::Barrier::new(24);
        let replies = std::thread::scope(|scope| {
            let tasks = (0..24)
                .map(|_| {
                    let barrier = &barrier;
                    let engine = &engine;
                    let ctx = &ctx;
                    scope.spawn(move || {
                        barrier.wait();
                        open(engine, input(), ctx, &caps()).unwrap()
                    })
                })
                .collect::<Vec<_>>();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(replies.iter().filter(|r| r["reused"] == false).count(), 1);
        assert_eq!(engine.store.list().unwrap().len(), 1);
        let id = replies[0]["id"].as_str().unwrap();
        let barrier = std::sync::Barrier::new(24);
        let successes = std::thread::scope(|scope| {
            let tasks = (0..24)
                .map(|_| {
                    let barrier = &barrier;
                    let engine = &engine;
                    scope.spawn(move || {
                        barrier.wait();
                        command(
                            engine,
                            "requests.draft_answer",
                            json!({"id":id,"expected_revision":1,"text":"Bedroom"}),
                            &context("owner"),
                            &caps(),
                        )
                        .is_ok()
                    })
                })
                .collect::<Vec<_>>();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|won| *won)
                .count()
        });
        assert_eq!(successes, 1);
        assert_eq!(engine.store.get(id).unwrap().revision, 2);
    }
    #[tokio::test]
    async fn invalid_context_bounds_and_shutdown_do_not_persist_or_rewrite_reviews() {
        let engine = engine();
        let ctx = context("owner");
        let created = draft(&engine, &ctx);
        let id = created["id"].as_str().unwrap();
        let before = json!(engine.store.get(id).unwrap());
        let mut invalid = input();
        invalid["request_context"] = json!(ctx);
        assert!(open(&engine, invalid, &context("owner"), &caps()).is_err());
        assert!(open(&engine, input(), &context("owner"), &[]).is_err());
        let bad = RequestContext::new("not-a-receipt".into(), "owner".into(), vec![]).unwrap();
        assert!(open(&engine, input(), &bad, &caps()).is_err());
        for n in 0..16 {
            let rev = engine.store.get(id).unwrap().revision;
            command(
                &engine,
                "requests.draft_answer",
                json!({"id":id,"expected_revision":rev,"text":format!("answer-{n}")}),
                &context("owner"),
                &caps(),
            )
            .unwrap();
        }
        let full = json!(engine.store.get(id).unwrap());
        assert!(
            command(
                &engine,
                "requests.draft_answer",
                json!({"id":id,"expected_revision":17,"text":"too many"}),
                &context("owner"),
                &caps()
            )
            .is_err()
        );
        assert_eq!(json!(engine.store.get(id).unwrap()), full);
        assert!(before != full);
        engine.shutdown().await;
        assert!(open(&engine, input(), &context("owner"), &caps()).is_err());
        assert_eq!(engine.store.list().unwrap().len(), 1);
    }
    #[test]
    fn review_mutations_cannot_cross_execution_boundary_or_exceed_journal_quota() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = crate::jobs::JobStore::open(&path, 1, 6000).unwrap();
        let engine = TaskEngine::with_store(crate::event::EventBus::new(20), store.clone());
        let created = draft(&engine, &context("owner"));
        let id = created["id"].as_str().unwrap();
        let before = json!(store.get(id).unwrap());
        assert!(
            store
                .change(id, "invalid execution", |p| {
                    p.status = PlanStatus::Pending;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(store.get(id).unwrap()), before);
        assert!(
            command(
                &engine,
                "requests.draft_answer",
                json!({"id":id,"expected_revision":1,"text":"x".repeat(8192)}),
                &context("owner"),
                &caps()
            )
            .is_err()
        );
        assert_eq!(json!(store.get(id).unwrap()), before);
        assert!(open(&engine, input(), &context("owner"), &caps()).is_err());
        assert_eq!(store.list().unwrap().len(), 1);
        command(
            &engine,
            "requests.draft_answer",
            json!({"id":id,"expected_revision":1,"text":"Bedroom"}),
            &context("owner"),
            &caps(),
        )
        .unwrap();
        engine.cancel(id).unwrap();
        store.remove_finished(id).unwrap();
        assert!(open(&engine, input(), &context("owner"), &caps()).is_ok());
    }
    #[test]
    fn damaged_review_is_retained_without_recovery_dispatch_or_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(crate::event::EventBus::new(20), store.clone());
        let created = draft(&engine, &context("owner"));
        let id = created["id"].as_str().unwrap().to_string();
        let mut corrupt = json!(store.get(&id).unwrap());
        corrupt["status"] = json!("pending");
        drop(engine);
        drop(store);
        let sql = rusqlite::Connection::open(&path).unwrap();
        let raw = serde_json::to_string(&corrupt).unwrap();
        sql.execute(
            "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
            rusqlite::params![id, raw],
        )
        .unwrap();
        assert!(crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).is_err());
        let retained: String = sql
            .query_row(
                "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(retained, raw);
    }
}
