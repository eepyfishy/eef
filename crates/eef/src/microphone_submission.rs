//! Coordinator-owned microphone selection and immutable authoring receipts.
use anyhow::{Context, Result, bail};
use eefn::microphone_submission::outcomes::{self, RefusalReason};
use eefn::{
    context::RequestContext,
    microphone_submission::{MicrophoneCommand, MicrophoneInput, MicrophoneReport},
};
use serde_json::{Value, json};
fn refuse(
    input: &MicrophoneInput,
    context: &RequestContext,
    reason: RefusalReason,
) -> Result<Value> {
    outcomes::refused(
        context.origin_node(),
        &MicrophoneCommand {
            operation_id: context.request_id().into(),
            input: input.clone(),
        },
        reason,
    )
}
impl crate::Runtime {
    pub async fn origin_microphone_request(
        &self,
        input: MicrophoneInput,
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
            bail!("Ambiguous retained microphone request receipt")
        }
        // Inspect exact retained intent/bindings BEFORE any provider lookup.
        let concrete = if let Some(saved) = saved {
            crate::jobs::validate_retained_review(&saved)?;
            if saved
                .request_review
                .as_ref()
                .and_then(|d| d.workload_source.as_ref())
                != Some(&input)
            {
                return refuse(&input, context, RefusalReason::IntentConflict);
            }
            let draft = saved.request_review.as_ref().unwrap();
            let retained = draft
                .concrete_input
                .as_ref()
                .context("Retained microphone bindings unavailable")?;
            let template =
                eefn::workload_leases::definition::Template::from_params(&retained.params)?;
            let concrete = input.concrete(&template, Some(context.origin_node()))?;
            if concrete.input != *retained {
                bail!("Retained microphone choices changed")
            }
            if saved.planning.is_none() {
                return refuse(&input, context, RefusalReason::PreparationIncomplete);
            }
            concrete
        } else {
            let source = input.source(context.origin_node());
            let permitted = match self.disclosed_planning_nodes(context.origin_node()).await {
                Ok(nodes) => nodes,
                Err(_) => return refuse(&input, context, RefusalReason::SourceUnavailable),
            };
            if !permitted.contains(source) {
                return refuse(
                    &input,
                    context,
                    RefusalReason::SourceUnavailableOrNotDisclosed,
                );
            }
            let snapshot = match self.node_server.workload_sources(source, 5).await {
                Ok(snapshot) => snapshot,
                Err(_) => return refuse(&input, context, RefusalReason::SourceUnavailable),
            };
            match input.resolve(context.origin_node(), &snapshot) {
                Ok(concrete) => concrete,
                Err(error) => {
                    return refuse(
                        &input,
                        context,
                        error
                            .downcast_ref::<RefusalReason>()
                            .copied()
                            .unwrap_or(RefusalReason::SourceUnavailable),
                    );
                }
            }
        };
        let result = self
            .origin_concrete_request_with_sources(
                concrete.clone(),
                context,
                None,
                Some(input.clone()),
            )
            .await?;
        let report = json!(MicrophoneReport {
            schema_version: 1,
            report_type: "coordinator_microphone_request".into(),
            operation_id: context.request_id().into(),
            input: input.clone(),
            concrete,
            result,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false
        });
        if !eefn::microphone_submission::confirmed(
            &report,
            context.origin_node(),
            &MicrophoneCommand {
                operation_id: context.request_id().into(),
                input,
            },
        ) {
            bail!("Microphone authoring checkpoint failed confirmation; inspect retained history")
        }
        Ok(report)
    }
}
