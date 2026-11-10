//! Initial interpretation ownership in the existing job journal. Never a grant.
use crate::task::{Plan, PlanStatus};
use anyhow::{Context, Result, ensure};
use eefn::interpretation::Proposal;
pub use eefn::request_interpretation::{Claim, Record, State};
use serde_json::json;

pub(crate) enum Transition {
    Claim(Claim),
    Promote {
        claim: Claim,
        expected_revision: u64,
        proposal: Proposal,
    },
    /// Core-only source edit after a separate, explicit recovery decision.
    Recover {
        claim: Claim,
        expected_revision: u64,
        proposal: Proposal,
    },
    Interrupt,
}

pub(crate) fn checkpoint(plan: &Plan, reused: bool) -> Result<serde_json::Value> {
    let record = plan
        .input_interpretation
        .as_deref()
        .context("Missing interpretation checkpoint")?
        .clone();
    let report = eefn::request_interpretation::Checkpoint {
        schema_version: 1,
        report_type: "coordinator_input_interpretation".into(),
        success: true,
        id: plan.plan_id.clone(),
        revision: plan.revision,
        status: serde_json::to_value(plan.status)?
            .as_str()
            .context("Invalid request status")?
            .into(),
        operation_id: record.claim.operation_id.clone(),
        record: record.clone(),
        reused,
        draft: plan.request_review.clone(),
        inference_authorized: false,
        execution_authorized: false,
        dispatched: false,
    };
    report.validate(&record.claim, record.request_context.origin_node())?;
    Ok(json!(report))
}
fn local_binding(claim: &Claim, context: &eefn::context::RequestContext) -> Result<()> {
    ensure!(
        claim.operation_id == context.request_id(),
        "Interpretation receipt differs from authenticated operation"
    );
    let interpreter = claim
        .interpreter
        .as_ref()
        .context("Missing interpreter binding")?;
    ensure!(
        interpreter.node_id == context.origin_node()
            && interpreter.runtime_id == claim.origin_runtime_id,
        "Local interpretation must bind the originating node runtime; remote inference needs the broker grant path"
    );
    Ok(())
}
pub(crate) fn claim_command(
    engine: &crate::task::TaskEngine,
    payload: serde_json::Value,
    context: &eefn::context::RequestContext,
) -> Result<serde_json::Value> {
    let command: eefn::request_interpretation::ClaimCommand = serde_json::from_value(payload)?;
    command.validate()?;
    local_binding(&command.claim, context)?;
    let id = command.claim.id.clone();
    let (plan, changed) = engine.transition_input_interpretation(
        &id,
        Transition::Claim(command.claim),
        Some(context.origin_node()),
        &[],
    )?;
    checkpoint(&plan, !changed)
}
pub(crate) fn promote_command(
    engine: &crate::task::TaskEngine,
    payload: serde_json::Value,
    context: &eefn::context::RequestContext,
    capabilities: &[String],
) -> Result<serde_json::Value> {
    let command: eefn::request_interpretation::PromotionCommand = serde_json::from_value(payload)?;
    command.validate()?;
    local_binding(&command.claim, context)?;
    let id = command.claim.id.clone();
    let (plan, changed) = engine.transition_input_interpretation(
        &id,
        Transition::Promote {
            claim: command.claim,
            expected_revision: command.expected_revision,
            proposal: command.proposal,
        },
        Some(context.origin_node()),
        capabilities,
    )?;
    checkpoint(&plan, !changed)
}

/// Immutable source evidence survives later review, compilation and execution.
pub(crate) fn validate(plan: &Plan) -> Result<()> {
    let Some(record) = &plan.input_interpretation else {
        return Ok(());
    };
    record.validate_source(
        &plan.plan_id,
        plan.revision,
        plan.goal
            .request_context
            .as_ref()
            .context("Interpretation source origin missing")?,
    )?;
    record.claim.validate()?;
    record.input.validate()?;
    ensure!(
        record.schema_version == 1
            && record.claim.id == plan.plan_id
            && record.claim.expected_revision < plan.revision
            && record.claimed_at_ms > 0
            && plan.goal.request_context.as_ref() == Some(&record.request_context)
            && record.input.digest(&record.request_context)? == record.claim.source_sha256,
        "Interpretation source evidence is inconsistent"
    );
    if record.state == State::Promoted {
        let draft = plan
            .request_review
            .as_ref()
            .context("Promoted interpretation has no review")?;
        draft.validate()?;
        let proposal = &draft.proposals[0];
        ensure!(
            draft.original_text == record.input.original_text
                && draft.audio_source.is_none()
                && draft.workload_source.is_none()
                && draft.camera_source.is_none()
                && proposal.operation_id == record.request_context.request_id()
                && record.proposal_sha256.as_deref()
                    == Some(eefn::grants::definition_digest(&json!(proposal.proposal))?.as_str())
                && record
                    .promoted_revision
                    .is_some_and(|r| r > record.claim.expected_revision && r <= plan.revision),
            "Promoted interpretation lost its original source or proposal"
        );
    } else {
        ensure!(
            crate::request_intent::validate(plan)? == record.input,
            "Interpretation source differs from retained raw input"
        );
        ensure!(
            record.promoted_revision.is_none() && record.proposal_sha256.is_none(),
            "Unpromoted interpretation cannot carry completion evidence"
        );
    }
    Ok(())
}

