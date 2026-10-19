//! Deterministic resource authoring in the existing durable review/job journal.
use anyhow::{Context, Result, bail};
use eefn::{
    audio_submission::{AudioCommand, AudioInput, AudioReport},
    context::RequestContext,
};
use serde_json::{Value, json};

impl crate::Runtime {
    pub async fn origin_audio_request(
        &self,
        input: AudioInput,
        context: &RequestContext,
    ) -> Result<Value> {
        input.validate()?;
        eefn::job_commands::validate_operation_id(context.request_id())?;
        let mut previous = self.engine.store.list()?.into_iter().filter(|p| {
            p.goal.request_context.as_ref().is_some_and(|c| {
                c.origin_node() == context.origin_node() && c.request_id() == context.request_id()
            })
        });
        let saved = previous.next();
        if previous.next().is_some() {
            bail!("Ambiguous retained audio request receipt")
        }
        // Retained intent/choices are checked BEFORE any current provider lookup.
        let concrete = if let Some(saved) = saved {
            crate::jobs::validate_retained_review(&saved)?;
            let draft = saved
                .request_review
                .as_ref()
                .context("Not a retained audio source")?;
            if draft.audio_source.as_ref() != Some(&input) {
                bail!("Audio operation ID conflicts with retained immutable intent")
            }
            let concrete = draft
                .concrete_input
                .as_ref()
                .context("Retained audio bindings unavailable")?;
            let pair = eefn::peer::plan::Pair::from_params(&concrete.params)?;
            let expected = input.concrete(&pair, Some(context.origin_node()))?;
            if expected.input != *concrete {
                bail!("Retained audio choices changed")
            }
            expected
        } else {
            let source = input.source(context.origin_node());
            if source == input.destination_node {
                bail!("Peer audio requires distinct nodes")
            }
            let source = self
                .disclosed_audio_endpoint(context.origin_node(), source)
                .await?;
            let destination = self
                .disclosed_audio_endpoint(context.origin_node(), &input.destination_node)
                .await?;
            input.resolve(context.origin_node(), &source, &destination)?
        };
        let result = self
            .origin_concrete_request_with_source(concrete.clone(), context, Some(input.clone()))
            .await?;
        let report = json!(AudioReport {
            schema_version: 1,
            report_type: "coordinator_audio_request".into(),
            operation_id: context.request_id().into(),
            input: input.clone(),
            concrete,
            result,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false
        });
        if !eefn::audio_submission::confirmed(
            &report,
            context.origin_node(),
            &AudioCommand {
                operation_id: context.request_id().into(),
                input,
            },
        ) {
            bail!("Audio authoring checkpoint failed confirmation; inspect retained history")
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::EventBus, task::TaskEngine};
    use eefn::audio_submission::{EndpointResources, ResourceChoice};
    fn source() -> (AudioInput, eefn::request_submission::ConcreteInput) {
        let input: AudioInput = serde_json::from_value(
            json!({"schema_version":1,"original_text":"Author finite audio","title":"Finite audio",
            "destination_node":"destination","duration_ms":500,"sample_rate":16000,"channels":1}),
        )
        .unwrap();
        let endpoint = |node: &str, cap: &str, id: &str| EndpointResources {
            node_id: node.into(),
            finite_audio: true,
            can_receive: node == "destination",
            resources: vec![
                ResourceChoice {
                    id: format!("{node}::transport"),
                    capability: "peer.transfer".into(),
                    available: true,
                },
                ResourceChoice {
                    id: format!("{node}::{id}"),
                    capability: cap.into(),
                    available: true,
                },
            ],
        };
        let concrete = input
            .resolve(
                "source",
                &endpoint("source", "audio.capture", "mic"),
                &endpoint("destination", "audio.play", "speaker"),
            )
            .unwrap();
        (input, concrete)
    }
    fn draft(
        input: AudioInput,
        concrete: &eefn::request_submission::ConcreteInput,
        context: &RequestContext,
    ) -> eefn::request_drafts::DraftRecord {
        eefn::request_drafts::DraftRecord {
            schema_version: 1,
            original_text: concrete.original_text.clone(),
            concrete_input: Some(concrete.input.clone()),
            audio_source: Some(input),
            proposals: vec![eefn::request_drafts::ProposalRevision {
                operation_id: context.request_id().into(),
                proposal: concrete.proposal().unwrap(),
                allowed_capabilities: eefn::request_planning::concrete_capabilities(
                    &concrete.input,
                )
                .unwrap(),
                at_ms: 1,
                resolution: None,
                preparation: None,
            }],
            answers: vec![],
        }
    }
    #[test]
    fn audio_admission_deduplicates_exact_intent_without_reselecting_or_erasing_provenance() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (input, concrete) = source();
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "source".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let review = draft(input.clone(), &concrete, &context);
        let saved = crate::request_drafts::store_review(&engine, review.clone(), &context).unwrap();
        let before = json!(saved.plan);
        let moved = RequestContext::new(
            context.request_id().into(),
            "source".into(),
            vec!["Elsewhere".into()],
        )
        .unwrap();
        let reused = crate::request_drafts::store_review(&engine, review.clone(), &moved).unwrap();
        assert!(reused.reused);
        assert_eq!(json!(reused.plan), before);
        let mut different = review.clone();
        different.audio_source.as_mut().unwrap().source_node = Some("source".into());
        different.validate().unwrap();
        assert!(crate::request_drafts::store_review(&engine, different, &context).is_err());
        let mut different = review.clone();
        different.audio_source = None;
        different.validate().unwrap();
        assert!(crate::request_drafts::store_review(&engine, different, &context).is_err());
        let mut different = review;
        different.concrete_input.as_mut().unwrap().params["transfer"]["audio"]["source_resource_id"] =
            json!("source::new-microphone");
        different.validate().unwrap();
        assert!(crate::request_drafts::store_review(&engine, different, &context).is_err());
        assert_eq!(
            json!(engine.store.get(&saved.plan.plan_id).unwrap()),
            before
        );
        assert_eq!(engine.store.list().unwrap().len(), 1);
        assert!(saved.plan.execution.is_none());
        assert!(saved.plan.approval.is_none());
        assert!(saved.plan.tasks.is_empty());
    }
    #[test]
    fn audio_journal_edits_cannot_remove_source_change_resources_or_move_origin() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (input, concrete) = source();
        let context = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "source".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let review = draft(input, &concrete, &context);
        let saved = crate::request_drafts::store_review(&engine, review.clone(), &context).unwrap();
        let before = json!(saved.plan);
        for change in 0..3 {
            assert!(
                engine
                    .store
                    .edit(
                        &saved.plan.plan_id,
                        "must not replace authored choices",
                        |plan| {
                            match change {
                                0 => plan.request_review.as_mut().unwrap().audio_source = None,
                                1 => {
                                    plan.request_review
                                        .as_mut()
                                        .unwrap()
                                        .concrete_input
                                        .as_mut()
                                        .unwrap()
                                        .params["transfer"]["audio"]["source_resource_id"] =
                                        json!("source::replacement")
                                }
                                _ => {
                                    plan.goal.request_context = Some(RequestContext::new(
                                        context.request_id().into(),
                                        "source".into(),
                                        vec!["Changed".into()],
                                    )?)
                                }
                            }
                            Ok(true)
                        }
                    )
                    .is_err()
            );
            assert_eq!(
                json!(engine.store.get(&saved.plan.plan_id).unwrap()),
                before
            );
        }
        let other = RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "other-origin".into(),
            vec![],
        )
        .unwrap();
        let mut rebound = review;
        rebound.proposals[0].operation_id = other.request_id().into();
        assert!(
            crate::request_drafts::store_review(&engine, rebound, &other).is_err(),
            "default source is the original authenticated origin, not arbitrary endpoint metadata"
        );
    }
}
