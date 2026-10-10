//! Coordinator-owned concrete planning/approval in the existing journal. Never dispatches.
use crate::{
    capability::CapabilityRegistry,
    model::{Modality, ModelRegistry, ModelRequest},
    task::{Goal, Plan, PlanStatus, TaskPlanner, TaskStatus},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub use eefn::request_planning::{PlanInput, read_input};
fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        bail!("Expected a bounded ASCII identifier")
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedBinding {
    pub task_id: String,
    pub node_id: String,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    pub backend: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningRecord {
    pub schema_version: u32,
    pub operation_id: String,
    pub source_revision: u64,
    pub proposal_index: usize,
    pub compiled_at_ms: u64,
    pub input: PlanInput,
    pub bindings: Vec<PlannedBinding>,
    pub plan_sha256: String,
}
fn digest(plan: &Plan, record: &PlanningRecord) -> Result<String> {
    let tasks=plan.tasks.iter().map(|t|json!({"id":t.task_id,"request_context":t.request_context,"capability":t.capability,"action":t.action,
        "params":t.params,"constraints":t.constraints,"dependencies":t.dependencies,"timeout_ms":t.timeout_ms,"retry_count":t.retry_count,"retry_delay_ms":t.retry_delay_ms})).collect::<Vec<_>>();
    let bytes = serde_json::to_vec(
        &json!({"schema_version":1,"id":plan.plan_id,"goal":plan.goal,"tasks":tasks,
        "request_review":plan.request_review,"operation_id":record.operation_id,"source_revision":record.source_revision,
        "proposal_index":record.proposal_index,"compiled_at_ms":record.compiled_at_ms,"input":record.input,"bindings":record.bindings}),
    )?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn unbound_plan(plan: &Plan, input: &PlanInput) -> Result<Plan> {
    input.validate()?;
    let compiled = TaskPlanner.plan(Goal {
        request_context: plan.goal.request_context.clone(),
        description: plan.goal.description.clone(),
        template: input.template.clone(),
        params: input.params.clone(),
        constraints: input.constraints.clone(),
    })?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Missing reviewed request")?;
    draft.validate()?;
    if compiled.tasks.iter().any(|t| {
        !draft
            .current()
            .suggested_capabilities
            .contains(&t.capability)
    }) {
        bail!("Concrete template widens the reviewed capability hints; revise the proposal first")
    }
    let used: BTreeSet<&str> = compiled
        .tasks
        .iter()
        .flat_map(|t| {
            t.params
                .as_object()
                .into_iter()
                .flat_map(|p| p.keys().map(String::as_str))
        })
        .collect();
    if input
        .params
        .as_object()
        .unwrap()
        .keys()
        .any(|k| !used.contains(k.as_str()))
    {
        bail!("Plan input contains parameters unused by its template")
    }
    Ok(compiled)
}
pub(crate) fn validate_retained(plan: &Plan) -> Result<()> {
    let record = plan
        .planning
        .as_ref()
        .context("Missing concrete plan record")?;
    record.input.validate()?;
    eefn::job_commands::validate_operation_id(&record.operation_id)?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Missing reviewed request")?;
    draft.validate()?;
    if record.schema_version != 1
        || record.source_revision == 0
        || record.source_revision >= plan.revision
        || record.compiled_at_ms == 0
        || record.proposal_index != draft.proposals.len() - 1
        || draft.current().needs_clarification
        || draft.current().workload != eefn::interpretation::Workload::OneShot
        || draft
            .proposals
            .iter()
            .any(|p| p.operation_id == record.operation_id)
        || draft
            .answers
            .iter()
            .any(|a| a.operation_id == record.operation_id)
        || (plan.execution.is_none()
            && !matches!(
                plan.status,
                PlanStatus::AwaitingApproval | PlanStatus::Approved | PlanStatus::Cancelled
            ))
        || plan.tasks.is_empty()
        || plan.tasks.len() > 128
        || record.bindings.len() != plan.tasks.len()
        || plan.goal.template != record.input.template
        || plan.goal.params != record.input.params
        || plan.goal.constraints != record.input.constraints
        || (plan.execution.is_none() && !plan.results.is_empty())
        || serde_json::to_vec(plan)?.len()
            > if plan.execution.is_some() {
                8 * 1024 * 1024
            } else {
                1024 * 1024
            }
    {
        bail!("Invalid retained concrete plan state")
    }
    let fresh = unbound_plan(plan, &record.input)?;
    if fresh.tasks.len() != plan.tasks.len() {
        bail!("Concrete template shape changed; migration required")
    }
    let mut seen = BTreeSet::new();
    for (index, ((task, binding), expected)) in plan
        .tasks
        .iter()
        .zip(&record.bindings)
        .zip(&fresh.tasks)
        .enumerate()
    {
        eefn::request_drafts::validate_id(&task.task_id)?;
        eefn::network::validate_remote_node_id(&binding.node_id)?;
        if binding.task_id != task.task_id
            || !seen.insert(&task.task_id)
            || task.request_context != plan.goal.request_context
            || task.capability != expected.capability
            || task.action != expected.action
            || (plan.execution.is_none()
                && (task.status
                    != if plan.status == PlanStatus::Cancelled {
                        TaskStatus::Cancelled
                    } else {
                        TaskStatus::Pending
                    }
                    || task.attempts != 0
                    || task.result.is_some()
                    || !task.assignments.is_empty()))
            || task.retry_count != 0
            || task.retry_delay_ms != 0
            || task.timeout_ms != expected.timeout_ms
            || task.dependencies
                != plan.tasks[..index]
                    .iter()
                    .map(|t| t.task_id.clone())
                    .collect::<Vec<_>>()
        {
            bail!("Concrete plan task is not its compiled non-executable definition")
        }
        let mut params = expected.params.clone();
        let mut constraints = record.input.constraints.clone();
        if constraints
            .get("node_id")
            .is_some_and(|id| id != &json!(binding.node_id))
        {
            bail!("Binding violates explicit node selection")
        }
        constraints["node_id"] = json!(binding.node_id);
        if matches!(task.capability.as_str(), "llm.infer" | "vlm.analyze") {
            validate_model_input(&expected.params, task.capability == "vlm.analyze")?;
            if constraints
                .as_object()
                .unwrap()
                .keys()
                .any(|k| !matches!(k.as_str(), "node_id" | "backend" | "model_role"))
            {
                bail!("Unsupported retained model constraints")
            }
            if binding.resource_id.is_some() {
                bail!("Inference cannot bind a physical resource")
            }
            let model = binding
                .model_id
                .as_deref()
                .context("Missing selected model")?;
            eefn::model_selection::validate_id(model)?;
            let backend = binding
                .backend
                .as_deref()
                .context("Missing selected backend")?;
            identifier(backend)?;
            if record
                .input
                .params
                .get("model_id")
                .is_some_and(|id| id != &json!(model))
                || constraints
                    .get("backend")
                    .is_some_and(|id| id != &json!(backend))
            {
                bail!("Binding violates explicit model/backend selection")
            }
            params["model_id"] = json!(model);
            constraints["backend"] = json!(backend);
        } else {
            if constraints.get("backend").is_some() || constraints.get("model_role").is_some() {
                bail!("Unsupported retained non-model constraints")
            }
            if binding.model_id.is_some() || binding.backend.is_some() {
                bail!("Non-model binding cannot claim a model")
            }
            if record
                .input
                .constraints
                .get("resource_id")
                .is_some_and(|r| binding.resource_id.as_deref() != r.as_str())
                || record
                    .input
                    .params
                    .get("resource_id")
                    .is_some_and(|r| binding.resource_id.as_deref() != r.as_str())
            {
                bail!("Binding violates explicit resource selection")
            }
            if let Some(resource) = &binding.resource_id {
                if resource.len() > 256
                    || resource.chars().any(char::is_control)
                    || !resource.starts_with(&format!("{}::", binding.node_id))
                {
                    bail!("Invalid resource binding")
                }
                params["resource_id"] = json!(resource);
                constraints["resource_id"] = json!(resource);
            }
        }
        if task.params != params || task.constraints != constraints {
            bail!("Concrete task differs from its retained input/binding")
        }
    }
    if record.plan_sha256.len() != 64 || digest(plan, record)? != record.plan_sha256 {
        bail!("Concrete plan digest mismatch")
    }
    crate::request_approval::validate_retained(plan)?;
    crate::request_execution::validate_retained(plan)?;
    Ok(())
}

#[derive(Debug, clap::Subcommand)]
pub enum PlanningAction {
    /// Close local tracking of an uncertain attempt; acknowledge that remote effects may continue.
    Abandon {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        grant_id: String,
        #[arg(long, required = true)]
        acknowledge_effects_may_continue: bool,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Inspect one issued attempt's current authenticated executor receipt; never replay or reconcile.
    Receipt {
        id: String,
        #[arg(long)]
        grant_id: String,
    },
    /// Explicitly request execution of one exact owner-approved plan; never replay.
    Execute {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        approval_digest: String,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Inspect private source/plan as the local coordinator owner; never executes.
    Inspect { id: String },
    /// Compile a reviewed one-shot request against live registrations; await approval.
    Compile {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        file: std::path::PathBuf,
    },
    /// Save bounded owner approval for an exact inspected plan; never dispatches.
    Approve {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        valid_for_ms: u64,
        #[arg(long)]
        max_execution_ms: u64,
        #[arg(long)]
        max_result_bytes: usize,
        /// Reuse only this exact receipt after an uncertain response; never renews.
        #[arg(long)]
        operation_id: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanningCommand {
    Inspect {
        id: String,
    },
    Compile {
        id: String,
        expected_revision: u64,
        operation_id: String,
        input: PlanInput,
    },
    Approve {
        id: String,
        expected_revision: u64,
        operation_id: String,
        plan_sha256: String,
        valid_for_ms: u64,
        limits: eefn::grants::GrantLimits,
    },
}
impl PlanningCommand {
    pub fn id(&self) -> &str {
        match self {
            Self::Inspect { id } | Self::Compile { id, .. } | Self::Approve { id, .. } => id,
        }
    }
    pub fn validate(&self) -> Result<()> {
        eefn::request_drafts::validate_id(self.id())?;
        if let Self::Compile {
            expected_revision,
            operation_id,
            input,
            ..
        } = self
        {
            if *expected_revision == 0 || *expected_revision == u64::MAX {
                bail!("Invalid planning revision")
            }
            eefn::job_commands::validate_operation_id(operation_id)?;
            input.validate()?;
        }
        if let Self::Approve {
            expected_revision,
            operation_id,
            plan_sha256,
            valid_for_ms,
            limits,
            ..
        } = self
        {
            if *expected_revision == 0 || *expected_revision == u64::MAX {
                bail!("Invalid approval revision")
            }
            eefn::job_commands::validate_operation_id(operation_id)?;
            if uuid::Uuid::parse_str(operation_id)?.is_nil() {
                bail!("Nil approval receipt")
            }
            crate::request_approval::validate_options(plan_sha256, *valid_for_ms, limits)?;
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: PlanningCommand,
}

fn report(plan: Plan, command: &PlanningCommand, runtime_id: &str, reused: bool) -> Value {
    json!({"schema_version":1,"report_type":"request_planning","success":true,"runtime_id":runtime_id,"id":plan.plan_id,
        "operation_id":match command {PlanningCommand::Inspect{..}=>None,PlanningCommand::Compile{operation_id,..}|PlanningCommand::Approve{operation_id,..}=>Some(operation_id)},
        "plan":plan,"reused":reused,"execution_authorized":false,"dispatched":false,"automatically_retried":false,
        "note":"Private owner inspection/compilation/conditional approval only; this command dispatches no work. The retained record may include separately requested execution and its receipts."})
}
pub fn confirmed_report(value: &Value, runtime_id: &str, command: &PlanningCommand) -> bool {
    if command.validate().is_err() {
        return false;
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Report {
        schema_version: u32,
        report_type: String,
        success: bool,
        runtime_id: String,
        id: String,
        operation_id: Option<String>,
        plan: Plan,
        reused: bool,
        execution_authorized: bool,
        dispatched: bool,
        automatically_retried: bool,
        note: String,
    }
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if r.schema_version != 1
        || r.report_type != "request_planning"
        || !r.success
        || r.runtime_id != runtime_id
        || r.id != command.id()
        || r.plan.plan_id != r.id
        || r.execution_authorized
        || r.dispatched
        || r.automatically_retried
        || r.note.len() > 2048
        || json!(r.plan) != value["plan"]
        || crate::jobs::validate_retained_review(&r.plan).is_err()
    {
        return false;
    }
    match command {
        PlanningCommand::Inspect { .. } => {
            r.operation_id.is_none() && !r.reused && r.plan.request_review.is_some()
        }
        PlanningCommand::Compile {
            operation_id,
            expected_revision,
            input,
            ..
        } => {
            r.operation_id.as_deref() == Some(operation_id)
                && r.plan.planning.as_ref().is_some_and(|p| {
                    p.operation_id == *operation_id
                        && p.source_revision == *expected_revision
                        && p.input == *input
                })
                && (r.reused
                    || expected_revision
                        .checked_add(1)
                        .is_some_and(|rev| r.plan.revision == rev))
        }
        PlanningCommand::Approve {
            operation_id,
            expected_revision,
            ..
        } => {
            r.operation_id.as_deref() == Some(operation_id)
                && r.plan
                    .approval
                    .as_ref()
                    .is_some_and(|a| a.confirms(command))
                && (r.reused
                    || expected_revision
                        .checked_add(1)
                        .is_some_and(|rev| r.plan.revision == rev))
        }
    }
}

impl crate::Runtime {
    pub async fn planning_command(&self, request: PlanningRequest) -> Result<Value> {
        self.planning_command_scoped(request, None).await
    }
    async fn planning_command_scoped(
        &self,
        request: PlanningRequest,
        origin: Option<&str>,
    ) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Planning schema/runtime changed; inspect before retrying")
        }
        request.command.validate()?;
        let saved = self.engine.store.get(request.command.id())?;
        if origin.is_some_and(|node| {
            saved
                .goal
                .request_context
                .as_ref()
                .is_none_or(|c| c.origin_node() != node)
        }) {
            bail!("Request unavailable or not owned by this origin")
        }
        if saved.request_review.is_none() {
            bail!("Not a reviewed request")
        }
        if matches!(request.command, PlanningCommand::Approve { .. }) {
            if origin.is_some() {
                bail!("Only the coordinator owner may approve work")
            }
            let (plan, reused) =
                crate::request_approval::approve(self, saved, &request.command).await?;
            return Ok(report(plan, &request.command, &self.instance_id, reused));
        }
        let PlanningCommand::Compile {
            expected_revision,
            operation_id,
            input,
            ..
        } = &request.command
        else {
            return Ok(report(saved, &request.command, &self.instance_id, false));
        };
        if let Some(record) = &saved.planning {
            if record.operation_id == *operation_id
                && record.source_revision == *expected_revision
                && record.input == *input
            {
                return Ok(report(saved, &request.command, &self.instance_id, true));
            }
            bail!("A concrete plan is already retained; inspect/cancel it before replacing work")
        }
        if saved.status != PlanStatus::Review || saved.revision != *expected_revision {
            bail!("Request state/revision changed")
        }
        let draft = saved.request_review.as_ref().unwrap();
        if draft.current().needs_clarification {
            bail!("Resolve clarification through an explicit proposal revision before planning")
        }
        if draft.current().workload != eefn::interpretation::Workload::OneShot {
            bail!("Continuous/unknown workloads require the future local pipeline compiler")
        }
        let mut compiled = unbound_plan(&saved, input)?;
        let permitted = match origin {
            Some(node) => Some(self.disclosed_planning_nodes(node).await?),
            None => None,
        };
        let mut record = PlanningRecord {
            schema_version: 1,
            operation_id: operation_id.clone(),
            source_revision: *expected_revision,
            proposal_index: draft.proposals.len() - 1,
            compiled_at_ms: eefn::protocol::now_ms(),
            input: input.clone(),
            bindings: vec![],
            plan_sha256: String::new(),
        };
        bind_tasks(
            &mut compiled,
            &mut record,
            &self.registry,
            &self.model_registry,
            &self.node_server,
            &crate::runtime::model_tiers(&self.config),
            permitted.as_ref(),
        )
        .await?;
        compiled.plan_id = saved.plan_id.clone();
        compiled.request_review = saved.request_review.clone();
        compiled.status = PlanStatus::AwaitingApproval;
        compiled.revision = saved
            .revision
            .checked_add(1)
            .context("Revision exhausted")?;
        record.plan_sha256 = digest(&compiled, &record)?;
        compiled.planning = Some(record);
        validate_retained(&compiled)?;
        let mut reused = false;
        let result = self.engine.edit_review_plan(&saved.plan_id, |plan| {
            if let Some(record) = &plan.planning {
                if record.operation_id == *operation_id
                    && record.source_revision == *expected_revision
                    && record.input == *input
                {
                    reused = true;
                    return Ok(false);
                }
                bail!("Concurrent concrete plan retained; inspect before retrying")
            }
            if plan.status != PlanStatus::Review || plan.revision != *expected_revision {
                bail!("Request state/revision changed before checkpoint")
            }
            plan.goal = compiled.goal;
            plan.tasks = compiled.tasks;
            plan.planning = compiled.planning;
            plan.status = PlanStatus::AwaitingApproval;
            Ok(true)
        })?;
        Ok(report(result, &request.command, &self.instance_id, reused))
    }

    pub async fn origin_planning_command(
        &self,
        command: eefn::request_planning::PlanningCommand,
        context: &eefn::context::RequestContext,
    ) -> Result<Value> {
        use eefn::request_planning::{PlannedTask, PlanningCommand as NodeCommand, PlanningView};
        command.validate()?;
        let owner_command = match &command {
            NodeCommand::Inspect { id } => PlanningCommand::Inspect { id: id.clone() },
            NodeCommand::Compile {
                id,
                expected_revision,
                operation_id,
                input,
            } => {
                if operation_id != context.request_id() {
                    bail!("Planning receipt does not match authenticated origin")
                }
                PlanningCommand::Compile {
                    id: id.clone(),
                    expected_revision: *expected_revision,
                    operation_id: operation_id.clone(),
                    input: input.clone(),
                }
            }
        };
        let result = self
            .planning_command_scoped(
                PlanningRequest {
                    schema_version: 1,
                    expected_runtime_id: self.instance_id.clone(),
                    command: owner_command,
                },
                Some(context.origin_node()),
            )
            .await?;
        let plan: Plan = serde_json::from_value(result["plan"].clone())?;
        crate::jobs::validate_retained_review(&plan)?;
        let planning = plan.planning.as_ref();
        let view = PlanningView {
            schema_version: 1,
            report_type: "coordinator_node_planning".into(),
            runtime_id: self.instance_id.clone(),
            id: plan.plan_id.clone(),
            operation_id: context.request_id().into(),
            revision: plan.revision,
            status: serde_json::to_value(plan.status)?
                .as_str()
                .context("Invalid plan state")?
                .into(),
            request_context: plan
                .goal
                .request_context
                .clone()
                .context("Missing immutable origin")?,
            draft: plan
                .request_review
                .clone()
                .context("Missing retained review")?,
            input: planning.map(|p| p.input.clone()),
            planning_operation_id: planning.map(|p| p.operation_id.clone()),
            source_revision: planning.map(|p| p.source_revision),
            plan_sha256: planning.map(|p| p.plan_sha256.clone()),
            bindings: planning
                .map(|p| serde_json::from_value(json!(p.bindings)))
                .transpose()?
                .unwrap_or_default(),
            tasks: plan
                .tasks
                .iter()
                .map(|t| {
                    Ok(PlannedTask {
                        task_id: t.task_id.clone(),
                        capability: t.capability.clone(),
                        action: t.action.clone(),
                        params: t.params.clone(),
                        constraints: t.constraints.clone(),
                        dependencies: t.dependencies.clone(),
                        timeout_ms: t.timeout_ms,
                        status: serde_json::to_value(t.status)?
                            .as_str()
                            .context("Invalid task state")?
                            .into(),
                    })
                })
                .collect::<Result<_>>()?,
            reused: result["reused"] == true,
            persisted: true,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false,
        };
        let value = json!(view);
        if !eefn::request_planning::confirmed(
            &value,
            context.origin_node(),
            context.request_id(),
            &command,
        ) {
            bail!("Retained node planning view failed validation")
        }
        Ok(value)
    }
}

pub(crate) async fn live_peer(
    server: &eefn::NodeServer,
    node: &str,
    capability: &str,
    freshness: u64,
) -> Result<eefn::network::PeerRecord> {
    eefn::network::validate_remote_node_id(node)?;
    let (peer, age) = server
        .peer_records(&BTreeSet::from([node.to_owned()]))
        .await?
        .pop()
        .context("Selected node is not registered")?;
    if age.as_millis() > u128::from(freshness * 1000)
        || !peer.capabilities.iter().any(|c| c == capability)
    {
        bail!("Selected provider is stale or withdrawn")
    }
    Ok(peer)
}
async fn bind_tasks(
    plan: &mut Plan,
    record: &mut PlanningRecord,
    registry: &CapabilityRegistry,
    models: &ModelRegistry,
    server: &eefn::NodeServer,
    tiers: &BTreeMap<String, Vec<String>>,
    permitted: Option<&BTreeSet<String>>,
) -> Result<()> {
    let freshness = registry.scheduling_policy().freshness_seconds;
    for task in &mut plan.tasks {
        task.retry_count = 0;
        task.retry_delay_ms = 0;
        let mut constraints = task.constraints.clone();
        let mut binding = PlannedBinding {
            task_id: task.task_id.clone(),
            node_id: String::new(),
            resource_id: None,
            model_id: None,
            backend: None,
        };
        if matches!(task.capability.as_str(), "llm.infer" | "vlm.analyze") {
            if constraints
                .as_object()
                .unwrap()
                .keys()
                .any(|k| !matches!(k.as_str(), "node_id" | "backend" | "model_role"))
            {
                bail!("Model planning currently supports exact node/backend/role constraints only")
            }
            validate_model_input(&task.params, task.capability == "vlm.analyze")?;
            let candidates = models
                .route(
                    &ModelRequest {
                        modality: if task.capability == "vlm.analyze" {
                            Modality::Vlm
                        } else {
                            Modality::Text
                        },
                        tier: task.params["tier"].as_str().map(str::to_owned),
                        model_id: task.params["model_id"].as_str().map(str::to_owned),
                        constraints: constraints.clone(),
                    },
                    tiers,
                )
                .into_iter()
                .filter(|spec| permitted.is_none_or(|nodes| nodes.contains(&spec.node_id)))
                .collect::<Vec<_>>();
            if candidates.len() > 256 {
                bail!("Too many planning candidates; constrain the model route")
            }
            let mut selected = None;
            for spec in candidates {
                let Ok(peer) = live_peer(server, &spec.node_id, &task.capability, freshness).await
                else {
                    continue;
                };
                if peer.models.iter().any(|m| current_model_matches(m, &spec)) {
                    selected = Some(spec);
                    break;
                }
            }
            let spec = selected.context("No current registered model matches the concrete plan")?;
            binding.node_id = spec.node_id;
            binding.model_id = Some(spec.model_id.clone());
            binding.backend = Some(spec.backend.clone());
            task.params["model_id"] = json!(spec.model_id);
            constraints["backend"] = json!(spec.backend);
        } else {
            if constraints.get("backend").is_some() || constraints.get("model_role").is_some() {
                bail!("Model constraints cannot be ignored by a non-model task")
            }
            if let Some(resource) = task.params.get("resource_id") {
                if constraints
                    .get("resource_id")
                    .is_some_and(|v| v != resource)
                {
                    bail!("Conflicting resource selection")
                }
                constraints["resource_id"] = resource.clone();
            }
            let provider = registry
                .find_best_scoped(
                    &task.capability,
                    &task.action,
                    &constraints,
                    task.request_context.as_ref(),
                    permitted,
                )
                .context("No current provider matches the concrete plan")?;
            live_peer(server, &provider.node_id, &task.capability, freshness).await?;
            CapabilityRegistry::validate_provider_params(&provider, &task.params)?;
            binding.node_id = provider.node_id;
            binding.resource_id = provider.properties["selected_resource_id"]
                .as_str()
                .map(str::to_owned);
            if let Some(resource) = &binding.resource_id {
                task.params["resource_id"] = json!(resource);
                constraints["resource_id"] = json!(resource);
            }
        }
        constraints["node_id"] = json!(binding.node_id);
        task.constraints = constraints;
        record.bindings.push(binding);
    }
    Ok(())
}
pub(crate) fn current_model_matches(
    peer: &eefn::network::PeerModel,
    spec: &crate::model::ModelSpec,
) -> bool {
    let expected = spec.model_metadata.clone().unwrap_or_else(|| {
        eefn::model_metadata::ModelMetadata::from_legacy(Some(match spec.modality {
            Modality::Text => "text",
            Modality::Vlm => "vlm",
            Modality::Other => "other",
        }))
    });
    peer.model_id == spec.model_id
        && peer.backend.as_deref().unwrap_or("unknown") == spec.backend
        && peer.normalized_metadata() == expected
}
fn validate_model_input(params: &Value, vision: bool) -> Result<()> {
    let text = params.get("prompt");
    let messages = params.get("messages");
    if text.is_some() == messages.is_some() {
        bail!("Provide exactly one prompt or messages")
    }
    if let Some(text) = text {
        eefn::interpretation::messages(text.as_str().context("Prompt must be text")?, &[])?;
    }
    if let Some(messages) = messages {
        let messages = messages.as_array().context("Messages must be a list")?;
        if messages.is_empty() || messages.len() > 64 {
            bail!("Message list exceeds bounds")
        }
        for message in messages {
            let object = message.as_object().context("Message must be an object")?;
            if object
                .keys()
                .any(|k| !matches!(k.as_str(), "role" | "content"))
                || !matches!(
                    message["role"].as_str(),
                    Some("system" | "user" | "assistant")
                )
            {
                bail!("Unsupported concrete model message")
            }
            eefn::interpretation::messages(
                message["content"]
                    .as_str()
                    .context("Message content must be text")?,
                &[],
            )?;
        }
    }
    if let Some(images) = params.get("images") {
        let images = images.as_array().context("Images must be a list")?;
        if images.len() > 8 || images.iter().any(|v| v.as_str().is_none_or(str::is_empty)) {
            bail!("Invalid image list")
        }
        if !vision && !images.is_empty() {
            bail!("Text task cannot claim image support")
        }
    }
    if vision && params["images"].as_array().is_none_or(|v| v.is_empty()) {
        bail!(
            "Vision task requires explicit image input; implicit camera-result wiring is not implemented"
        )
    }
    if params
        .get("temperature")
        .is_some_and(|v| v.as_f64().is_none_or(|n| !(0.0..=2.0).contains(&n)))
    {
        bail!("Invalid temperature")
    }
    if params
        .get("max_tokens")
        .is_some_and(|v| v.as_u64().is_none_or(|n| n == 0 || n > 32768))
    {
        bail!("Invalid token limit")
    }
    if let Some(model) = params.get("model_id") {
        eefn::model_selection::validate_id(model.as_str().context("Model ID must be text")?)?;
    }
    if let Some(tier) = params.get("tier") {
        identifier(tier.as_str().context("Tier must be text")?)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{event::EventBus, jobs::JobStore, task::TaskEngine};

    fn input() -> PlanInput {
        PlanInput {
            schema_version: 1,
            template: "system_info".into(),
            params: json!({}),
            constraints: json!({"node_id":"owner"}),
        }
    }
    pub(crate) fn review(engine: &TaskEngine) -> Plan {
        let ctx = eefn::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "owner".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let report=crate::request_drafts::open(engine,json!({"schema_version":1,"original_text":" Inspect status ","proposal":{
            "schema_version":1,"intent":"information","goal":"Inspect status","context_hints":[],"constraints":[],
            "suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null}}),
            &ctx,&["system.info".into()]).unwrap();
        engine.store.get(report["id"].as_str().unwrap()).unwrap()
    }
    pub(crate) fn compiled(saved: &Plan) -> Plan {
        let mut p = unbound_plan(saved, &input()).unwrap();
        p.plan_id = saved.plan_id.clone();
        p.request_review = saved.request_review.clone();
        p.status = PlanStatus::AwaitingApproval;
        p.revision = saved.revision + 1;
        for t in &mut p.tasks {
            t.retry_count = 0;
            t.retry_delay_ms = 0;
        }
        let mut r = PlanningRecord {
            schema_version: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
            source_revision: saved.revision,
            proposal_index: 0,
            compiled_at_ms: eefn::protocol::now_ms(),
            input: input(),
            bindings: p
                .tasks
                .iter()
                .map(|t| PlannedBinding {
                    task_id: t.task_id.clone(),
                    node_id: "owner".into(),
                    resource_id: None,
                    model_id: None,
                    backend: None,
                })
                .collect(),
            plan_sha256: String::new(),
        };
        r.plan_sha256 = digest(&p, &r).unwrap();
        p.planning = Some(r);
        p
    }
    #[test]
    fn inputs_reject_ignored_authority_and_invalid_model_payloads() {
        let engine = TaskEngine::new(EventBus::new(20));
        let saved = review(&engine);
        for constraint in [
            json!({"permissions":true}),
            json!({"node_id":"local"}),
            json!({"area":"Room"}),
            json!({"vram_min_mb":-1}),
        ] {
            let mut i = input();
            i.constraints = constraint;
            assert!(i.validate().is_err());
        }
        let mut i = input();
        i.params = json!({"permissions":true});
        assert!(unbound_plan(&saved, &i).is_err());
        i.params = json!({});
        i.template = "".into();
        assert!(unbound_plan(&saved, &i).is_err());
        i.template = "execute_process".into();
        i.params = json!({"program":"unreviewed"});
        assert!(unbound_plan(&saved, &i).is_err());
        assert!(serde_json::from_value::<PlanInput>(json!({"schema_version":1,"template":"system_info","params":{},"constraints":{},"approved":true})).is_err());
        for p in [
            json!({}),
            json!({"prompt":"x","messages":[]}),
            json!({"prompt":""}),
            json!({"prompt":"x","max_tokens":0}),
            json!({"messages":[{"role":"user","content":"x","tool_call":{}}]}),
            json!({"prompt":"x","images":["img"]}),
        ] {
            assert!(validate_model_input(&p, false).is_err());
        }
        assert!(validate_model_input(&json!({"prompt":"x"}), true).is_err());
        assert!(validate_model_input(&json!({"prompt":"x","images":["img"]}), true).is_ok());
    }
    #[tokio::test]
    async fn compiled_checkpoint_survives_restart_without_execution_and_can_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let saved = review(&engine);
        let candidate = compiled(&saved);
        validate_retained(&candidate).unwrap();
        let persisted = engine
            .edit_review_plan(&saved.plan_id, |p| {
                p.goal = candidate.goal.clone();
                p.tasks = candidate.tasks.clone();
                p.planning = candidate.planning.clone();
                p.status = PlanStatus::AwaitingApproval;
                Ok(true)
            })
            .unwrap();
        assert_eq!(persisted.revision, 2);
        assert_eq!(
            persisted.request_review.as_ref().unwrap().original_text,
            " Inspect status "
        );
        assert!(engine.run_plan(persisted.clone()).await.is_err());
        assert!(engine.enqueue(persisted.clone()).is_err());
        assert!(engine.enqueue_request(persisted.clone()).is_err());
        assert!(engine.pause(&saved.plan_id).is_err());
        assert!(engine.resume_job(&saved.plan_id).is_err());
        assert_eq!(
            crate::jobs::view(&persisted, true)["planning_state"],
            "compiled_no_grant"
        );
        assert!(
            crate::jobs::view(&persisted, true)
                .get("planning")
                .is_none()
        );
        let draft = persisted.request_review.as_ref().unwrap();
        let opening = crate::request_drafts::open(
            &engine,
            json!({"schema_version":1,"original_text":draft.original_text,
            "proposal":draft.proposals[0].proposal}),
            persisted.goal.request_context.as_ref().unwrap(),
            &["system.info".into()],
        )
        .unwrap();
        assert_eq!(opening["id"], persisted.plan_id);
        assert_eq!(opening["reused"], true);
        assert_eq!(opening["revision"], 2);
        let unchanged = json!(persisted);
        engine.shutdown().await;
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        assert_eq!(json!(store.get(&saved.plan_id).unwrap()), unchanged);
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let cancelled = engine.cancel(&saved.plan_id).unwrap();
        validate_retained(&cancelled).unwrap();
        assert_eq!(cancelled.tasks[0].status, TaskStatus::Cancelled);
        assert_eq!(
            cancelled.planning.as_ref().unwrap().plan_sha256,
            unchanged["planning"]["plan_sha256"]
        );
    }
    #[tokio::test]
    async fn compiler_checkpoint_quota_and_shutdown_fail_without_rewriting_review() {
        let engine = TaskEngine::new(EventBus::new(20));
        let saved = review(&engine);
        let candidate = compiled(&saved);
        engine.shutdown().await;
        assert!(
            engine
                .edit_review_plan(&saved.plan_id, |p| {
                    *p = candidate;
                    Ok(true)
                })
                .is_err()
        );
        assert_eq!(
            json!(engine.store.get(&saved.plan_id).unwrap()),
            json!(saved)
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 1, 2800).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let saved = review(&engine);
        let candidate = compiled(&saved);
        assert!(
            engine
                .edit_review_plan(&saved.plan_id, |p| {
                    p.goal = candidate.goal;
                    p.tasks = candidate.tasks;
                    p.planning = candidate.planning;
                    p.status = PlanStatus::AwaitingApproval;
                    Ok(true)
                })
                .is_err()
        );
        assert_eq!(json!(store.get(&saved.plan_id).unwrap()), json!(saved));
    }
    #[test]
    fn input_file_supports_utf8_bom_and_rejects_oversize_or_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.json");
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(serde_json::to_vec(&input()).unwrap());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read_input(&path).unwrap(), input());
        std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_input(&path).is_err());
        std::fs::write(&path,br#"{"schema_version":1,"template":"system_info","params":{},"constraints":{},"approved":true}"#).unwrap();
        assert!(read_input(&path).is_err());
    }
    #[test]
    fn definition_context_bindings_and_confirmation_cannot_be_changed() {
        let engine = TaskEngine::new(EventBus::new(20));
        let saved = review(&engine);
        let p = compiled(&saved);
        let command = PlanningCommand::Compile {
            id: p.plan_id.clone(),
            expected_revision: 1,
            operation_id: p.planning.as_ref().unwrap().operation_id.clone(),
            input: input(),
        };
        let good = report(p.clone(), &command, "runtime", false);
        assert!(confirmed_report(&good, "runtime", &command));
        for (key, value) in [
            ("execution_authorized", json!(true)),
            ("runtime_id", json!("other")),
            ("operation_id", json!(uuid::Uuid::new_v4().to_string())),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(!confirmed_report(&bad, "runtime", &command));
        }
        let mut bad = p.clone();
        bad.tasks[0].constraints["node_id"] = json!("other");
        assert!(validate_retained(&bad).is_err());
        let mut bad = p.clone();
        bad.tasks[0].attempts = 1;
        assert!(validate_retained(&bad).is_err());
        let mut bad = p.clone();
        bad.request_review.as_mut().unwrap().original_text = "altered".into();
        assert!(validate_retained(&bad).is_err());
        let mut bad = p.clone();
        bad.status = PlanStatus::Pending;
        assert!(validate_retained(&bad).is_err());
        let mut bad = p.clone();
        bad.planning.as_mut().unwrap().bindings[0].node_id = "other".into();
        bad.tasks[0].constraints["node_id"] = json!("other");
        assert!(validate_retained(&bad).is_err());
        let mut bad = good.clone();
        bad["plan"]["approved"] = json!(true);
        assert!(!confirmed_report(&bad, "runtime", &command));
    }
    #[test]
    fn current_model_snapshot_must_agree_with_cached_role_and_readiness() {
        let mut peer = eefn::network::PeerModel {
            model_id: "m".into(),
            backend: Some("ollama".into()),
            modality: Some("text".into()),
            model_metadata: None,
        };
        let spec = crate::model::ModelSpec {
            model_id: "m".into(),
            modality: Modality::Text,
            family: String::new(),
            quality_tier: String::new(),
            vram_needed_mb: 0,
            backend: "ollama".into(),
            hardware: "unknown".into(),
            node_id: "owner".into(),
            status: crate::model::ModelStatus::Unloaded,
            size_bytes: 0,
            model_metadata: None,
        };
        assert!(current_model_matches(&peer, &spec));
        peer.backend = Some("llamacpp".into());
        assert!(!current_model_matches(&peer, &spec));
        peer.backend = Some("ollama".into());
        let mut meta = peer.normalized_metadata();
        meta.roles = Some(vec!["different".into()]);
        peer.model_metadata = Some(meta);
        assert!(!current_model_matches(&peer, &spec));
    }
    #[test]
    fn damaged_concrete_definition_is_retained_without_automatic_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        drop(rusqlite::Connection::open(&path).unwrap());
        let store = JobStore::open(&path, 500, 64 * 1024 * 1024).unwrap();
        let engine = TaskEngine::with_store(EventBus::new(20), store.clone());
        let saved = review(&engine);
        let candidate = compiled(&saved);
        let p = engine
            .edit_review_plan(&saved.plan_id, |p| {
                p.goal = candidate.goal;
                p.tasks = candidate.tasks;
                p.planning = candidate.planning;
                p.status = PlanStatus::AwaitingApproval;
                Ok(true)
            })
            .unwrap();
        drop(engine);
        drop(store);
        let sql = rusqlite::Connection::open(&path).unwrap();
        let mut bad = json!(p);
        bad["tasks"][0]["params"] = json!({"program":"altered"});
        let raw = serde_json::to_string(&bad).unwrap();
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
        assert_eq!(retained, raw);
    }
}
