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
    /// Resolve the selected interpreter in the backend, after checking history.
    StartSelected {
        approve_inference: bool,
    },
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
        if matches!(
            self.command,
            Command::StartSelected {
                approve_inference: false
            }
        ) {
            bail!("Explicit inference consent required")
        }
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
    NotReady,
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

/// Fixed public guidance only: never copy arbitrary peer failure text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessIssue {
    MissingInterpreter,
    AmbiguousInterpreter,
    ApplySavedInterpreter,
    RuntimeUnavailable,
    ExecutionTransportUnavailable,
    ManagementConsentRequired,
    SetupApprovalRequired,
    NoApprovedDefault,
    InspectModelSetup,
    SetupUnconfirmed,
    PermissionsUnconfirmed,
}
impl ReadinessIssue {
    fn unconfirmed(self) -> bool {
        matches!(self, Self::SetupUnconfirmed | Self::PermissionsUnconfirmed)
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::MissingInterpreter => {
                "Inspect node setup/model setup and select one request interpreter. No model was chosen or installed by this check."
            }
            Self::AmbiguousInterpreter => {
                "Select exactly one request interpreter; this check will not choose the first candidate."
            }
            Self::ApplySavedInterpreter => {
                "Inspect saved/applied node setup and apply the intended interpreter with an approved activation or restart. This check will not restart the node."
            }
            Self::RuntimeUnavailable => {
                "Wait for the node runtime, then inspect its setup. No source, approval or inference was requested by this call."
            }
            Self::ExecutionTransportUnavailable => {
                "Inspect node setup and establish a private paired connection with reviewed-work receipts before checking inference. No pairing or update was attempted."
            }
            Self::ManagementConsentRequired => {
                "On this node, explicitly enable management from eef with: eefn permissions allow remote-management. This check cannot grant that permission remotely."
            }
            Self::SetupApprovalRequired => {
                "Inspect model setup and review its exact pending setup approval. Do not select a replacement interpreter just because setup has not applied one yet. This check does not approve or resume setup."
            }
            Self::NoApprovedDefault => {
                "No project model default is approved. Inspect model setup and deliberately choose an installed or owner-selected catalog model; this check cannot approve a default or download one."
            }
            Self::InspectModelSetup => {
                "Inspect the existing model-setup status and exact attempt/transfer receipts before changing selection or retrying. Retained checkpoints do not prove a live worker or successful inference. This check does not drive setup."
            }
            Self::SetupUnconfirmed => {
                "Inspect the node connection, version and setup. This call did not prepare a source; inspect this operation ID before deciding whether to start again."
            }
            Self::PermissionsUnconfirmed => {
                "Inspect this node's connection, version and permissions locally. An unconfirmed reply is not permission consent; inspect this operation ID before deciding whether to start again."
            }
        }
    }
}

fn selected_readiness(
    observed: &Value,
    request: &crate::node_setup::Request,
    operation: &str,
) -> std::result::Result<Definition, ReadinessIssue> {
    use eefn::setup_check::InterpreterSelection as Selection;
    if !crate::node_setup::confirmed(observed, request) {
        return Err(ReadinessIssue::SetupUnconfirmed);
    }
    let result = &observed["result"];
    let saved: Selection = serde_json::from_value(result["saved_interpreter"].clone())
        .map_err(|_| ReadinessIssue::SetupUnconfirmed)?;
    let applied: Selection = serde_json::from_value(result["applied_interpreter"].clone())
        .map_err(|_| ReadinessIssue::SetupUnconfirmed)?;
    if matches!(applied, Selection::RuntimeUnavailable {}) {
        return Err(ReadinessIssue::RuntimeUnavailable);
    }
    if matches!(saved, Selection::Ambiguous { .. })
        || matches!(applied, Selection::Ambiguous { .. })
    {
        return Err(ReadinessIssue::AmbiguousInterpreter);
    }
    if result["saved_matches_applied"] != true || matches!(applied, Selection::Missing {}) {
        // Reuse validated shared guidance. An absent interpreter may belong to
        // an existing approval/setup attempt, not a request for another model.
        let steps: Vec<eefn::setup_check::NextStep> =
            serde_json::from_value(result["next_steps"].clone())
                .map_err(|_| ReadinessIssue::SetupUnconfirmed)?;
        for step in steps {
            use eefn::setup_check::NextStep;
            match step {
                NextStep::ApproveModelSetup => return Err(ReadinessIssue::SetupApprovalRequired),
                NextStep::ChooseModelSetup => return Err(ReadinessIssue::NoApprovedDefault),
                NextStep::InspectModelSetup => return Err(ReadinessIssue::InspectModelSetup),
                _ => {}
            }
        }
    }
    if result["saved_matches_applied"] != true {
        return Err(ReadinessIssue::ApplySavedInterpreter);
    }
    if matches!(applied, Selection::Missing {}) {
        return Err(ReadinessIssue::MissingInterpreter);
    }
    if result["execution_receipts_available"] != true {
        return Err(ReadinessIssue::ExecutionTransportUnavailable);
    }
    selected_definition(observed, request, operation).map_err(|_| ReadinessIssue::SetupUnconfirmed)
}

