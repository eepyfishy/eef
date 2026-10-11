//! Owner-approved fixed scopes in the existing journal; no occurrence runner.
use crate::{Runtime, task::Plan};
use anyhow::{Context, Result, bail};
use eefn::automation_policy::{
    Authorization, FixedScope, PolicyCommand, PolicyReport, PolicyRequest, ScopedOperation,
};
use serde_json::{Value, json};

pub(crate) fn fixed_scope(plan: &Plan) -> Result<FixedScope> {
    crate::jobs::validate_retained_review(plan)?;
    let planning = plan
        .planning
        .as_ref()
        .context("Policy requires a compiled source")?;
    let scope = FixedScope {
        owner_context: plan
            .goal
            .request_context
            .clone()
            .context("Missing source origin")?,
        input: planning.input.clone(),
        operations: plan
            .tasks
            .iter()
            .zip(&planning.bindings)
            .enumerate()
            .map(|(index, (task, binding))| ScopedOperation {
                capability: task.capability.clone(),
                action: task.action.clone(),
                params: task.params.clone(),
                constraints: task.constraints.clone(),
                dependencies: (0..index).collect(),
                timeout_ms: task.timeout_ms,
                executor_node: binding.node_id.clone(),
                resource_id: binding.resource_id.clone(),
                model_id: binding.model_id.clone(),
                backend: binding.backend.clone(),
            })
            .collect(),
    };
    scope.validate()?;
    Ok(scope)
}

pub(crate) fn validate_authorization(
    record: &Authorization,
    definition: &eefn::automation::Definition,
) -> Result<()> {
    record.validate()?;
    definition.validate()?;
    let PolicyCommand::Approve {
        definition: id,
        revision,
        definition_sha256,
        valid_for_ms,
        max_runs,
        ..
    } = &record.creation
    else {
        bail!("Missing owner policy command");
    };
    let eefn::automation::AutomationCommand::Define {
        from_job,
        valid_for_ms: declared_validity,
        max_runs: declared_runs,
        ..
    } = &definition.creation
    else {
        bail!("Missing source definition");
    };
    if *id != definition.id
        || *revision != 1
        || *definition_sha256 != definition.definition_sha256
        || valid_for_ms > declared_validity
        || max_runs > declared_runs
        || record.approved_at_ms < definition.created_at_ms
        || record.scope.owner_context != definition.owner_context
    {
        bail!("Approval widens or substitutes the retained definition");
    }
    if let Some(source) = &definition.source {
        let plan: Plan = serde_json::from_value(source.clone())?;
        if fixed_scope(&plan)? != record.scope {
            bail!("Approval scope differs from frozen compiled source");
        }
    }
    validate_scope_compilation(record)?;
    // Private validation objects only, never issued/persisted/returned grants.
    // This preserves exact native-operation contracts even after a definition
    // is removed and its larger source snapshot has been released.
    let limits = record.creation.limits().unwrap();
    for operation in &record.scope.operations {
        let grant = eefn::grants::ExecutionGrant {
            schema_version: 1,
            grant_id: record.id.clone(),
            session_id: record.runtime_id.clone(),
            issuer_id: record.issuer_id.clone(),
            executor_node: operation.executor_node.clone(),
            job_id: from_job.clone(),
            task_id: from_job.clone(),
            attempt: 1,
            plan_sha256: record.scope_sha256.clone(),
            request_context: record.scope.owner_context.clone(),
            capability: operation.capability.clone(),
            action: operation.action.clone(),
            params: operation.params.clone(),
            resource_id: operation.resource_id.clone(),
            model_id: operation.model_id.clone(),
            backend: operation.backend.clone(),
            model_role: operation.constraints["model_role"]
                .as_str()
                .map(str::to_owned),
            priority: 0,
            limits: limits.clone(),
            reporting: "acceptance_and_completion".into(),
            issued_at_ms: record.approved_at_ms,
            expires_at_ms: record
                .approved_at_ms
                .checked_add(limits.max_execution_ms)
                .context("Policy time overflow")?,
        };
        grant.validate()?;
    }
    Ok(())
}

