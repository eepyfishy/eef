//! Source-bound semantic checkpoints in the existing journal; never infer/execute.
use crate::task::{Plan, PlanStatus, TaskEngine};
use anyhow::{Context, Result, bail};
use eefn::{context::RequestContext, request_preparation::PreparationCheckpoint};
use serde_json::Value;

fn owns(plan: &Plan, origin: &str) -> bool {
    plan.goal
        .request_context
        .as_ref()
        .is_some_and(|c| c.origin_node() == origin)
}

/// Check receipts before live lookup; a same-ID different payload is a conflict,
/// never a reason to replace a question, rebind a plan or repeat inference.
fn exact_receipt(plan: &Plan, checkpoint: &PreparationCheckpoint) -> Result<bool> {
    let draft = plan
        .request_review
        .as_ref()
        .context("Not a retained request")?;
    if let Some(record) = plan
        .planning
        .as_ref()
        .filter(|p| p.operation_id == checkpoint.operation_id)
    {
        if record.preparation.as_ref() == Some(&checkpoint.source)
            && checkpoint.proposal.method.as_ref() == Some(&record.input)
            && checkpoint.proposal.clarification.is_none()
        {
            return Ok(true);
        }
        bail!("Preparation receipt conflicts with retained plan")
    }
    if let Some(revision) = draft
        .proposals
        .iter()
        .find(|p| p.operation_id == checkpoint.operation_id)
    {
        if revision.preparation.as_ref() == Some(&checkpoint.source)
            && checkpoint.proposal.method.is_none()
            && revision.proposal.clarification == checkpoint.proposal.clarification
        {
            return Ok(true);
        }
        bail!("Preparation receipt conflicts with retained proposal")
    }
    if draft
        .answers
        .iter()
        .any(|a| a.operation_id == checkpoint.operation_id)
    {
        bail!("Preparation receipt already belongs to an answer")
    }
    Ok(false)
}

fn question_checkpoint(
    engine: &TaskEngine,
    checkpoint: &PreparationCheckpoint,
    origin: &str,
    capabilities: &[String],
) -> Result<(Plan, bool)> {
    checkpoint.validate()?;
    let e = &checkpoint.source.evidence;
    let question = checkpoint
        .proposal
        .clarification
        .as_ref()
        .context("Not a question candidate")?;
    if checkpoint.proposal.method.is_some() {
        bail!("Ambiguous preparation candidate")
    }
    let mut reused = false;
    let plan = engine.edit_review_checkpoint(
        &e.job_id,
        "Method clarification saved; no execution authorized",
        |plan| {
            if !owns(plan, origin) {
                bail!("Request not owned by this origin")
            }
            crate::jobs::validate_retained_review(plan)?;
            if exact_receipt(plan, checkpoint)? {
                reused = true;
                return Ok(false);
            }
            if plan.status != PlanStatus::Review
                || plan.revision != e.source_revision
                || plan.planning.is_some()
            {
                bail!("Request state/revision changed before question checkpoint")
            }
            let context = plan
                .goal
                .request_context
                .as_ref()
                .context("Missing immutable origin")?;
            let draft = plan
                .request_review
                .as_mut()
                .context("Missing reviewed source")?;
            checkpoint.confirms_source(draft, context, capabilities)?;
            let mut proposal = draft.current().clone();
            proposal.needs_clarification = true;
            proposal.clarification = Some(question.clone());
            eefn::interpretation::validate(
                &draft.original_text,
                &serde_json::to_vec(&proposal)?,
                capabilities,
            )?;
            draft
                .proposals
                .push(eefn::request_drafts::ProposalRevision {
                    operation_id: checkpoint.operation_id.clone(),
                    proposal,
                    allowed_capabilities: capabilities.to_vec(),
                    at_ms: eefn::protocol::now_ms(),
                    resolution: None,
                    preparation: Some(checkpoint.source.clone()),
                });
            draft.validate()?;
            Ok(true)
        },
    )?;
    Ok((plan, reused))
}

