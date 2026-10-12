//! Fresh trigger admission through the existing compiler/reservation/grant driver.
//! The bounded timer owns triggering; there is no independent workload runner.
use crate::{
    Runtime,
    jobs::automation_schedules::{State, Trigger},
};
use anyhow::{Context, Result, bail};
use eefn::{
    automation_occurrence::{OccurrenceCommand, OccurrenceRequest},
    request_submission::ConcreteInput,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

impl Runtime {
    pub(crate) async fn launch_automation_trigger(&self, id: &str) -> Result<Value> {
        let t = self.engine.store.trigger_get(id)?;
        let schedule = self.engine.store.schedule_get(&t.schedule_id)?;
        if schedule.launch_started_at_ms.is_some()
            || schedule.active_trigger.as_deref() != Some(id)
            || schedule.state != State::InFlight
            || schedule.runtime_id != self.instance_id
        {
            return self.trigger_launch_inspection(&t, true);
        }
        let outcome = self.launch_fresh_trigger(&t).await;
        let launched = match outcome {
            Ok(launched) => launched,
            Err(error) => {
                // Do not turn a lost/partial local checkpoint into a new receipt or
                // continuation. Already-issued work retains its native lease/result
                // lifecycle. Failed interruption is also an error, not rollback proof.
                tracing::warn!(trigger_id=%t.id,%error,"Trigger launch needs inspection; no replay");
                if !self
                    .engine
                    .store
                    .trigger_record_refusal(&t.id, &self.instance_id)?
                {
                    self.engine
                        .store
                        .trigger_interrupt_now(&t.id, "launch_failed")?;
                }
                return Err(error);
            }
        };
        self.trigger_launch_inspection(&t, !launched)
    }
    async fn launch_fresh_trigger(&self, t: &Trigger) -> Result<bool> {
        let schedule = self.engine.store.schedule_get(&t.schedule_id)?;
        if schedule.launch_started_at_ms.is_some() {
            return Ok(false);
        }
        if schedule.runtime_id != self.instance_id
            || schedule.state != State::InFlight
            || schedule.active_trigger.as_deref() != Some(&t.id)
        {
            bail!("Trigger no longer owns fresh admission authority");
        }
        let policy = self
            .engine
            .store
            .automation_authorization(&schedule.creation.policy)?
            .context("Policy missing")?;
        let value = self.engine.store.automation_command(
            eefn::automation::AutomationCommand::Show {
                id: schedule.definition_id.clone(),
            },
            None,
        )?;
        let definition: eefn::automation::Definition =
            serde_json::from_value(value["definition"].clone())?;
        let source: crate::task::Plan =
            serde_json::from_value(definition.source.context("Definition removed")?)?;
        // This is a new explicit owner-directed request, not reuse of source LM
        // interpretation, preparation, task IDs, approval or execution evidence.
        let input = ConcreteInput {
            schema_version: 1,
            original_text: format!(
                "Owner-scheduled fixed input for definition {}",
                schedule.definition_id
            ),
            title: source.goal.description.clone(),
            input: policy.scope.input.clone(),
        };
        input.validate()?;
        let (_, age) = self
            .node_server
            .peer_records(&BTreeSet::from([t
                .request_context
                .origin_node()
                .to_owned()]))
            .await?
            .pop()
            .context("Origin node not registered")?;
        if age.as_millis() > u128::from(self.registry.scheduling_policy().freshness_seconds) * 1000
        {
            bail!("Origin node is stale");
        }
        let snapshot = self
            .node_server
            .node_state(t.request_context.origin_node())
            .await
            .context("Origin node disconnected")?;
        let metadata: eefn::context::NodeMetadata = serde_json::from_value(
            snapshot
                .registration
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        if metadata.area != t.request_context.origin_area() {
            bail!("Origin area changed; fixed schedule context cannot be retagged");
        }
        let driver = crate::request_execution::Driver::new(self);
        for index in 0..source.tasks.len() {
            let session = driver.session(&source, index).await?;
            let bounds: eefn::grants::GrantPolicy =
                serde_json::from_value(session["limits"].clone())?;
            bounds.validate()?;
            let limits = policy.creation.limits().context("Policy limits missing")?;
            if session["issuer_id"] != policy.issuer_id
                || limits.max_execution_ms > bounds.max_duration_ms
                || limits.max_result_bytes > bounds.max_result_bytes
            {
                bail!("Exact approved providers/issuer/grant bounds changed");
            }
        }
        if !self.engine.begin_trigger_launch(&t.id, &self.instance_id)? {
            return Ok(false);
        };
        let submitted = self
            .origin_concrete_request(input, &t.request_context)
            .await?;
        if submitted["creation_reused"] != false {
            bail!("Existing source receipt cannot trigger implicit continuation");
        }
        let id = submitted["plan"]["id"]
            .as_str()
            .context("Fresh compiled job missing")?;
        let compiled = self.engine.store.get(id)?;
        let reserved = self
            .automation_occurrence_command(OccurrenceRequest {
                schema_version: 1,
                expected_runtime_id: self.instance_id.clone(),
                command: OccurrenceCommand::Reserve {
                    policy: policy.id.clone(),
                    policy_sha256: policy.authorization_sha256.clone(),
                    job: id.into(),
                    revision: compiled.revision,
                    plan_sha256: compiled
                        .planning
                        .as_ref()
                        .context("Fresh compilation missing")?
                        .plan_sha256
                        .clone(),
                    operation_id: t.reservation_id.clone(),
                },
            })
            .await?;
        if reserved["reused"] != false {
            bail!("Existing reservation cannot trigger implicit continuation");
        }
        let approved = self.engine.store.get(id)?;
        self.execute_reviewed(
            eefn::request_execution::ExecuteCommand {
                id: id.into(),
                expected_revision: approved.revision,
                operation_id: t.execution_id.clone(),
                approval_sha256: approved
                    .approval
                    .as_ref()
                    .context("Delegated approval missing")?
                    .approval_sha256
                    .clone(),
            },
            Some(t.request_context.origin_node()),
        )
        .await?;
        Ok(true)
    }
    fn trigger_launch_inspection(&self, t: &Trigger, reused: bool) -> Result<Value> {
        let job = self.engine.store.trigger_job(&t.id)?;
        let schedule = self.engine.store.schedule_get(&t.schedule_id)?;
        Ok(
            json!({"schema_version":1,"report_type":"automation_trigger_launch","runtime_id":self.instance_id,
            "schedule_id":t.schedule_id,"trigger_id":t.id,"request_operation_id":t.request_context.request_id(),
            "reservation_id":t.reservation_id,"execution_operation_id":t.execution_id,"launch_reused":reused,
            "launch_started_at_ms":if schedule.active_trigger.as_deref()==Some(&t.id) {schedule.launch_started_at_ms} else {None},"schedule_state":schedule.state,
            "job_id":job.as_ref().map(|p|&p.plan_id),"job_status":job.as_ref().map(|p|p.status),
            "execution_requested":job.as_ref().is_some_and(|p|p.execution.is_some()),"automatically_retried":false,
            "note":"Actual retained launch/job state, not completion, readiness or remote-stop proof. Exact launch reuse never resumes partial work."}),
        )
    }
}
