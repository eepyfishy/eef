//! Explicit one-step request admission and compilation in the existing journal.
use anyhow::{Context, Result, bail};
use eefn::{
    context::RequestContext,
    request_submission::{ConcreteCommand, ConcreteInput, ConcreteReport},
};
use serde_json::{Value, json};

fn matches_source(plan: &crate::task::Plan, input: &ConcreteInput) -> Result<()> {
    let draft = plan
        .request_review
        .as_ref()
        .context("Not an explicit request")?;
    if draft.original_text != input.original_text
        || draft.concrete_input.as_ref() != Some(&input.input)
        || draft.proposals.len() != 1
        || !draft.answers.is_empty()
        || draft.proposals[0].proposal != input.proposal()?
    {
        bail!("Explicit request receipt conflicts with immutable input/review")
    }
    Ok(())
}
impl crate::Runtime {
    pub async fn origin_concrete_request(
        &self,
        input: ConcreteInput,
        context: &RequestContext,
    ) -> Result<Value> {
        input.validate()?;
        eefn::job_commands::validate_operation_id(context.request_id())?;
        // Historical exact receipts remain usable without current model/provider
        // availability. A missing receipt is not proof of never-submitted work.
        let mut matches = self.engine.store.list()?.into_iter().filter(|p| {
            p.goal.request_context.as_ref().is_some_and(|c| {
                c.origin_node() == context.origin_node() && c.request_id() == context.request_id()
            })
        });
        let previous = matches.next();
        if matches.next().is_some() {
            bail!("Ambiguous retained request receipt")
        }
        let (saved, creation_reused) = if let Some(saved) = previous {
            crate::jobs::validate_retained_review(&saved)?;
            matches_source(&saved, &input)?;
            (saved, true)
        } else {
            let capabilities = self
                .disclosed_interpretation_capabilities(context.origin_node())
                .await?;
            let proposal = input.proposal()?;
            eefn::interpretation::validate_concrete(&input, &capabilities)?;
            let draft = eefn::request_drafts::DraftRecord {
                schema_version: 1,
                original_text: input.original_text.clone(),
                concrete_input: Some(input.input.clone()),
                proposals: vec![eefn::request_drafts::ProposalRevision {
                    operation_id: context.request_id().into(),
                    proposal,
                    allowed_capabilities: capabilities,
                    at_ms: eefn::protocol::now_ms(),
                    resolution: None,
                    preparation: None,
                }],
                answers: vec![],
            };
            let admission = crate::request_drafts::store_review(&self.engine, draft, context)?;
            (admission.plan, admission.reused)
        };
        let receipt = eefn::request_submission::compilation_receipt(
            context.origin_node(),
            context.request_id(),
            &saved.plan_id,
        )?;
        let command = eefn::request_planning::PlanningCommand::Compile {
            id: saved.plan_id.clone(),
            expected_revision: 1,
            operation_id: receipt.clone(),
            input: input.input.clone(),
        };
        let compile_context = RequestContext::new(
            receipt.clone(),
            context.origin_node().into(),
            context.origin_area().to_vec(),
        )?;
        let value = self
            .origin_planning_command(command, &compile_context)
            .await?;
        let plan = self.engine.store.get(&saved.plan_id)?;
        matches_source(&plan, &input)?;
        let result = json!(ConcreteReport {
            schema_version: 1,
            report_type: "coordinator_concrete_request".into(),
            operation_id: context.request_id().into(),
            compilation_operation_id: receipt,
            creation_reused,
            plan: serde_json::from_value(value)?,
            job: crate::jobs::view(&plan, true),
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false
        });
        if !eefn::request_submission::confirmed(
            &result,
            context.origin_node(),
            &ConcreteCommand {
                operation_id: context.request_id().into(),
                request: input,
            },
        ) {
            bail!(
                "Concrete request checkpoint changed or failed confirmation; inspect retained history"
            )
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::EventBus,
        task::{PlanStatus, TaskEngine},
    };
    fn input() -> ConcreteInput {
        ConcreteInput {
            schema_version: 1,
            original_text: "  Inspect this node\n".into(),
            title: "Explicit status".into(),
            input: eefn::request_planning::PlanInput {
                schema_version: 1,
                template: "system_info".into(),
                params: json!({}),
                constraints: json!({}),
            },
        }
    }
    fn draft(input: &ConcreteInput, context: &RequestContext) -> eefn::request_drafts::DraftRecord {
        eefn::request_drafts::DraftRecord {
            schema_version: 1,
            original_text: input.original_text.clone(),
            concrete_input: Some(input.input.clone()),
            proposals: vec![eefn::request_drafts::ProposalRevision {
                operation_id: context.request_id().into(),
                proposal: input.proposal().unwrap(),
                allowed_capabilities: vec!["system.info".into()],
                at_ms: 1,
                resolution: None,
                preparation: None,
            }],
            answers: vec![],
        }
    }
    #[tokio::test]
    async fn explicit_admission_keeps_original_input_and_deduplicates_without_a_runner_or_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "owner".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let input = input();
        let saved = crate::request_drafts::store_review(&engine, draft(&input, &context), &context)
            .unwrap();
        matches_source(&saved.plan, &input).unwrap();
        assert_eq!(saved.plan.status, PlanStatus::Review);
        assert_eq!(saved.plan.revision, 1);
        assert!(saved.plan.tasks.is_empty());
        assert!(!saved.reused);
        assert!(engine.enqueue_request(saved.plan.clone()).is_err());
        let moved = RequestContext::new(
            context.request_id().into(),
            "owner".into(),
            vec!["Moved".into()],
        )
        .unwrap();
        let reused =
            crate::request_drafts::store_review(&engine, draft(&input, &moved), &moved).unwrap();
        assert!(reused.reused);
        assert_eq!(json!(reused.plan), json!(saved.plan));
        let mut changed = input.clone();
        changed.input.constraints = json!({"node_id":"other"});
        assert!(
            crate::request_drafts::store_review(&engine, draft(&changed, &context), &context)
                .is_err()
        );
        assert!(matches_source(&saved.plan, &changed).is_err());
        engine.shutdown().await;
        drop(engine);
        drop(store);
        let reopened = crate::jobs::JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(
            json!(reopened.get(&saved.plan.plan_id).unwrap()),
            json!(saved.plan)
        );
    }
    #[test]
    fn explicit_peer_content_survives_review_without_authorizing_execution() {
        let engine = TaskEngine::new(EventBus::new(20));
        for version in [1, 2] {
            let mut transfer = json!({"schema_version":version,
                "source":{"node_id":"source","resource_id":"source::transport"},
                "destination":{"node_id":"sink","resource_id":"sink::transport"},
                "payload_bytes":3,"payload_sha256":"a".repeat(64),
                "limits":{"max_wire_bytes":1048576,"max_bytes_per_second":1048576,
                    "max_frame_bytes":4096,"max_frames":256,"max_inflight_frames":1}});
            if version == 2 {
                transfer.as_object_mut().unwrap().remove("payload_sha256");
                transfer["payload_bytes"] = json!(17026);
                transfer["audio"] = json!({"duration_ms":500,"sample_rate":16000,"channels":1,
                    "source_resource_id":"source::microphone",
                    "destination_resource_id":"sink::speaker"});
            }
            let mut input = input();
            input.input = serde_json::from_value(json!({"schema_version":1,
                "template":"peer_transfer","params":{"transfer":transfer},"constraints":{}}))
            .unwrap();
            let context =
                RequestContext::new(uuid::Uuid::new_v4().to_string(), "source".into(), vec![])
                    .unwrap();
            let mut review = draft(&input, &context);
            review.proposals[0].allowed_capabilities =
                eefn::request_planning::concrete_capabilities(&input.input).unwrap();
            let saved = crate::request_drafts::store_review(&engine, review, &context).unwrap();
            matches_source(&saved.plan, &input).unwrap();
            assert_eq!(saved.plan.status, PlanStatus::Review);
            assert!(saved.plan.approval.is_none());
            assert!(saved.plan.execution.is_none());
            assert!(saved.plan.tasks.is_empty());
            assert!(!eefn::request_planning::METHOD_TEMPLATES.contains(&"peer_transfer"));
            let mut hidden = saved.plan.request_review.unwrap();
            hidden.proposals[0].allowed_capabilities.clear();
            assert!(
                hidden.validate().is_err(),
                "explicit work still needs disclosed capabilities"
            );
        }
    }
    #[test]
    fn explicit_input_cannot_be_rebound_by_compilation_or_hidden_by_omitting_the_source() {
        let engine = TaskEngine::new(EventBus::new(20));
        let mut saved = crate::request_planning::tests::review(&engine);
        let mut compiled = crate::request_planning::tests::compiled(&saved);
        let input = compiled.planning.as_ref().unwrap().input.clone();
        compiled.request_review.as_mut().unwrap().concrete_input = Some(input);
        // Adding an explicit source changes the definition digest; cannot silently
        // relabel an already-compiled legacy source, even with matching inputs.
        assert!(crate::request_planning::validate_retained(&compiled).is_err());
        saved.request_review.as_mut().unwrap().concrete_input =
            Some(eefn::request_planning::PlanInput {
                schema_version: 1,
                template: "system_info".into(),
                params: json!({}),
                constraints: json!({"node_id":"other"}),
            });
        compiled.request_review = saved.request_review;
        assert!(crate::request_planning::validate_retained(&compiled).is_err());
    }
}
