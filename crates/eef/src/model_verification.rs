//! Explicit coordinator-owner model check over ordinary node review and grants.
//! Historical intent never renews approval, retries inference or creates a model.
use crate::task::{Plan, PlanStatus};
use anyhow::{Context, Result, bail};
use eefn::model_verification::{Definition, MAX_EXECUTION_MS, MAX_RESULT_BYTES, receipt_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

const NOTE: &str = "Fixed, explicitly owner-approved inference check using the existing review and grant services. A returned-text result is historical operation evidence, not interpretation quality, a project-default approval, current residency or automatic onboarding. Receipt reuse only inspects retained history; no automatic replay, renewal, restart, download or permission change.";

#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Authorize one fixed greeting test of the selected interpreter, not arbitrary work.
    Start {
        #[arg(long, required = true)]
        allow_inference: bool,
        /// Keep this UUID to inspect an uncertain outcome; never renews old work.
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Read retained evidence only, without approval, dispatch or model probing.
    Inspect {
        #[arg(long)]
        operation_id: String,
    },
}

pub fn selected_definition(
    observed: &Value,
    request: &crate::node_setup::Request,
    operation: &str,
) -> Result<Definition> {
    if !crate::node_setup::confirmed(observed, request)
        || observed["result"]["saved_matches_applied"] != true
    {
        bail!("Inspect setup and apply exactly one interpreter before checking inference")
    }
    let eefn::setup_check::InterpreterSelection::Selected { model } =
        serde_json::from_value(observed["result"]["applied_interpreter"].clone())?
    else {
        bail!("Exactly one applied request interpreter is required")
    };
    let definition = Definition {
        schema_version: 1,
        node_id: request.node_id.clone(),
        node_runtime_id: observed["runtime_id"]
            .as_str()
            .context("Missing node runtime")?
            .into(),
        operation_id: operation.into(),
        model,
    };
    definition.validate()?;
    Ok(definition)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Start {
        definition: Definition,
        approve_inference: bool,
    },
    Inspect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported model verification schema")
        }
        eefn::automation_occurrence::uuid(&self.expected_runtime_id)?;
        receipt_id(&self.node_id, &self.operation_id, "source", None)?;
        if let Command::Start {
            definition,
            approve_inference,
        } = &self.command
        {
            definition.validate()?;
            if !approve_inference
                || definition.node_id != self.node_id
                || definition.operation_id != self.operation_id
            {
                bail!("Explicit inference consent and exact model-check intent required")
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NotFound,
    Incomplete,
    AwaitingApproval,
    Approved,
    Running,
    ReturnedText,
    NoText,
    Failed,
    Unknown,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Evidence {
    CoordinatorJournal,
    NodeJournal,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    coordinator_runtime_id: String,
    node_id: String,
    operation_id: String,
    source_operation_id: String,
    success: bool,
    state: State,
    definition: Option<Definition>,
    job_id: Option<String>,
    grant_id: Option<String>,
    executor_session_id: Option<String>,
    evidence_source: Option<Evidence>,
    source_preparation_requested: bool,
    owner_approval_requested: bool,
    execution_requested: bool,
    historical: bool,
    outcome_unknown: bool,
    inference_returned_text: bool,
    model_quality_approved: bool,
    current_setup_verified: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let Ok(source) = receipt_id(&request.node_id, &request.operation_id, "source", None) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_model_verification"
        && r.coordinator_runtime_id == request.expected_runtime_id
        && r.node_id == request.node_id
        && r.operation_id == request.operation_id
        && r.source_operation_id == source
        && r.success == success(r.state)
        && r.inference_returned_text == (r.state == State::ReturnedText)
        && !r.model_quality_approved
        && !r.current_setup_verified
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == NOTE
        && r.definition.as_ref().is_none_or(|d| {
            d.validate().is_ok() && d.node_id == r.node_id && d.operation_id == r.operation_id
        })
        && (matches!(r.state, State::NotFound | State::Unconfirmed)
            || r.definition.is_some() && r.job_id.is_some())
        && (r.state != State::NotFound
            || r.definition.is_none()
                && r.job_id.is_none()
                && r.grant_id.is_none()
                && r.executor_session_id.is_none()
                && r.evidence_source.is_none()
                && !r.outcome_unknown)
        && r.outcome_unknown == matches!(r.state, State::Unknown | State::Unconfirmed)
        && (r.grant_id.is_some() == r.executor_session_id.is_some())
        && (r.grant_id.is_none() || r.job_id.is_some() && r.definition.is_some())
        && (!matches!(r.state, State::ReturnedText | State::NoText) || r.grant_id.is_some())
        && (!matches!(r.state, State::ReturnedText | State::NoText) || r.evidence_source.is_some())
        && (r.evidence_source.is_none() || r.grant_id.is_some())
        && (r.historical
            || match &request.command {
                Command::Start { definition, .. } => {
                    r.definition.as_ref().is_none_or(|d| d == definition)
                }
                Command::Inspect => false,
            })
        && r.job_id
            .as_ref()
            .is_none_or(|id| eefn::request_drafts::validate_id(id).is_ok())
        && r.grant_id
            .as_ref()
            .is_none_or(|id| eefn::automation_occurrence::uuid(id).is_ok())
        && r.executor_session_id
            .as_ref()
            .is_none_or(|id| eefn::automation_occurrence::uuid(id).is_ok())
        && (!r.historical
            || !(r.source_preparation_requested
                || r.owner_approval_requested
                || r.execution_requested))
        && (!matches!(request.command, Command::Inspect)
            || !(r.source_preparation_requested
                || r.owner_approval_requested
                || r.execution_requested))
        && (!r.inference_returned_text
            || r.success
                && r.definition.is_some()
                && r.job_id.is_some()
                && r.grant_id.is_some()
                && r.executor_session_id.is_some()
                && !r.outcome_unknown)
        && (!r.execution_requested || r.owner_approval_requested && r.source_preparation_requested)
}

fn fixed_source(plan: &Plan) -> Result<Definition> {
    crate::jobs::validate_retained_review(plan)?;
    let draft = plan
        .request_review
        .as_ref()
        .context("Missing model verification source")?;
    if draft.proposals.len() != 1 || !draft.answers.is_empty() || draft.audio_source.is_some() {
        bail!("Model verification source was revised or widened")
    }
    let source = eefn::request_submission::ConcreteInput {
        schema_version: 1,
        original_text: draft.original_text.clone(),
        title: draft.current().goal.clone(),
        input: draft
            .concrete_input
            .clone()
            .context("Missing fixed model input")?,
    };
    let definition = Definition::from_source(&source)?;
    let context = plan
        .goal
        .request_context
        .as_ref()
        .context("Missing model-test origin")?;
    if context.origin_node() != definition.node_id
        || context.request_id() != definition.receipt("source", None)?
    {
        bail!("Model-test source origin/receipt changed")
    }
    if let Some(planning) = &plan.planning {
        if planning.input != source.input
            || plan.tasks.len() != 1
            || planning.bindings.len() != 1
            || planning.preparation.is_some()
        {
            bail!("Model-test concrete plan changed")
        }
        let task = &plan.tasks[0];
        let binding = &planning.bindings[0];
        let backend = source.input.constraints["backend"]
            .as_str()
            .context("Missing backend")?;
        if task.capability != "llm.infer"
            || task.action != "run"
            || task.params != source.input.params
            || task.constraints != source.input.constraints
            || !task.dependencies.is_empty()
            || task.retry_count != 0
            || task.retry_delay_ms != 0
            || task.timeout_ms != MAX_EXECUTION_MS
            || binding.task_id != task.task_id
            || binding.node_id != definition.node_id
            || binding.model_id.as_deref() != Some(&definition.model.model_id)
            || binding.backend.as_deref() != Some(backend)
            || binding.resource_id.is_some()
        {
            bail!("Model-test task/binding widened beyond one exact inference")
        }
    }
    Ok(definition)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        let node_id = "node".to_string();
        let operation_id = uuid::Uuid::new_v4().to_string();
        Request {
            schema_version: 1,
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            node_id: node_id.clone(),
            operation_id: operation_id.clone(),
            command: Command::Start {
                definition: Definition {
                    schema_version: 1,
                    node_id,
                    operation_id,
                    node_runtime_id: uuid::Uuid::new_v4().to_string(),
                    model: eefn::setup_check::ModelRef {
                        backend: eefn::model_selection::Backend::Ollama,
                        model_id: "chosen".into(),
                    },
                },
                approve_inference: true,
            },
        }
    }
    fn report(q: &Request) -> Value {
        let Command::Start { definition, .. } = &q.command else {
            unreachable!()
        };
        json!(Report {
            schema_version: 1,
            report_type: "coordinator_model_verification".into(),
            coordinator_runtime_id: q.expected_runtime_id.clone(),
            node_id: q.node_id.clone(),
            operation_id: q.operation_id.clone(),
            source_operation_id: definition.receipt("source", None).unwrap(),
            success: true,
            state: State::ReturnedText,
            definition: Some(definition.clone()),
            job_id: Some("a".repeat(32)),
            grant_id: Some(uuid::Uuid::new_v4().to_string()),
            executor_session_id: Some(uuid::Uuid::new_v4().to_string()),
            evidence_source: Some(Evidence::CoordinatorJournal),
            source_preparation_requested: true,
            owner_approval_requested: true,
            execution_requested: true,
            historical: false,
            outcome_unknown: false,
            inference_returned_text: true,
            model_quality_approved: false,
            current_setup_verified: false,
            restart_requested: false,
            automatically_retried: false,
            note: NOTE.into(),
        })
    }
    #[test]
    fn only_explicit_exact_inference_intent_can_start() {
        let q = request();
        q.validate().unwrap();
        for change in ["consent", "node", "operation", "runtime", "model"] {
            let mut bad = q.clone();
            if let Command::Start {
                definition,
                approve_inference,
            } = &mut bad.command
            {
                match change {
                    "consent" => *approve_inference = false,
                    "node" => definition.node_id = "other".into(),
                    "operation" => definition.operation_id = uuid::Uuid::new_v4().to_string(),
                    "runtime" => definition.node_runtime_id = "invalid".into(),
                    _ => definition.model.model_id = "".into(),
                }
            }
            assert!(bad.validate().is_err(), "{change}");
        }
        let mut expanded = json!(q);
        expanded["command"]["prompt"] = json!("Run arbitrary work");
        assert!(serde_json::from_value::<Request>(expanded).is_err());
    }
    #[test]
    fn retained_source_must_keep_the_authenticated_origin_and_fixed_review() {
        let q = request();
        let Command::Start { definition, .. } = &q.command else {
            unreachable!()
        };
        let source = definition.source().unwrap();
        let context = eefn::context::RequestContext::new(
            source.operation_id.clone(),
            q.node_id.clone(),
            vec!["Room".into()],
        )
        .unwrap();
        let draft = eefn::request_drafts::DraftRecord {
            schema_version: 1,
            original_text: source.request.original_text.clone(),
            concrete_input: Some(source.request.input.clone()),
            audio_source: None,
            proposals: vec![eefn::request_drafts::ProposalRevision {
                operation_id: source.operation_id,
                proposal: source.request.proposal().unwrap(),
                allowed_capabilities: vec!["llm.infer".into()],
                at_ms: 1,
                resolution: None,
                preparation: None,
            }],
            answers: vec![],
        };
        let engine = crate::task::TaskEngine::new(crate::event::EventBus::new(20));
        let plan = crate::request_drafts::store_review(&engine, draft, &context)
            .unwrap()
            .plan;
        assert_eq!(&fixed_source(&plan).unwrap(), definition);
        for change in ["node", "receipt", "prompt", "title", "revision"] {
            let mut bad = plan.clone();
            match change {
                "node" => {
                    bad.goal.request_context = Some(
                        eefn::context::RequestContext::new(
                            context.request_id().into(),
                            "other".into(),
                            vec![],
                        )
                        .unwrap(),
                    )
                }
                "receipt" => {
                    let other = uuid::Uuid::new_v4().to_string();
                    bad.goal.request_context = Some(
                        eefn::context::RequestContext::new(
                            other.clone(),
                            q.node_id.clone(),
                            vec![],
                        )
                        .unwrap(),
                    );
                    bad.request_review.as_mut().unwrap().proposals[0].operation_id = other;
                }
                "prompt" => {
                    bad.request_review
                        .as_mut()
                        .unwrap()
                        .concrete_input
                        .as_mut()
                        .unwrap()
                        .params["prompt"] = json!("Other work")
                }
                "title" => bad.goal.description = "Other work".into(),
                _ => {
                    let draft = bad.request_review.as_mut().unwrap();
                    let mut revised = draft.proposals[0].clone();
                    revised.operation_id = uuid::Uuid::new_v4().to_string();
                    draft.proposals.push(revised);
                }
            }
            assert!(fixed_source(&bad).is_err(), "{change}");
        }
    }
    #[test]
    fn final_evidence_requires_exact_grant_and_distinguishes_unknown_empty_and_failure() {
        use eefn::grants::{ExecutionGrant, GrantLimits, GrantReport, ReceiptState};
        let q = request();
        let Command::Start { definition, .. } = &q.command else {
            unreachable!()
        };
        let grant = ExecutionGrant {
            schema_version: 1,
            grant_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: "a".repeat(64),
            session_id: uuid::Uuid::new_v4().to_string(),
            executor_node: q.node_id.clone(),
            job_id: "b".repeat(32),
            task_id: "c".repeat(32),
            attempt: 1,
            plan_sha256: "d".repeat(64),
            request_context: eefn::context::RequestContext::new(
                definition.receipt("source", None).unwrap(),
                q.node_id.clone(),
                vec![],
            )
            .unwrap(),
            capability: "llm.infer".into(),
            action: "run".into(),
            params: definition.source().unwrap().request.input.params,
            resource_id: None,
            model_id: Some(definition.model.model_id.clone()),
            backend: Some("ollama".into()),
            model_role: Some("request_interpreter".into()),
            priority: 0,
            limits: GrantLimits {
                max_execution_ms: MAX_EXECUTION_MS,
                max_result_bytes: MAX_RESULT_BYTES,
            },
            reporting: "acceptance_and_completion".into(),
            issued_at_ms: 1,
            expires_at_ms: MAX_EXECUTION_MS + 1,
        };
        grant.validate().unwrap();
        let final_report = GrantReport::new(
            &grant,
            ReceiptState::Completed,
            json!({"content":"Hello"}),
            None,
        );
        assert_eq!(
            report_state(&grant, &final_report).unwrap(),
            State::ReturnedText
        );
        for key in [
            "grant_id",
            "issuer_id",
            "session_id",
            "executor_node",
            "job_id",
            "task_id",
            "plan_sha256",
            "grant_sha256",
        ] {
            let mut bad = json!(final_report);
            bad[key] = json!("different");
            assert!(
                report_state(&grant, &serde_json::from_value(bad).unwrap()).is_err(),
                "{key}"
            );
        }
        let empty = GrantReport::new(
            &grant,
            ReceiptState::Completed,
            json!({"content":"  "}),
            None,
        );
        assert_eq!(report_state(&grant, &empty).unwrap(), State::NoText);
        let oversized = GrantReport::new(
            &grant,
            ReceiptState::Completed,
            json!({"content":"x".repeat(MAX_RESULT_BYTES+1)}),
            None,
        );
        assert!(report_state(&grant, &oversized).is_err());
        let unknown = GrantReport::new(
            &grant,
            ReceiptState::Unknown,
            Value::Null,
            Some("effects_unknown"),
        );
        assert_eq!(report_state(&grant, &unknown).unwrap(), State::Unknown);
        let failed = GrantReport::new(
            &grant,
            ReceiptState::Completed,
            Value::Null,
            Some("provider_error"),
        );
        assert_eq!(report_state(&grant, &failed).unwrap(), State::Failed);
        let accepted = GrantReport::new(&grant, ReceiptState::Accepted, Value::Null, None);
        assert!(report_state(&grant, &accepted).is_err());
    }
    #[test]
    fn replies_cannot_invent_quality_authority_or_returned_text() {
        let q = request();
        let value = report(&q);
        assert!(confirmed(&value, &q));
        for (key, bad) in [
            ("success", json!(false)),
            ("state", json!("failed")),
            (
                "coordinator_runtime_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
            ("source_operation_id", json!(q.operation_id)),
            ("definition", Value::Null),
            ("job_id", Value::Null),
            ("grant_id", Value::Null),
            ("executor_session_id", Value::Null),
            ("evidence_source", Value::Null),
            ("historical", json!(true)),
            ("outcome_unknown", json!(true)),
            ("inference_returned_text", json!(false)),
            ("current_setup_verified", json!(true)),
            ("model_quality_approved", json!(true)),
            ("automatically_retried", json!(true)),
            ("restart_requested", json!(true)),
            ("owner_approval_requested", json!(false)),
            ("private_peer_text", json!("PRIVATE")),
        ] {
            let mut changed = value.clone();
            changed[key] = bad;
            assert!(!confirmed(&changed, &q), "{key}");
        }
        let mut changed = value.clone();
        changed["definition"]["model"]["model_id"] = json!("other");
        assert!(!confirmed(&changed, &q));
        let mut inspect = q.clone();
        inspect.command = Command::Inspect;
        assert!(!confirmed(&value, &inspect));
        let mut historical = value;
        historical["historical"] = json!(true);
        for flag in [
            "source_preparation_requested",
            "owner_approval_requested",
            "execution_requested",
        ] {
            historical[flag] = json!(false);
        }
        assert!(confirmed(&historical, &inspect));
    }
    #[test]
    fn unknown_or_missing_receipts_never_become_success() {
        let mut q = request();
        let mut value = report(&q);
        q.command = Command::Inspect;
        for key in [
            "source_preparation_requested",
            "owner_approval_requested",
            "execution_requested",
            "inference_returned_text",
            "success",
        ] {
            value[key] = json!(false);
        }
        value["historical"] = json!(true);
        value["state"] = json!("unknown");
        value["outcome_unknown"] = json!(true);
        assert!(confirmed(&value, &q));
        value["success"] = json!(true);
        assert!(!confirmed(&value, &q));
        value["success"] = json!(false);
        value["state"] = json!("not_found");
        value["outcome_unknown"] = json!(false);
        assert!(!confirmed(&value, &q));
        for key in [
            "definition",
            "job_id",
            "grant_id",
            "executor_session_id",
            "evidence_source",
        ] {
            value[key] = Value::Null;
        }
        assert!(confirmed(&value, &q));
        assert!(
            selected_definition(
                &json!({"success":true}),
                &crate::node_setup::Request {
                    schema_version: 1,
                    node_id: q.node_id.clone(),
                    expected_runtime_id: None,
                    request_id: uuid::Uuid::new_v4().to_string(),
                },
                &q.operation_id
            )
            .is_err()
        );
    }
}
fn success(state: State) -> bool {
    matches!(
        state,
        State::Incomplete
            | State::AwaitingApproval
            | State::Approved
            | State::Running
            | State::ReturnedText
    )
}

fn report_state(
    grant: &eefn::grants::ExecutionGrant,
    retained: &eefn::grants::GrantReport,
) -> Result<State> {
    use eefn::grants::ReceiptState;
    grant.validate()?;
    if !retained.confirms(grant) || retained.state == ReceiptState::Accepted {
        bail!("Model-test final report does not confirm exact issuance")
    }
    Ok(match retained.state {
        ReceiptState::Completed if retained.success => {
            if retained.data["content"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty() && s.len() <= MAX_RESULT_BYTES)
            {
                State::ReturnedText
            } else {
                State::NoText
            }
        }
        ReceiptState::Completed | ReceiptState::Rejected => State::Failed,
        ReceiptState::Unknown => State::Unknown,
        ReceiptState::Accepted => unreachable!(),
    })
}

impl crate::Runtime {
    fn model_verification_plan(&self, node: &str, operation: &str) -> Result<Option<Plan>> {
        let source = receipt_id(node, operation, "source", None)?;
        let mut found = self.engine.store.list()?.into_iter().filter(|p| {
            p.goal
                .request_context
                .as_ref()
                .is_some_and(|c| c.origin_node() == node && c.request_id() == source)
        });
        let plan = found.next();
        if found.next().is_some() {
            bail!("Ambiguous model verification receipt")
        }
        if let Some(plan) = &plan {
            let d = fixed_source(plan)?;
            if d.node_id != node || d.operation_id != operation {
                bail!("Model verification receipt conflicts")
            }
        }
        Ok(plan)
    }
    async fn observe_model_verification(&self, report: &mut Report) -> Result<()> {
        let Some(plan) = self.model_verification_plan(&report.node_id, &report.operation_id)?
        else {
            report.state = State::NotFound;
            report.success = false;
            return Ok(());
        };
        report.definition = Some(fixed_source(&plan)?);
        report.job_id = Some(plan.plan_id.clone());
        report.state = match plan.status {
            PlanStatus::Review => State::Incomplete,
            PlanStatus::AwaitingApproval => State::AwaitingApproval,
            PlanStatus::Approved => State::Approved,
            PlanStatus::Running | PlanStatus::Pausing | PlanStatus::Stopping => State::Running,
            PlanStatus::Completed => State::Unknown,
            PlanStatus::Failed | PlanStatus::Cancelled => State::Failed,
            _ => State::Unknown,
        };
        if let Some(execution) = &plan.execution {
            if execution.attempts.len() > 1 {
                bail!("Model-test history has multiple attempts")
            }
            if let Some(attempt) = execution.attempts.first() {
                report.grant_id = Some(attempt.grant.grant_id.clone());
                report.executor_session_id = Some(attempt.grant.session_id.clone());
                let local = attempt.final_report();
                // A durable definitive final report is already exact-grant
                // evidence. Do not require a live node just to re-read it.
                if let Some(retained) = local.filter(|r| {
                    matches!(
                        r.state,
                        eefn::grants::ReceiptState::Completed
                            | eefn::grants::ReceiptState::Rejected
                    )
                }) {
                    report.state = report_state(&attempt.grant, retained)?;
                    report.evidence_source = Some(Evidence::CoordinatorJournal);
                } else {
                    let result = self
                        .inspect_request_result(
                            eefn::request_result::ResultCommand {
                                id: plan.plan_id.clone(),
                                grant_id: attempt.grant.grant_id.clone(),
                            },
                            Some(&report.node_id),
                        )
                        .await;
                    // A retained unknown outcome remains useful offline. Newer node
                    // evidence is a read, never reconciliation or renewed authority.
                    if let Some(retained) = local {
                        report.state = report_state(&attempt.grant, retained)?;
                        report.evidence_source = Some(Evidence::CoordinatorJournal);
                    }
                    let result = match result {
                        Ok(result) => Some(result),
                        Err(_) if local.is_some() => None,
                        Err(error) => return Err(error),
                    };
                    if let Some(result) =
                        result.filter(|r| r["result"]["availability"] == "retained_final_report")
                    {
                        let retained: eefn::grants::GrantReport =
                            serde_json::from_value(result["result"]["report"].clone())?;
                        report.state = report_state(&attempt.grant, &retained)?;
                        report.evidence_source = Some(Evidence::NodeJournal);
                    }
                }
            }
        }
        report.inference_returned_text = report.state == State::ReturnedText;
        report.success = success(report.state);
        report.outcome_unknown = matches!(report.state, State::Unknown | State::Unconfirmed);
        Ok(())
    }
    pub async fn model_verification_command(&self, request: Request) -> Result<Value> {
        request.validate()?;
        if request.expected_runtime_id != self.instance_id {
            bail!("Coordinator model-check runtime changed")
        }
        let mut report = Report {
            schema_version: 1,
            report_type: "coordinator_model_verification".into(),
            coordinator_runtime_id: self.instance_id.clone(),
            node_id: request.node_id.clone(),
            operation_id: request.operation_id.clone(),
            source_operation_id: receipt_id(
                &request.node_id,
                &request.operation_id,
                "source",
                None,
            )?,
            success: false,
            state: State::Unconfirmed,
            definition: None,
            job_id: None,
            grant_id: None,
            executor_session_id: None,
            evidence_source: None,
            source_preparation_requested: false,
            owner_approval_requested: false,
            execution_requested: false,
            historical: false,
            outcome_unknown: false,
            inference_returned_text: false,
            model_quality_approved: false,
            current_setup_verified: false,
            restart_requested: false,
            automatically_retried: false,
            note: NOTE.into(),
        };
        let result = match &request.command {
            Command::Inspect => {
                report.historical = true;
                self.observe_model_verification(&mut report).await
            }
            Command::Start { definition, .. } => {
                self.start_model_verification(definition, &mut report).await
            }
        };
        if result.is_err() {
            report.state = State::Unconfirmed;
            report.success = false;
            report.inference_returned_text = false;
            // An unconfirmed read/busy/conflicting intent cannot prove the
            // operation had no effects (another call may already own it).
            // Per-call request flags separately state what this call attempted.
            report.outcome_unknown = true;
        }
        let value = json!(report);
        if !confirmed(&value, &request) {
            bail!("Model verification report unconfirmed; inspect exact receipt")
        }
        Ok(value)
    }
    async fn start_model_verification(
        &self,
        definition: &Definition,
        report: &mut Report,
    ) -> Result<()> {
        let _owned = self
            .model_verification
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Model-check workflow busy; no queue or retry"))?;
        if let Some(existing) =
            self.model_verification_plan(&definition.node_id, &definition.operation_id)?
        {
            if fixed_source(&existing)? != *definition {
                bail!("Retained model-check intent conflicts")
            }
            report.historical = true;
            return self.observe_model_verification(report).await;
        }
        report.definition = Some(definition.clone());
        report.source_preparation_requested = true;
        let prepared = self
            .node_server
            .invoke_remote(
                &definition.node_id,
                "node.configure",
                "prepare_model_verification",
                json!(definition),
                Duration::from_secs(20),
            )
            .await?;
        if prepared["success"] != true
            || !eefn::model_verification::preparation_confirmed(&prepared["data"], definition)
        {
            bail!("Model-test preparation unconfirmed")
        }
        if prepared["data"]["source"]["plan"]["runtime_id"] != self.instance_id {
            bail!("Model-test source reached another coordinator")
        }
        let plan = self
            .model_verification_plan(&definition.node_id, &definition.operation_id)?
            .context("Model-test source missing")?;
        if fixed_source(&plan)? != *definition {
            bail!("Model-test source changed")
        }
        report.job_id = Some(plan.plan_id.clone());
        if prepared["data"]["source"]["creation_reused"] == true {
            // A late/cancelled earlier request may have saved this source. This
            // call cannot continue that uncertain operation into new authority.
            report.source_preparation_requested = false;
            report.historical = true;
            return self.observe_model_verification(report).await;
        }
        if plan.status != PlanStatus::AwaitingApproval
            || plan.approval.is_some()
            || plan.execution.is_some()
        {
            bail!("Fresh model-test plan is not awaiting owner approval")
        }
        let planning = plan.planning.as_ref().context("Missing model-test plan")?;
        let approve = crate::request_planning::PlanningCommand::Approve {
            id: plan.plan_id.clone(),
            expected_revision: plan.revision,
            operation_id: definition.receipt("approval", Some(&plan.plan_id))?,
            plan_sha256: planning.plan_sha256.clone(),
            valid_for_ms: 60_000,
            limits: eefn::grants::GrantLimits {
                max_execution_ms: MAX_EXECUTION_MS,
                max_result_bytes: MAX_RESULT_BYTES,
            },
        };
        report.owner_approval_requested = true;
        self.planning_command(crate::request_planning::PlanningRequest {
            schema_version: 1,
            expected_runtime_id: self.instance_id.clone(),
            command: approve.clone(),
        })
        .await?;
        let approved = self.engine.store.get(&plan.plan_id)?;
        let approval = approved
            .approval
            .as_ref()
            .context("Owner approval not retained")?;
        if fixed_source(&approved)? != *definition
            || !approval.confirms(&approve)
            || !approval.active_at(&approved, eefn::protocol::now_ms())
            || approved.execution.is_some()
        {
            bail!("Model-test approval changed")
        }
        report.execution_requested = true;
        self.execution_command(crate::request_execution::ExecutionRequest {
            schema_version: 1,
            expected_runtime_id: self.instance_id.clone(),
            command: eefn::request_execution::ExecuteCommand {
                id: plan.plan_id.clone(),
                expected_revision: approved.revision,
                operation_id: definition.receipt("execution", Some(&plan.plan_id))?,
                approval_sha256: approval.approval_sha256.clone(),
            },
        })
        .await?;
        self.observe_model_verification(report).await
    }
}