/// Caller holds the existing journal gate. This records ownership, not model
/// authorization: an actual inference still needs its separate local consent or
/// normal policy-backed execution grant. No model is called by a transition.
pub(crate) fn apply(
    plan: &mut Plan,
    transition: Transition,
    origin: Option<&str>,
    capabilities: &[String],
) -> Result<bool> {
    let context = plan
        .goal
        .request_context
        .as_ref()
        .context("Input origin is missing")?
        .clone();
    ensure!(
        origin.is_none_or(|node| node == context.origin_node()),
        "Input is unavailable for this origin"
    );
    if matches!(transition, Transition::Interrupt) {
        ensure!(
            origin.is_none(),
            "Interpretation interruption is reserved for journal recovery"
        );
        let record = plan
            .input_interpretation
            .as_mut()
            .context("No interpretation attempt")?;
        if record.state != State::Claimed {
            return Ok(false);
        }
        record.state = State::Unknown;
        return Ok(true);
    }
    let recovering = matches!(&transition, Transition::Recover { .. });
    let (claim, promotion) = match transition {
        Transition::Claim(claim) => (claim, None),
        Transition::Promote {
            claim,
            expected_revision,
            proposal,
        } => (claim, Some((expected_revision, proposal))),
        Transition::Recover {
            claim,
            expected_revision,
            proposal,
        } => (claim, Some((expected_revision, proposal))),
        Transition::Interrupt => unreachable!(),
    };
    claim.validate()?;
    ensure!(
        claim.id == plan.plan_id,
        "Interpretation targets another input"
    );
    if let Some((revision, proposal)) = promotion {
        let record = plan
            .input_interpretation
            .as_ref()
            .context("Interpretation was not claimed")?;
        ensure!(
            record.claim == claim,
            "Interpretation claim differs from retained ownership"
        );
        let digest = eefn::grants::definition_digest(&json!(proposal))?;
        if record.state == State::Promoted {
            ensure!(
                record.proposal_sha256.as_deref() == Some(digest.as_str())
                    && record.promoted_revision.and_then(|r| r.checked_sub(1)) == Some(revision),
                "Interpretation completion conflicts with retained evidence"
            );
            return Ok(false);
        }
        ensure!(
            record.state
                == (if recovering {
                    State::Unknown
                } else {
                    State::Claimed
                })
                && plan.status == PlanStatus::Review
                && plan.revision == revision,
            "Interpretation is cancelled, uncertain or stale; no replay"
        );
        crate::request_review::review(
            json!({"schema_version":1,"original_text":record.input.original_text,"proposal":proposal}),
            &context,
            capabilities,
        )?;
        let draft = eefn::request_drafts::DraftRecord {
            schema_version: 1,
            original_text: record.input.original_text.clone(),
            concrete_input: None,
            audio_source: None,
            workload_source: None,
            camera_source: None,
            proposals: vec![eefn::request_drafts::ProposalRevision {
                operation_id: context.request_id().into(),
                proposal,
                allowed_capabilities: capabilities.to_vec(),
                at_ms: eefn::protocol::now_ms(),
                resolution: None,
                preparation: None,
            }],
            answers: vec![],
        };
        draft.validate()?;
        plan.goal.description = draft.proposals[0].proposal.goal.clone();
        plan.goal.template = "request_review".into();
        plan.goal.params = json!({});
        plan.request_review = Some(draft);
        let record = plan.input_interpretation.as_mut().unwrap();
        record.state = State::Promoted;
        record.promoted_revision = Some(
            plan.revision
                .checked_add(1)
                .context("Input revision overflow")?,
        );
        record.proposal_sha256 = Some(digest);
        return Ok(true);
    }
    if let Some(record) = &plan.input_interpretation {
        ensure!(
            record.claim == claim,
            "A different interpretation attempt already owns this input"
        );
        return Ok(false); // Inspection never licenses a second inference.
    }
    let input = crate::request_intent::validate(plan)?;
    ensure!(
        plan.status == PlanStatus::Review
            && plan.revision == claim.expected_revision
            && input.digest(&context)? == claim.source_sha256,
        "Input is cancelled or its source revision/digest changed"
    );
    plan.input_interpretation = Some(Box::new(Record {
        schema_version: 1,
        claim,
        input,
        request_context: context,
        claimed_at_ms: eefn::protocol::now_ms(),
        state: State::Claimed,
        promoted_revision: None,
        proposal_sha256: None,
    }));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::EventBus, jobs::JobStore, task::TaskEngine};
    use eefn::context::RequestContext;
    use std::sync::Arc;
    fn fixture(store: Arc<JobStore>) -> (Arc<TaskEngine>, Claim) {
        let engine = TaskEngine::with_store(EventBus::new(10), store);
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin-node".into(),
            vec!["home".into(), "bedroom".into()],
        )
        .unwrap();
        let saved = crate::request_intent::save(
            &engine,
            json!({"schema_version":1,"original_text":"Check this node"}),
            &context,
        )
        .unwrap();
        let claim = Claim {
            interpreter: None,
            id: saved["id"].as_str().unwrap().into(),
            expected_revision: saved["revision"].as_u64().unwrap(),
            source_sha256: saved["source_sha256"].as_str().unwrap().into(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            origin_runtime_id: uuid::Uuid::new_v4().to_string(),
        };
        (engine, claim)
    }
    fn proposal() -> Proposal {
        serde_json::from_value(json!({"schema_version":1,"intent":"information","goal":"Check status",
            "context_hints":["this node"],"constraints":[],"suggested_capabilities":["system.info"],
            "complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null})).unwrap()
    }
    #[test]
    fn wire_checkpoints_bind_authenticated_local_origin_without_inference_or_execution() {
        use eefn::request_interpretation::{
            Checkpoint, ClaimCommand, Interpreter, PromotionCommand,
        };
        let store = JobStore::in_memory().unwrap();
        let (engine, mut c) = fixture(store.clone());
        c.interpreter = Some(Interpreter {
            node_id: "origin-node".into(),
            backend: "ollama".into(),
            model_id: "selected".into(),
            runtime_id: c.origin_runtime_id.clone(),
        });
        let context = RequestContext::new(
            c.operation_id.clone(),
            "origin-node".into(),
            vec!["moved".into()],
        )
        .unwrap();
        let command = ClaimCommand {
            schema_version: 1,
            claim: c.clone(),
        };
        for mutation in 0..3 {
            let mut invalid = command.clone();
            match mutation {
                0 => invalid.claim.operation_id = uuid::Uuid::new_v4().to_string(),
                1 => invalid.claim.interpreter.as_mut().unwrap().node_id = "other-node".into(),
                _ => {
                    invalid.claim.interpreter.as_mut().unwrap().runtime_id =
                        uuid::Uuid::new_v4().to_string()
                }
            }
            assert!(claim_command(&engine, json!(invalid), &context).is_err());
        }
        assert!(store.get(&c.id).unwrap().input_interpretation.is_none());
        let claimed: Checkpoint =
            serde_json::from_value(claim_command(&engine, json!(command), &context).unwrap())
                .unwrap();
        claimed.validate(&c, "origin-node").unwrap();
        assert!(!claimed.reused);
        assert_eq!(
            claimed.record.request_context.origin_area().to_vec(),
            vec!["home".to_string(), "bedroom".to_string()]
        );
        let promotion = PromotionCommand {
            schema_version: 1,
            claim: c.clone(),
            expected_revision: claimed.revision,
            proposal: proposal(),
        };
        assert!(promote_command(&engine, json!(promotion), &context, &[]).is_err());
        let promoted: Checkpoint = serde_json::from_value(
            promote_command(&engine, json!(promotion), &context, &["system.info".into()]).unwrap(),
        )
        .unwrap();
        promoted.validate(&c, "origin-node").unwrap();
        assert_eq!(promoted.record.state, State::Promoted);
        let row = store.get(&c.id).unwrap();
        assert!(row.tasks.is_empty() && row.execution.is_none() && row.approval.is_none());
    }
    fn claim(store: &JobStore, c: &Claim) -> (Plan, bool) {
        store
            .input_interpretation_transition(
                &c.id,
                Transition::Claim(c.clone()),
                Some("origin-node"),
                &[],
            )
            .unwrap()
    }
    fn promote(store: &JobStore, c: &Claim, revision: u64) -> Result<(Plan, bool)> {
        store.input_interpretation_transition(
            &c.id,
            Transition::Promote {
                claim: c.clone(),
                expected_revision: revision,
                proposal: proposal(),
            },
            Some("origin-node"),
            &["system.info".into()],
        )
    }
    fn disk() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        (dir, path)
    }
    #[test]
    fn claim_is_durable_ownership_not_a_proposal_or_execution_and_reuse_is_read_only() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, c) = fixture(store.clone());
        let (first, changed) = claim(&store, &c);
        assert!(changed);
        validate(&first).unwrap();
        assert_eq!(
            first.input_interpretation.as_ref().unwrap().state,
            State::Claimed
        );
        assert!(
            first.request_review.is_none()
                && first.planning.is_none()
                && first.approval.is_none()
                && first.execution.is_none()
        );
        assert!(first.tasks.is_empty());
        let before = json!(first);
        let (again, changed) = claim(&store, &c);
        assert!(!changed);
        assert_eq!(json!(again), before);
        for mutation in 0..3 {
            let mut other = c.clone();
            match mutation {
                0 => other.operation_id = uuid::Uuid::new_v4().to_string(),
                1 => other.origin_runtime_id = uuid::Uuid::new_v4().to_string(),
                _ => other.source_sha256 = "0".repeat(64),
            }
            assert!(
                store
                    .input_interpretation_transition(
                        &c.id,
                        Transition::Claim(other),
                        Some("origin-node"),
                        &[]
                    )
                    .is_err()
            );
        }
        assert_eq!(json!(store.get(&c.id).unwrap()), before);
    }
    #[test]
    fn promotion_retains_identity_origin_source_and_first_untrusted_proposal_without_authority() {
        let (_dir, path) = disk();
        let store = JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
        let (engine, c) = fixture(store.clone());
        let (claimed, _) = claim(&store, &c);
        let (promoted, changed) = promote(&store, &c, claimed.revision).unwrap();
        assert!(changed);
        validate(&promoted).unwrap();
        assert_eq!(promoted.plan_id, c.id);
        assert_eq!(promoted.goal.request_context, claimed.goal.request_context);
        assert_eq!(
            promoted
                .input_interpretation
                .as_ref()
                .unwrap()
                .claim
                .source_sha256,
            c.source_sha256
        );
        assert_eq!(
            promoted.request_review.as_ref().unwrap().original_text,
            "Check this node"
        );
        assert!(
            promoted.planning.is_none()
                && promoted.approval.is_none()
                && promoted.execution.is_none()
                && promoted.tasks.is_empty()
        );
        assert_eq!(promoted.status, PlanStatus::Review);
        let before = json!(promoted);
        let (again, changed) = promote(&store, &c, claimed.revision).unwrap();
        assert!(!changed);
        assert_eq!(json!(again), before);
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "origin-node".into(),
            vec![],
        )
        .unwrap();
        let inspected = crate::request_drafts::command(
            &engine,
            "requests.draft_get",
            json!({"id":c.id}),
            &context,
            &["system.info".into()],
        )
        .unwrap();
        assert_eq!(inspected["id"], c.id);
        assert_eq!(inspected["execution_authorized"], false);
        assert!(engine.resume_job(&c.id).is_err());
        let mut changed_proposal = proposal();
        changed_proposal.goal = "Changed output".into();
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Promote {
                        claim: c.clone(),
                        expected_revision: claimed.revision,
                        proposal: changed_proposal
                    },
                    Some("origin-node"),
                    &["system.info".into()]
                )
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), before);
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
        let retained = store.get(&c.id).unwrap();
        assert_eq!(
            retained.input_interpretation.as_ref().unwrap().state,
            State::Promoted
        );
        assert_eq!(json!(retained), before);
    }
    #[test]
    fn restart_retires_claim_to_unknown_even_under_lowered_quota_without_replaying() {
        let (_dir, path) = disk();
        let c = {
            let store = JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
            let (_engine, c) = fixture(store.clone());
            claim(&store, &c);
            c
        };
        let store = JobStore::open(&path, 10, 1024).unwrap();
        let retained = store.get(&c.id).unwrap();
        assert_eq!(store.usage().unwrap()["reserved_bytes"], 1024);
        assert_eq!(
            retained.input_interpretation.as_ref().unwrap().state,
            State::Unknown
        );
        assert!(retained.request_review.is_none() && retained.tasks.is_empty());
        let (same, changed) = claim(&store, &c);
        assert!(!changed);
        assert_eq!(same.revision, retained.revision);
        assert!(promote(&store, &c, retained.revision).is_err());
        let engine = TaskEngine::with_store(EventBus::new(10), store.clone());
        assert_eq!(engine.cancel(&c.id).unwrap().status, PlanStatus::Cancelled);
        assert_eq!(store.usage().unwrap()["reserved_bytes"], 0);
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 10, 1024).unwrap();
        assert_eq!(store.get(&c.id).unwrap().status, PlanStatus::Cancelled);
    }
    #[test]
    fn wrong_origin_source_runtime_revision_and_undisclosed_capability_do_not_mutate() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, c) = fixture(store.clone());
        let initial = json!(store.get(&c.id).unwrap());
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Claim(c.clone()),
                    Some("stranger"),
                    &[]
                )
                .is_err()
        );
        let mut stale = c.clone();
        stale.expected_revision += 1;
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Claim(stale),
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        let mut wrong = c.clone();
        wrong.source_sha256 = "0".repeat(64);
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Claim(wrong),
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), initial);
        let (claimed, _) = claim(&store, &c);
        let before = json!(claimed);
        let mut inconsistent = claimed.clone();
        let evidence = inconsistent.input_interpretation.as_mut().unwrap();
        evidence.input.original_text = "A different source".into();
        evidence.claim.source_sha256 = evidence.input.digest(&evidence.request_context).unwrap();
        assert!(validate(&inconsistent).is_err());
        let mut changed = c.clone();
        changed.origin_runtime_id = uuid::Uuid::new_v4().to_string();
        assert!(promote(&store, &changed, claimed.revision).is_err());
        assert!(promote(&store, &c, claimed.revision + 1).is_err());
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Promote {
                        claim: c.clone(),
                        expected_revision: claimed.revision,
                        proposal: proposal()
                    },
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), before);
    }
    #[test]
    fn ordinary_edits_cannot_claim_erase_or_rewrite_interpretation_evidence() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, c) = fixture(store.clone());
        let mut candidate = store.get(&c.id).unwrap();
        apply(
            &mut candidate,
            Transition::Claim(c.clone()),
            Some("origin-node"),
            &[],
        )
        .unwrap();
        assert!(
            store
                .change(&c.id, "Cannot claim through ordinary editing", |p| {
                    p.input_interpretation = candidate.input_interpretation;
                    Ok(())
                })
                .is_err()
        );
        assert!(store.get(&c.id).unwrap().input_interpretation.is_none());
        let (claimed, _) = claim(&store, &c);
        let before = json!(claimed);
        assert!(
            store
                .change(&c.id, "Cannot erase", |p| {
                    p.input_interpretation = None;
                    Ok(())
                })
                .is_err()
        );
        assert!(
            store
                .change(&c.id, "Cannot pretend completion", |p| {
                    p.input_interpretation.as_mut().unwrap().state = State::Promoted;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), before);
        let (promoted, _) = promote(&store, &c, claimed.revision).unwrap();
        assert!(
            store
                .change(&c.id, "Cannot change source", |p| {
                    p.input_interpretation.as_mut().unwrap().input.original_text = "Changed".into();
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), json!(promoted));
    }
    #[tokio::test]
    async fn cancelled_input_or_insufficient_storage_cannot_begin_interpretation() {
        let store = JobStore::in_memory().unwrap();
        let (engine, c) = fixture(store);
        engine.shutdown().await;
        assert!(
            engine
                .transition_input_interpretation(
                    &c.id,
                    Transition::Claim(c.clone()),
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        let store = JobStore::in_memory().unwrap();
        let (engine, c) = fixture(store.clone());
        engine.cancel(&c.id).unwrap();
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Claim(c.clone()),
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        let store = JobStore::in_memory().unwrap();
        let (engine, c) = fixture(store.clone());
        let (claimed, _) = claim(&store, &c);
        engine.cancel(&c.id).unwrap();
        assert!(promote(&store, &c, claimed.revision).is_err());
        let (_dir, path) = disk();
        let c = {
            let store = JobStore::open(&path, 10, 2 * 1024 * 1024).unwrap();
            let (_engine, c) = fixture(store);
            c
        };
        let store = JobStore::open(&path, 10, 1024).unwrap();
        let before = json!(store.get(&c.id).unwrap());
        assert!(
            store
                .input_interpretation_transition(
                    &c.id,
                    Transition::Claim(c.clone()),
                    Some("origin-node"),
                    &[]
                )
                .is_err()
        );
        assert_eq!(json!(store.get(&c.id).unwrap()), before);
    }
}
