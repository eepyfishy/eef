//! Coordinator-owned microphone selection and immutable authoring receipts.
use anyhow::{Context, Result, bail};
use eefn::{
    context::RequestContext,
    microphone_submission::{MicrophoneCommand, MicrophoneInput, MicrophoneReport},
};
use serde_json::{Value, json};
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
            let draft = saved
                .request_review
                .as_ref()
                .context("Not a retained microphone request")?;
            if draft.workload_source.as_ref() != Some(&input) {
                bail!("Microphone operation ID conflicts with immutable retained intent")
            }
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
            concrete
        } else {
            let source = input.source(context.origin_node());
            if !self
                .disclosed_planning_nodes(context.origin_node())
                .await?
                .contains(source)
            {
                bail!("Microphone source unavailable or not disclosed to this origin")
            }
            let snapshot = self.node_server.workload_sources(source, 5).await?;
            input.resolve(context.origin_node(), &snapshot)?
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