impl crate::Runtime {
    pub async fn origin_preparation_checkpoint(
        &self,
        checkpoint: PreparationCheckpoint,
        context: &RequestContext,
    ) -> Result<Value> {
        checkpoint.validate()?;
        if checkpoint.operation_id != context.request_id()
            || checkpoint.source.request_context.origin_node() != context.origin_node()
        {
            bail!("Preparation receipt/origin does not match authenticated node")
        }
        let e = &checkpoint.source.evidence;
        let saved = self.engine.store.get(&e.job_id)?;
        if !owns(&saved, context.origin_node()) {
            bail!("Request unavailable for this origin")
        }
        crate::jobs::validate_retained_review(&saved)?;
        let value = if exact_receipt(&saved, &checkpoint)? {
            self.node_planning_view(&saved, context, true)?
        } else {
            if saved.status != PlanStatus::Review || saved.revision != e.source_revision {
                bail!("Preparation source revision changed")
            }
            let capabilities = self
                .disclosed_interpretation_capabilities(context.origin_node())
                .await?;
            checkpoint.confirms_source(
                saved
                    .request_review
                    .as_ref()
                    .context("Missing reviewed source")?,
                saved
                    .goal
                    .request_context
                    .as_ref()
                    .context("Missing immutable origin")?,
                &capabilities,
            )?;
            if let Some(input) = &checkpoint.proposal.method {
                let report = self
                    .planning_command_scoped(
                        crate::request_planning::PlanningRequest {
                            schema_version: 1,
                            expected_runtime_id: self.instance_id.clone(),
                            command: crate::request_planning::PlanningCommand::Compile {
                                id: e.job_id.clone(),
                                expected_revision: e.source_revision,
                                operation_id: checkpoint.operation_id.clone(),
                                input: input.clone(),
                            },
                        },
                        Some(context.origin_node()),
                        Some(&checkpoint.source),
                    )
                    .await?;
                let plan: Plan = serde_json::from_value(report["plan"].clone())?;
                self.node_planning_view(&plan, context, report["reused"] == true)?
            } else {
                let (plan, reused) = question_checkpoint(
                    &self.engine,
                    &checkpoint,
                    context.origin_node(),
                    &capabilities,
                )?;
                self.node_planning_view(&plan, context, reused)?
            }
        };
        if !eefn::request_preparation::confirmed_checkpoint(
            &value,
            context.origin_node(),
            &checkpoint,
        ) {
            bail!("Prepared checkpoint failed confirmation")
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::EventBus, jobs::JobStore};
    use eefn::request_preparation::{MethodProposal, PreparationEvidence, PreparationSource};
    use serde_json::json;

    fn question(saved: &Plan) -> PreparationCheckpoint {
        let draft = saved.request_review.as_ref().unwrap();
        PreparationCheckpoint {
            schema_version: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
            source: PreparationSource {
                evidence: PreparationEvidence {
                    schema_version: 1,
                    job_id: saved.plan_id.clone(),
                    source_revision: saved.revision,
                    source_sha256: eefn::grants::definition_digest(&json!(draft)).unwrap(),
                    source_proposal_count: draft.proposals.len(),
                    source_answer_count: draft.answers.len(),
                    backend: "ollama".into(),
                    model_id: "fixture".into(),
                },
                request_context: saved.goal.request_context.clone().unwrap(),
            },
            proposal: MethodProposal {
                schema_version: 1,
                method: None,
                clarification: Some("Which node?".into()),
            },
        }
    }

    #[tokio::test]
    async fn method_question_is_source_pinned_answerable_durable_and_exact_reuse_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let saved = crate::request_planning::tests::review(&engine);
        let checkpoint = question(&saved);
        let caps = vec!["system.info".into()];
        let (asked, reused) = question_checkpoint(&engine, &checkpoint, "owner", &caps).unwrap();
        assert!(!reused);
        assert_eq!(asked.revision, 2);
        assert!(asked.tasks.is_empty());
        assert_eq!(asked.status, PlanStatus::Review);
        assert!(asked.approval.is_none());
        assert!(asked.execution.is_none());
        assert_eq!(asked.goal.request_context, saved.goal.request_context);
        assert_eq!(
            asked.request_review.as_ref().unwrap().state(),
            "clarification_required"
        );
        assert!(engine.enqueue_request(asked.clone()).is_err());
        let before = json!(asked);
        assert!(
            question_checkpoint(&engine, &checkpoint, "owner", &[])
                .unwrap()
                .1
        );
        assert_eq!(json!(store.get(&saved.plan_id).unwrap()), before);
        for field in ["question", "model", "digest"] {
            let mut bad = checkpoint.clone();
            match field {
                "question" => bad.proposal.clarification = Some("Changed same receipt".into()),
                "model" => bad.source.evidence.model_id = "other".into(),
                "digest" => bad.source.evidence.source_sha256 = "a".repeat(64),
                _ => unreachable!(),
            }
            assert!(question_checkpoint(&engine, &bad, "owner", &caps).is_err());
        }
        let answer_context =
            RequestContext::new(uuid::Uuid::new_v4().to_string(), "owner".into(), vec![]).unwrap();
        crate::request_drafts::command(
            &engine,
            "requests.draft_answer",
            json!({"id":saved.plan_id,"expected_revision":2,"text":"Bedroom"}),
            &answer_context,
            &caps,
        )
        .unwrap();
        let answered = store.get(&saved.plan_id).unwrap();
        assert_eq!(answered.revision, 3);
        assert!(answered.request_review.as_ref().unwrap().resolvable());
        assert!(
            question_checkpoint(&engine, &checkpoint, "owner", &[])
                .unwrap()
                .1
        );
        let mut changed = json!(answered.clone());
        changed["request_review"]["proposals"][1]["proposal"]["goal"] = json!("Widened intent");
        let changed: Plan = serde_json::from_value(changed).unwrap();
        assert!(crate::jobs::validate_retained_review(&changed).is_err());
        let mut changed = answered.clone();
        changed.request_review.as_mut().unwrap().proposals[1]
            .preparation
            .as_mut()
            .unwrap()
            .evidence
            .job_id = uuid::Uuid::new_v4().simple().to_string();
        assert!(crate::jobs::validate_retained_review(&changed).is_err());
        engine.shutdown().await;
        drop(engine);
        drop(store);
        let reopened = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(
            json!(reopened.get(&saved.plan_id).unwrap()),
            json!(answered)
        );
        drop(reopened);
        let sql = rusqlite::Connection::open(&path).unwrap();
        let mut corrupted = json!(answered);
        corrupted["request_review"]["proposals"][1]["preparation"]["evidence"]["source_sha256"] =
            json!("b".repeat(64));
        let raw = serde_json::to_string(&corrupted).unwrap();
        sql.execute(
            "UPDATE durable_jobs SET plan_json=?2 WHERE job_id=?1",
            rusqlite::params![saved.plan_id, raw],
        )
        .unwrap();
        assert!(JobStore::open(&path, 500, 64 * 1024 * 1024).is_err());
        let retained: String = sql
            .query_row(
                "SELECT plan_json FROM durable_jobs WHERE job_id=?1",
                [&saved.plan_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            retained, raw,
            "damaged source must remain available for explicit recovery"
        );
    }

    #[tokio::test]
    async fn questions_reject_wrong_origin_source_revision_context_or_shutdown_without_changes() {
        let engine = TaskEngine::new(EventBus::new(20));
        let saved = crate::request_planning::tests::review(&engine);
        let checkpoint = question(&saved);
        let caps = vec!["system.info".into()];
        assert!(question_checkpoint(&engine, &checkpoint, "other", &caps).is_err());
        for field in [
            "digest",
            "revision",
            "count",
            "context",
            "authority",
            "method",
        ] {
            let mut bad = checkpoint.clone();
            match field {
                "digest" => bad.source.evidence.source_sha256 = "a".repeat(64),
                "revision" => bad.source.evidence.source_revision = 2,
                "count" => bad.source.evidence.source_answer_count = 1,
                "context" => {
                    bad.source.request_context = RequestContext::new(
                        saved
                            .goal
                            .request_context
                            .as_ref()
                            .unwrap()
                            .request_id()
                            .into(),
                        "owner".into(),
                        vec!["Elsewhere".into()],
                    )
                    .unwrap()
                }
                "authority" => bad.proposal.schema_version = 2,
                "method" => {
                    bad.proposal.method = Some(crate::request_planning::PlanInput {
                        schema_version: 1,
                        template: "system_info".into(),
                        params: json!({}),
                        constraints: json!({}),
                    })
                }
                _ => unreachable!(),
            }
            assert!(
                question_checkpoint(&engine, &bad, "owner", &caps).is_err(),
                "{field}"
            );
            assert_eq!(
                json!(engine.store.get(&saved.plan_id).unwrap()),
                json!(saved)
            );
        }
        engine.shutdown().await;
        assert!(question_checkpoint(&engine, &checkpoint, "owner", &caps).is_err());
        assert_eq!(
            json!(engine.store.get(&saved.plan_id).unwrap()),
            json!(saved)
        );
    }
}