#[async_trait::async_trait]
trait ReadinessTransport: Sync {
    async fn setup(&self, request: crate::node_setup::Request) -> Result<Value>;
    async fn permissions(
        &self,
        request: &eefn::permission_commands::PermissionRequest,
    ) -> Result<Value>;
}
#[async_trait::async_trait]
impl ReadinessTransport for eefn::NodeServer {
    async fn setup(&self, request: crate::node_setup::Request) -> Result<Value> {
        crate::node_setup::command(self, request).await
    }
    async fn permissions(
        &self,
        request: &eefn::permission_commands::PermissionRequest,
    ) -> Result<Value> {
        self.invoke_remote(
            &request.expected_node_id,
            "node.configure",
            "permission_status",
            json!(request),
            Duration::from_secs(10),
        )
        .await
    }
}
async fn selected_on(
    transport: &impl ReadinessTransport,
    node: &str,
    operation: &str,
) -> std::result::Result<Definition, ReadinessIssue> {
    let request = crate::node_setup::Request {
        schema_version: 1,
        node_id: node.into(),
        expected_runtime_id: None,
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    let observed = transport
        .setup(request.clone())
        .await
        .map_err(|_| ReadinessIssue::SetupUnconfirmed)?;
    let definition = selected_readiness(&observed, &request, operation)?;
    let permissions = eefn::permission_commands::PermissionRequest {
        schema_version: 1,
        expected_node_id: node.into(),
        expected_runtime_id: definition.node_runtime_id.clone(),
        command: eefn::permission_commands::PermissionCommand::Show {},
    };
    let reply = transport
        .permissions(&permissions)
        .await
        .map_err(|_| ReadinessIssue::PermissionsUnconfirmed)?;
    if reply["success"] != true
        || !eefn::permission_commands::confirmed(&reply["data"], &permissions)
    {
        return Err(ReadinessIssue::PermissionsUnconfirmed);
    }
    if reply["data"]["remote_management_allowed"] != true {
        return Err(ReadinessIssue::ManagementConsentRequired);
    }
    Ok(definition)
}

/// Shared presentation of confirmed backend guidance; no client-owned decisions.
pub fn render(value: &Value) -> String {
    let mut text = format!(
        "Node model check: {}\nNode: {}\nOperation: {}\nSource receipt: {}\n",
        value["state"].as_str().unwrap_or("unconfirmed"),
        value["node_id"].as_str().unwrap_or("unknown"),
        value["operation_id"].as_str().unwrap_or("unknown"),
        value["source_operation_id"].as_str().unwrap_or("unknown"),
    );
    if let Ok(issue) = serde_json::from_value::<ReadinessIssue>(value["readiness_issue"].clone()) {
        text.push_str(issue.guidance());
        text.push('\n');
    }
    text.push_str("Inspect without replay: eef node verify-model --node NODE_ID inspect --operation-id OPERATION_ID (use the values above).\nReturned text is historical evidence, not model quality or current setup health. An unconfirmed start may have issued work; do not allocate a new operation to retry it.\n");
    text
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
    // Only the new start_selected command emits this extension. Existing
    // explicit-definition start/inspect replies keep their original wire shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    readiness_issue: Option<ReadinessIssue>,
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
        && (r.readiness_issue.is_none()
            || matches!(request.command, Command::StartSelected { .. })
                && !r.historical
                && r.definition.is_none()
                && r.job_id.is_none()
                && r.grant_id.is_none()
                && r.executor_session_id.is_none()
                && r.evidence_source.is_none()
                && !r.source_preparation_requested
                && !r.owner_approval_requested
                && !r.execution_requested
                && r.state
                    == if r.readiness_issue.unwrap().unconfirmed() {
                        State::Unconfirmed
                    } else {
                        State::NotReady
                    })
        && (r.state != State::NotReady || r.readiness_issue.is_some())
        && r.inference_returned_text == (r.state == State::ReturnedText)
        && !r.model_quality_approved
        && !r.current_setup_verified
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == NOTE
        && r.definition.as_ref().is_none_or(|d| {
            d.validate().is_ok() && d.node_id == r.node_id && d.operation_id == r.operation_id
        })
        && (matches!(
            r.state,
            State::NotFound | State::NotReady | State::Unconfirmed
        ) || r.definition.is_some() && r.job_id.is_some())
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
                Command::StartSelected { .. } => true,
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
    use std::sync::{Arc, Mutex};

    struct Peer {
        setup: Option<Value>,
        permissions: Option<Value>,
        calls: Mutex<Vec<Value>>,
    }
    #[async_trait::async_trait]
    impl ReadinessTransport for Peer {
        async fn setup(&self, request: crate::node_setup::Request) -> Result<Value> {
            self.calls.lock().unwrap().push(json!(request));
            let mut value = self.setup.clone().context("PRIVATE setup unavailable")?;
            value["request_id"] = json!(request.request_id);
            value["result"]["request_id"] = json!(request.request_id);
            Ok(value)
        }
        async fn permissions(
            &self,
            request: &eefn::permission_commands::PermissionRequest,
        ) -> Result<Value> {
            self.calls.lock().unwrap().push(json!(request));
            self.permissions
                .clone()
                .context("PRIVATE permissions unavailable")
        }
    }
    fn peer(saved: &[&str], applied: Option<&[&str]>, allow: bool, receipts: bool) -> Peer {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let models: Vec<_> = saved.iter().map(|id| json!({"model_id":id,"modality":"text","selection":{"roles":["request_interpreter"]}})).collect();
        std::fs::write(&path, serde_json::to_vec(&json!({"node_id":"node","permissions":{},"management":{"allow_remote":allow},"models":{"provider":"ollama","ollama":{"selected":models}}})).unwrap()).unwrap();
        let core = eefn::NodeService::new(path, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        {
            let mut live = core.live.lock().unwrap();
            live["runtime_id"] = json!(runtime);
            live["connection"] = json!({"state":"connected","transport":"pinned_tls13"});
            live["execution_grants"] = json!({"schema_version":1,"available":receipts});
        }
        let mut engine = eefn::NodeEngine::new(false, vec![], dir.path().into()).unwrap();
        if let Some(models) = applied {
            engine = engine
                .with_ollama(
                    "http://127.0.0.1:1".into(),
                    models.iter().map(|id| eefn::SelectedModel {
                        model_id: (*id).into(),
                        modality: "text".into(),
                        hints: eefn::model_selection::ModelHints {
                            roles: Some(vec!["request_interpreter".into()]),
                            ..Default::default()
                        },
                    }),
                )
                .unwrap();
        }
        let engine = Arc::new(engine);
        let _guard = applied.map(|_| core.attach_preview_runtime(&engine, runtime.clone()));
        let request_id = uuid::Uuid::new_v4().to_string();
        let setup = core
            .setup_check(eefn::setup_check::CheckRequest {
                schema_version: 1,
                expected_node_id: "node".into(),
                expected_runtime_id: runtime.clone(),
                request_id: request_id.clone(),
            })
            .unwrap();
        let permissions = applied.map(|_| {
            core.permission_command(
                eefn::permission_commands::PermissionRequest {
                    schema_version: 1,
                    expected_node_id: "node".into(),
                    expected_runtime_id: runtime.clone(),
                    command: eefn::permission_commands::PermissionCommand::Show {},
                },
                false,
            )
            .unwrap()
        });
        Peer {
            setup: Some(
                json!({"schema_version":1,"report_type":"remote_node_setup","node_id":"node","request_id":request_id,"runtime_id":runtime,
                "success":true,"inspection_requested":true,"result":setup,"changed":false,"model_inference_requested":false,
                "execution_authorized":false,"restart_requested":false,"automatically_retried":false}),
            ),
            permissions: permissions.map(|data| json!({"success":true,"data":data})),
            calls: Mutex::new(vec![]),
        }
    }
    #[tokio::test]
    async fn selected_start_reads_once_and_pins_permissions_to_observed_runtime() {
        let peer = peer(&["chosen"], Some(&["chosen"]), true, true);
        let operation = uuid::Uuid::new_v4().to_string();
        let definition = selected_on(&peer, "node", &operation).await.unwrap();
        assert_eq!(definition.model.model_id, "chosen");
        assert_eq!(definition.operation_id, operation);
        let calls = peer.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["command"], json!({"operation":"show"}));
        assert_eq!(calls[1]["expected_runtime_id"], definition.node_runtime_id);
        assert!(calls[0]["expected_runtime_id"].is_null());
    }
    #[tokio::test]
    async fn fresh_selection_explains_missing_ambiguous_pending_and_unavailable_without_mutation() {
        for (saved, applied, receipts, issue) in [
            (
                vec![],
                Some(vec![]),
                true,
                ReadinessIssue::MissingInterpreter,
            ),
            (
                vec!["one", "two"],
                Some(vec!["one", "two"]),
                true,
                ReadinessIssue::AmbiguousInterpreter,
            ),
            (
                vec!["new"],
                Some(vec!["old"]),
                true,
                ReadinessIssue::ApplySavedInterpreter,
            ),
            (vec!["one"], None, true, ReadinessIssue::RuntimeUnavailable),
            (
                vec!["one"],
                Some(vec!["one"]),
                false,
                ReadinessIssue::ExecutionTransportUnavailable,
            ),
        ] {
            let peer = peer(&saved, applied.as_deref(), false, receipts);
            assert_eq!(
                selected_on(&peer, "node", &uuid::Uuid::new_v4().to_string())
                    .await
                    .unwrap_err(),
                issue
            );
            assert_eq!(
                peer.calls.lock().unwrap().len(),
                1,
                "No permission/work request before confirmed selection"
            );
        }
        let peer = peer(&["chosen"], Some(&["chosen"]), false, true);
        assert_eq!(
            selected_on(&peer, "node", &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap_err(),
            ReadinessIssue::ManagementConsentRequired
        );
        assert_eq!(peer.calls.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn unconfirmed_reads_never_grant_consent_retry_or_choose_a_fallback() {
        for mode in [
            "lost-setup",
            "unsupported-setup",
            "wrong-setup-node",
            "unknown-setup-field",
            "lost-permission",
            "wrong-permission-runtime",
            "forged-consent",
            "unknown-permission-field",
        ] {
            let mut peer = peer(&["chosen"], Some(&["chosen"]), false, true);
            let setup_failure = match mode {
                "lost-setup" => {
                    peer.setup = None;
                    true
                }
                "unsupported-setup" => {
                    peer.setup.as_mut().unwrap()["schema_version"] = json!(99);
                    true
                }
                "wrong-setup-node" => {
                    peer.setup.as_mut().unwrap()["result"]["node_id"] = json!("other");
                    true
                }
                "unknown-setup-field" => {
                    peer.setup.as_mut().unwrap()["PRIVATE"] = json!("secret");
                    true
                }
                "lost-permission" => {
                    peer.permissions = None;
                    false
                }
                "wrong-permission-runtime" => {
                    peer.permissions.as_mut().unwrap()["data"]["runtime_id"] =
                        json!(uuid::Uuid::new_v4().to_string());
                    false
                }
                "forged-consent" => {
                    peer.permissions.as_mut().unwrap()["data"]["remote_management_allowed"] =
                        json!(true);
                    false
                }
                _ => {
                    peer.permissions.as_mut().unwrap()["data"]["PRIVATE"] = json!("secret");
                    false
                }
            };
            let issue = selected_on(&peer, "node", &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap_err();
            assert_eq!(
                issue,
                if setup_failure {
                    ReadinessIssue::SetupUnconfirmed
                } else {
                    ReadinessIssue::PermissionsUnconfirmed
                },
                "{mode}"
            );
            assert_eq!(
                peer.calls.lock().unwrap().len(),
                if setup_failure { 1 } else { 2 },
                "{mode}"
            );
            assert!(!issue.guidance().contains("PRIVATE"));
        }
    }
    #[tokio::test]
    async fn missing_interpreter_preserves_existing_setup_and_approval_guidance() {
        for (state, step, issue) in [
            (
                eefn::model_setup::State::AwaitingApproval,
                "approve_model_setup",
                ReadinessIssue::SetupApprovalRequired,
            ),
            (
                eefn::model_setup::State::Waiting,
                "inspect_model_setup",
                ReadinessIssue::InspectModelSetup,
            ),
            (
                eefn::model_setup::State::NoApprovedDefault,
                "choose_model_setup",
                ReadinessIssue::NoApprovedDefault,
            ),
        ] {
            let mut peer = peer(&[], Some(&[]), false, true);
            let catalog = (state != eefn::model_setup::State::NoApprovedDefault).then(|| {
                eefn::model_bootstrap::CatalogPin {
                    schema_version: 1,
                    model_id: "fixture".into(),
                    definition_sha256: "a".repeat(64),
                    artifact_sha256: "b".repeat(64),
                    artifact_bytes: 32,
                    source_uri_sha256: "c".repeat(64),
                    source_page_sha256: "d".repeat(64),
                    declared_license: "test-only".into(),
                }
            });
            let mode = if state == eefn::model_setup::State::Waiting {
                eefn::model_bootstrap::Mode::Auto
            } else {
                eefn::model_bootstrap::Mode::Prompt
            };
            let policy = eefn::model_bootstrap::Saved::new(
                eefn::model_bootstrap::Policy {
                    schema_version: 1,
                    mode,
                    automation: true,
                    catalog,
                    restart_after_setup: false,
                },
                0,
            )
            .unwrap();
            let status = eefn::model_setup::Status {
                schema_version: 1,
                node_id: "node".into(),
                policy_digest: policy.digest.clone(),
                state,
                attempt: None,
                current_policy_attempt: false,
                restart_required: false,
                readiness_verified: false,
                automatically_retried: false,
            };
            let result = &mut peer.setup.as_mut().unwrap()["result"];
            result["model_setup"] =
                json!({"availability":"observed","policy":policy,"status":status});
            let steps = result["next_steps"].as_array_mut().unwrap();
            steps.retain(|s| s != "choose_interpreter");
            steps.push(json!(step));
            assert_eq!(
                selected_on(&peer, "node", &uuid::Uuid::new_v4().to_string())
                    .await
                    .unwrap_err(),
                issue
            );
            assert_eq!(peer.calls.lock().unwrap().len(), 1);
        }
        let mut peer = peer(&[], Some(&[]), false, true);
        let result = &mut peer.setup.as_mut().unwrap()["result"];
        result["model_setup"] = json!({"availability":"unavailable"});
        let steps = result["next_steps"].as_array_mut().unwrap();
        steps.retain(|s| s != "choose_interpreter");
        steps.push(json!("inspect_model_setup"));
        assert_eq!(
            selected_on(&peer, "node", &uuid::Uuid::new_v4().to_string())
                .await
                .unwrap_err(),
            ReadinessIssue::InspectModelSetup
        );
        assert_eq!(peer.calls.lock().unwrap().len(), 1);
    }
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
            readiness_issue: None,
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
        let mut selected = q.clone();
        selected.command = Command::StartSelected {
            approve_inference: true,
        };
        selected.validate().unwrap();
        selected.command = Command::StartSelected {
            approve_inference: false,
        };
        assert!(selected.validate().is_err());
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
    fn readiness_reports_require_new_command_and_no_work_or_authority() {
        let mut q = request();
        let legacy = q.clone();
        let mut value = report(&q);
        assert!(
            value.get("readiness_issue").is_none(),
            "Old replies retain their wire shape"
        );
        q.command = Command::StartSelected {
            approve_inference: true,
        };
        for field in [
            "definition",
            "job_id",
            "grant_id",
            "executor_session_id",
            "evidence_source",
        ] {
            value[field] = Value::Null;
        }
        for field in [
            "success",
            "inference_returned_text",
            "source_preparation_requested",
            "owner_approval_requested",
            "execution_requested",
        ] {
            value[field] = json!(false);
        }
        value["state"] = json!("not_ready");
        value["readiness_issue"] = json!("management_consent_required");
        assert!(confirmed(&value, &q));
        assert!(render(&value).contains("eefn permissions allow remote-management"));
        for (field, bad) in [
            ("historical", json!(true)),
            ("source_preparation_requested", json!(true)),
            ("owner_approval_requested", json!(true)),
            ("execution_requested", json!(true)),
            ("success", json!(true)),
            ("outcome_unknown", json!(true)),
            ("job_id", json!("a".repeat(32))),
            ("definition", report(&request())["definition"].clone()),
            ("readiness_issue", json!("setup_unconfirmed")),
            ("readiness_issue", Value::Null),
            ("readiness_issue", json!("PRIVATE")),
            ("next_step", json!("PRIVATE")),
        ] {
            let mut bad_report = value.clone();
            bad_report[field] = bad;
            assert!(!confirmed(&bad_report, &q), "{field}");
        }
        assert!(!confirmed(&value, &legacy));
        q.command = Command::Inspect;
        assert!(!confirmed(&value, &q));
        q.command = Command::StartSelected {
            approve_inference: true,
        };
        value["state"] = json!("unconfirmed");
        value["outcome_unknown"] = json!(true);
        value["readiness_issue"] = json!("setup_unconfirmed");
        assert!(confirmed(&value, &q));
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
            readiness_issue: None,
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
                self.start_model_verification(Some(definition), &mut report)
                    .await
            }
            Command::StartSelected { .. } => self.start_model_verification(None, &mut report).await,
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
        requested: Option<&Definition>,
        report: &mut Report,
    ) -> Result<()> {
        let _owned = self
            .model_verification
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Model-check workflow busy; no queue or retry"))?;
        if let Some(existing) =
            self.model_verification_plan(&report.node_id, &report.operation_id)?
        {
            if let Some(definition) = requested
                && fixed_source(&existing)? != *definition
            {
                bail!("Retained model-check intent conflicts")
            }
            report.historical = true;
            return self.observe_model_verification(report).await;
        }
        // Resolve fresh intent only after checking retained history under the
        // same workflow slot. Reads never grant management or choose a model.
        let selected;
        let definition = if let Some(definition) = requested {
            definition
        } else {
            selected =
                match selected_on(&self.node_server, &report.node_id, &report.operation_id).await {
                    Ok(definition) => definition,
                    Err(issue) => {
                        report.readiness_issue = Some(issue);
                        report.state = if issue.unconfirmed() {
                            State::Unconfirmed
                        } else {
                            State::NotReady
                        };
                        report.outcome_unknown = issue.unconfirmed();
                        return Ok(());
                    }
                };
            &selected
        };
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