fn validate_scope_compilation(record: &Authorization) -> Result<()> {
    use crate::request_planning::{PlannedBinding, PlanningRecord};
    // Independently reconstruct only the compiler validation envelope. These
    // temporary identities/review markers are not factual occurrence history,
    // are never saved/returned, and grant no authority. The existing validator
    // checks input/operations/bindings even after source snapshot removal.
    let description = "Fixed automation scope validation";
    let scope = &record.scope;
    let mut plan = crate::task::TaskPlanner.plan(crate::task::Goal {
        request_context: Some(scope.owner_context.clone()),
        description: description.into(),
        template: scope.input.template.clone(),
        params: scope.input.params.clone(),
        constraints: scope.input.constraints.clone(),
    })?;
    if plan.tasks.len() != scope.operations.len() {
        bail!("Fixed scope template shape changed; migration required");
    }
    let capabilities = scope
        .operations
        .iter()
        .map(|o| o.capability.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    plan.request_review = Some(serde_json::from_value(
        json!({"schema_version":1,"original_text":description,
        "concrete_input":scope.input,"answers":[],"proposals":[{"operation_id":scope.owner_context.request_id(),
        "allowed_capabilities":capabilities,"at_ms":record.approved_at_ms,"proposal":{"schema_version":1,"intent":"information",
        "goal":description,"context_hints":[],"constraints":[],"suggested_capabilities":capabilities,"complexity":"simple",
        "workload":"one_shot","needs_clarification":false,"clarification":null}}]}),
    )?);
    plan.status = crate::task::PlanStatus::AwaitingApproval;
    plan.revision = 2;
    let mut bindings = Vec::new();
    for (task, operation) in plan.tasks.iter_mut().zip(&scope.operations) {
        task.capability = operation.capability.clone();
        task.action = operation.action.clone();
        task.params = operation.params.clone();
        task.constraints = operation.constraints.clone();
        task.timeout_ms = operation.timeout_ms;
        task.retry_count = 0;
        task.retry_delay_ms = 0;
        bindings.push(PlannedBinding {
            task_id: task.task_id.clone(),
            node_id: operation.executor_node.clone(),
            resource_id: operation.resource_id.clone(),
            model_id: operation.model_id.clone(),
            backend: operation.backend.clone(),
        });
    }
    let mut planning = PlanningRecord {
        schema_version: 1,
        operation_id: uuid::Uuid::new_v4().to_string(),
        source_revision: 1,
        proposal_index: 0,
        compiled_at_ms: record.approved_at_ms,
        input: scope.input.clone(),
        bindings,
        plan_sha256: String::new(),
        preparation: None,
    };
    planning.plan_sha256 = crate::request_planning::digest(&plan, &planning)?;
    plan.planning = Some(planning);
    crate::request_planning::validate_retained(&plan)
}

impl Runtime {
    pub async fn automation_policy_command(&self, request: PolicyRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Automation policy schema/coordinator runtime changed");
        }
        let command = &request.command;
        command.validate()?;
        // Retained exact receipt reuse is inspection, including after restart,
        // expiry, revocation, definition removal or executor disconnection.
        if let PolicyCommand::Approve { operation_id, .. } = command {
            if let Some(existing) = self.engine.store.automation_authorization(operation_id)? {
                if existing.creation != *command {
                    bail!("Approval receipt conflicts; inspect retained policy");
                }
                return Ok(report(
                    &self.instance_id,
                    command,
                    Some(existing),
                    vec![],
                    None,
                    true,
                ));
            }
        }
        let candidate = if let PolicyCommand::Approve {
            definition,
            valid_for_ms,
            operation_id,
            generation,
            ..
        } = command
        {
            let value = self.engine.store.automation_command(
                eefn::automation::AutomationCommand::Show {
                    id: definition.clone(),
                },
                None,
            )?;
            let definition: eefn::automation::Definition =
                serde_json::from_value(value["definition"].clone())?;
            let source: Plan = serde_json::from_value(
                definition
                    .source
                    .clone()
                    .context("Removed definition cannot receive approval")?,
            )?;
            let limits = command.limits().unwrap();
            crate::request_approval::validate_options(
                &source.planning.as_ref().unwrap().plan_sha256,
                *valid_for_ms,
                &limits,
            )?;
            let now = eefn::protocol::now_ms();
            let driver = crate::request_execution::Driver::new(self);
            let mut issuer_id = String::new();
            for index in 0..source.tasks.len() {
                let session = driver.session(&source, index).await?;
                let issuer = session["issuer_id"]
                    .as_str()
                    .context("Missing current grant issuer")?;
                if issuer_id.is_empty() {
                    issuer_id = issuer.into();
                }
                let policy: eefn::grants::GrantPolicy =
                    serde_json::from_value(session["limits"].clone())?;
                policy.validate()?;
                if issuer_id != issuer
                    || limits.max_execution_ms > policy.max_duration_ms
                    || limits.max_result_bytes > policy.max_result_bytes
                {
                    bail!("Owner policy exceeds current issuer/node grant bounds");
                }
            }
            let scope = fixed_scope(&source)?;
            let scope_sha256 = eefn::grants::definition_digest(&json!(scope))?;
            let mut record = Authorization {
                schema_version: 1,
                id: operation_id.clone(),
                creation: command.clone(),
                generation: generation
                    .checked_add(1)
                    .context("Policy generation exhausted")?,
                runtime_id: self.instance_id.clone(),
                issuer_id,
                approved_at_ms: now,
                expires_at_ms: now
                    .checked_add(*valid_for_ms)
                    .context("Policy time overflow")?,
                scope,
                scope_sha256,
                authorization_sha256: String::new(),
                revision: 1,
                state: "approved".into(),
                revocation: None,
            };
            record.authorization_sha256 = record.digest()?;
            validate_authorization(&record, &definition)?;
            Some(record)
        } else {
            None
        };
        let mut response: PolicyReport =
            serde_json::from_value(self.engine.store.automation_policy_checkpoint(
                command.clone(),
                candidate,
                &self.instance_id,
                eefn::protocol::now_ms(),
            )?)?;
        response.runtime_id = self.instance_id.clone();
        let value = json!(response);
        if !eefn::automation_policy::confirmed(&value, command, &self.instance_id) {
            bail!("Policy checkpoint reply failed confirmation; inspect receipt before retrying");
        }
        Ok(value)
    }
}
pub(crate) fn report(
    runtime: &str,
    command: &PolicyCommand,
    authorization: Option<Authorization>,
    items: Vec<Value>,
    next_after: Option<String>,
    reused: bool,
) -> Value {
    json!(PolicyReport {
        schema_version: 1,
        report_type: "automation_authorization".into(),
        runtime_id: runtime.into(),
        command: command.clone(),
        authorization,
        items,
        next_after,
        reused,
        occurrence_admission_available: true,
        timers_started: false,
        execution_requested: false,
        dispatched: false,
        automatically_retried: false
    })
}
