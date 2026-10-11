//! Owner adapter-independent reservation service; uses existing jobs and driver.
use crate::Runtime;
use anyhow::{Context, Result, bail};
use eefn::automation_occurrence::{
    OccurrenceCommand, OccurrenceReport, OccurrenceRequest, Reservation,
};
use serde_json::{Value, json};

impl Runtime {
    pub async fn automation_occurrence_command(&self, request: OccurrenceRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Occurrence schema/coordinator runtime changed");
        }
        let command = &request.command;
        command.validate()?;
        let mut source = None;
        if let OccurrenceCommand::Reserve { .. } = command {
            let (retained, plan) = self.engine.store.occurrence_source(command)?;
            source = plan;
            if let Some(record) = retained {
                return Ok(report(
                    &self.instance_id,
                    command,
                    Some(record),
                    vec![],
                    None,
                    true,
                ));
            }
        }
        let value = if let OccurrenceCommand::Reserve {
            job: _,
            revision,
            plan_sha256,
            policy,
            ..
        } = command
        {
            let plan = source.context("Reservation source missing")?;
            if plan.revision != *revision
                || plan.status != crate::task::PlanStatus::AwaitingApproval
                || plan
                    .planning
                    .as_ref()
                    .is_none_or(|p| p.plan_sha256 != *plan_sha256)
            {
                bail!("Reservation names an exact independently compiled awaiting job");
            }
            let policy = self
                .engine
                .store
                .automation_authorization(policy)?
                .context("Policy missing")?;
            let limits = policy.creation.limits().context("Policy limits missing")?;
            let driver = crate::request_execution::Driver::new(self);
            for index in 0..plan.tasks.len() {
                let session = driver.session(&plan, index).await?;
                let bounds: eefn::grants::GrantPolicy =
                    serde_json::from_value(session["limits"].clone())?;
                bounds.validate()?;
                if session["issuer_id"] != policy.issuer_id
                    || limits.max_execution_ms > bounds.max_duration_ms
                    || limits.max_result_bytes > bounds.max_result_bytes
                {
                    bail!("Exact occurrence issuer/providers/grant bounds are unavailable");
                }
            }
            self.engine
                .reserve_occurrence(command.clone(), &self.instance_id, &policy.issuer_id)?
        } else if matches!(command, OccurrenceCommand::Archive { .. }) {
            self.engine
                .archive_occurrence(command.clone(), &self.instance_id)?
        } else {
            self.engine
                .store
                .occurrence_checkpoint_now(command.clone(), &self.instance_id, None)?
        };
        if !eefn::automation_occurrence::confirmed(&value, command, &self.instance_id) {
            bail!(
                "Reservation confirmation failed; inspect exact receipt, never replace or retry automatically"
            );
        }
        Ok(value)
    }
}
pub(crate) fn report(
    runtime: &str,
    command: &OccurrenceCommand,
    reservation: Option<Reservation>,
    items: Vec<Value>,
    next_after: Option<String>,
    reused: bool,
) -> Value {
    json!(OccurrenceReport {
        schema_version: 1,
        report_type: "automation_occurrence".into(),
        runtime_id: runtime.into(),
        command: command.clone(),
        reservation,
        archive: None,
        items,
        next_after,
        reused,
        execution_requested: false,
        timers_started: false,
        automatically_retried: false
    })
}
