//! Read-only onboarding observations. Configuration is not model/workload health.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NOTE: &str = "Read-only setup inspection, not successful inference or execution authorization. No permission, model, connection or update setting was changed; no download, load, restart, probe or workload was requested.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub request_id: String,
}
impl CheckRequest {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        for id in [&self.expected_runtime_id, &self.request_id] {
            if uuid::Uuid::parse_str(id)
                .map_or(true, |uuid| uuid.is_nil() || uuid.to_string() != *id)
            {
                bail!("setup check requires current canonical runtime/correlation IDs")
            }
        }
        if self.schema_version != 1 {
            bail!("unsupported setup check schema")
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    pub backend: crate::model_selection::Backend,
    pub model_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum InterpreterSelection {
    Missing {},
    Selected { model: ModelRef },
    Ambiguous { count: usize },
    RuntimeUnavailable {},
}
impl InterpreterSelection {
    fn from_candidates(candidates: Vec<ModelRef>) -> Self {
        match candidates.len() {
            0 => Self::Missing {},
            1 => Self::Selected {
                model: candidates.into_iter().next().unwrap(),
            },
            count => Self::Ambiguous { count },
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Selected { model } => {
                crate::model_selection::validate_id(&model.model_id).is_ok()
            }
            Self::Ambiguous { count } => (2..=64).contains(count),
            _ => true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Starting,
    Waiting,
    Paused,
    Checking,
    Connecting,
    Connected,
    Disconnected,
    Retrying,
    Error,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    PinnedTls13,
    LegacySharedKey,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextStep {
    ResumeConnection,
    CheckCoordinator,
    PairPrivate,
    InspectExecutionTransport,
    ChooseInterpreter,
    ChooseOneInterpreter,
    RestartForSavedSettings,
    WaitForRuntime,
    InspectStartupIssues,
    WaitForInputChannel,
    WaitForInputCapacity,
    WaitForRegistrationReceipt,
}

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    success: bool,
    node_id: String,
    runtime_id: String,
    request_id: String,
    connection: ConnectionState,
    transport: Transport,
    #[serde(default)]
    input_channel: Option<crate::submission::InputChannelState>,
    #[serde(default)]
    registration: Option<crate::registration::State>,
    saved_connection_enabled: bool,
    saved_auth_mode: crate::authentication::AuthMode,
    #[serde(deserialize_with = "required_option")]
    execution_receipts_available: Option<bool>,
    saved_interpreter: InterpreterSelection,
    applied_interpreter: InterpreterSelection,
    #[serde(deserialize_with = "required_option")]
    saved_matches_applied: Option<bool>,
    #[serde(deserialize_with = "required_option")]
    pending_restart: Option<bool>,
    startup_issues: Vec<crate::service::StartupIssue>,
    next_steps: Vec<NextStep>,
    changed: bool,
    model_inference_requested: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
impl Report {
    fn next_steps(&self) -> Vec<NextStep> {
        let mut steps = Vec::new();
        if !self.saved_connection_enabled || self.connection == ConnectionState::Paused {
            steps.push(NextStep::ResumeConnection);
        } else if self.connection != ConnectionState::Connected {
            steps.push(NextStep::CheckCoordinator);
        }
        if self.connection == ConnectionState::Connected {
            match self.input_channel {
                Some(crate::submission::InputChannelState::Unavailable) => {
                    steps.push(NextStep::WaitForInputChannel)
                }
                Some(crate::submission::InputChannelState::Busy) => {
                    steps.push(NextStep::WaitForInputCapacity)
                }
                _ => {}
            }
            if matches!(
                self.registration,
                Some(
                    crate::registration::State::AwaitingReceipt
                        | crate::registration::State::Unknown
                )
            ) {
                steps.push(NextStep::WaitForRegistrationReceipt);
            }
        }
        if self.execution_receipts_available != Some(true) {
            if self.saved_auth_mode == crate::authentication::AuthMode::Legacy {
                steps.push(NextStep::PairPrivate);
            } else if self.pending_restart != Some(true) {
                steps.push(NextStep::InspectExecutionTransport);
            }
        }
        match &self.applied_interpreter {
            InterpreterSelection::Missing {} => {
                if matches!(self.saved_interpreter, InterpreterSelection::Missing {}) {
                    steps.push(NextStep::ChooseInterpreter);
                }
            }
            InterpreterSelection::Ambiguous { .. } => steps.push(NextStep::ChooseOneInterpreter),
            InterpreterSelection::RuntimeUnavailable {} => steps.push(NextStep::WaitForRuntime),
            _ => {}
        }
        if matches!(
            self.saved_interpreter,
            InterpreterSelection::Ambiguous { .. }
        ) && !steps.contains(&NextStep::ChooseOneInterpreter)
        {
            steps.push(NextStep::ChooseOneInterpreter);
        }
        if self.pending_restart == Some(true) || self.saved_matches_applied == Some(false) {
            steps.push(NextStep::RestartForSavedSettings);
        }
        if !self.startup_issues.is_empty() {
            steps.push(NextStep::InspectStartupIssues);
        }
        steps
    }
    fn valid(&self, request: &CheckRequest) -> bool {
        let matching = match (&self.saved_interpreter, &self.applied_interpreter) {
            (_, InterpreterSelection::RuntimeUnavailable {}) => None,
            (InterpreterSelection::Ambiguous { .. }, _)
            | (_, InterpreterSelection::Ambiguous { .. }) => None,
            (saved, applied) => Some(saved == applied),
        };
        request.validate().is_ok()
            && self.schema_version == 1
            && self.report_type == "node_setup_check"
            && self.success
            && self.node_id == request.expected_node_id
            && self.runtime_id == request.expected_runtime_id
            && self.request_id == request.request_id
            && (self.registration != Some(crate::registration::State::Accepted)
                || (self.connection == ConnectionState::Connected
                    && self.input_channel
                        != Some(crate::submission::InputChannelState::Unavailable)))
            && self.saved_interpreter.valid()
            && self.applied_interpreter.valid()
            && !matches!(
                self.saved_interpreter,
                InterpreterSelection::RuntimeUnavailable {}
            )
            && self.saved_matches_applied == matching
            && self.startup_issues.len() <= 4
            && self.next_steps == self.next_steps()
            && !self.changed
            && !self.model_inference_requested
            && !self.execution_authorized
            && !self.restart_requested
            && !self.automatically_retried
            && self.note == NOTE
    }
}

pub fn confirmed(value: &Value, request: &CheckRequest) -> bool {
    serde_json::from_value::<Report>(value.clone()).is_ok_and(|report| report.valid(request))
}

impl crate::NodeService {
    pub fn setup_check(&self, request: CheckRequest) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("setup check targets a different node")
        }
        // Preserve the existing config lock and never save or infer here.
        let config = self.inspect_grant_configuration(|config| Ok(config.clone()))?;
        if config["node_id"] != self.node_id {
            bail!("saved node identity changed")
        }
        let saved = crate::model_selection::selection_view(&config)?;
        let saved = saved
            .into_iter()
            .filter(|model| {
                model["model_metadata"]["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps.contains(&json!("llm.infer")))
                    && model["model_metadata"]["roles"]
                        .as_array()
                        .is_some_and(|roles| roles.contains(&json!("request_interpreter")))
            })
            .map(|model| {
                serde_json::from_value::<ModelRef>(
                    json!({"backend":model["backend"],"model_id":model["model_id"]}),
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let applied = self.setup_interpreter_runtime();
        let live = self.live.lock().unwrap().clone();
        if live["runtime_id"] != request.expected_runtime_id {
            bail!("setup check runtime changed")
        }
        let applied = match applied {
            Some((runtime, candidates)) if runtime == request.expected_runtime_id => {
                InterpreterSelection::from_candidates(
                    candidates
                        .into_iter()
                        .map(|(backend, model_id)| {
                            let backend = match backend {
                                "ollama" => crate::model_selection::Backend::Ollama,
                                "llamacpp" => crate::model_selection::Backend::Llamacpp,
                                _ => bail!("unknown applied interpreter backend"),
                            };
                            Ok(ModelRef { backend, model_id })
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
            }
            _ => InterpreterSelection::RuntimeUnavailable {},
        };
        let saved = InterpreterSelection::from_candidates(saved);
        let matching = match (&saved, &applied) {
            (_, InterpreterSelection::RuntimeUnavailable {}) => None,
            (InterpreterSelection::Ambiguous { .. }, _)
            | (_, InterpreterSelection::Ambiguous { .. }) => None,
            _ => Some(saved == applied),
        };
        let mut report = Report {
            schema_version: 1,
            report_type: "node_setup_check".into(),
            success: true,
            node_id: self.node_id.clone(),
            runtime_id: request.expected_runtime_id.clone(),
            request_id: request.request_id.clone(),
            connection: serde_json::from_value(live["connection"]["state"].clone())
                .unwrap_or(ConnectionState::Unknown),
            transport: serde_json::from_value(live["connection"]["transport"].clone())
                .unwrap_or(Transport::Unknown),
            input_channel: Some(self.submissions.state()),
            registration: Some(crate::registration::State::observe(
                &live,
                &self.node_id,
                self.submissions.state(),
            )),
            saved_connection_enabled: config["connection_enabled"].as_bool().unwrap_or(true),
            saved_auth_mode: serde_json::from_value(
                config.get("auth_mode").cloned().unwrap_or(json!("legacy")),
            )?,
            execution_receipts_available: if live["execution_grants"]["schema_version"] == 1 {
                live["execution_grants"]["available"].as_bool()
            } else {
                None
            },
            saved_interpreter: saved,
            applied_interpreter: applied,
            saved_matches_applied: matching,
            pending_restart: live["pending_restart"].as_bool(),
            startup_issues: live["startup_issues"]
                .as_array()
                .into_iter()
                .flatten()
                .take(4)
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect(),
            next_steps: Vec::new(),
            changed: false,
            model_inference_requested: false,
            execution_authorized: false,
            restart_requested: false,
            automatically_retried: false,
            note: NOTE.into(),
        };
        report.next_steps = report.next_steps();
        if !report.valid(&request) {
            bail!("setup observations are invalid")
        }
        Ok(serde_json::to_value(report)?)
    }
}

/// Render only a confirmed typed report, never arbitrary node-provided prose.
pub fn render(value: &Value) -> Result<String> {
    let report: Report = serde_json::from_value(value.clone())?;
    let request = CheckRequest {
        schema_version: 1,
        expected_node_id: report.node_id.clone(),
        expected_runtime_id: report.runtime_id.clone(),
        request_id: report.request_id.clone(),
    };
    if !report.valid(&request) {
        bail!("unconfirmed setup observations")
    }
    let connection = match report.connection {
        ConnectionState::Connected => "Connected to EEF (not permission to execute work)",
        ConnectionState::Paused => "Connection paused; local node controls remain usable",
        ConnectionState::Unknown => "Connection state unknown",
        _ => "Not connected to EEF yet; local node controls remain usable",
    };
    let receipts = match report.execution_receipts_available {
        Some(true) => "Configured; each workload still needs separate owner approval",
        Some(false) => "Unavailable; connection alone cannot enable reviewed work",
        None => "Unknown; not treated as available",
    };
    let interpreter = match &report.applied_interpreter {
        InterpreterSelection::Selected { model } => format!(
            "Configured: {} / {} (inference and quality not tested)",
            model.backend.name(),
            model.model_id
        ),
        InterpreterSelection::Missing {} => {
            "No interpreter selected in this runtime; explicit commands still work".into()
        }
        InterpreterSelection::Ambiguous { .. } => {
            "More than one interpreter selected; interpretation is refused".into()
        }
        InterpreterSelection::RuntimeUnavailable {} => {
            "Runtime unavailable; saved choices are not running readiness".into()
        }
    };
    let input = match report.input_channel {
        Some(crate::submission::InputChannelState::Ready) => {
            "Local queue ready (not delivery, approval or execution readiness)"
        }
        Some(crate::submission::InputChannelState::Busy) => {
            "Local queue busy; no extra queue or automatic retry is created"
        }
        Some(crate::submission::InputChannelState::Unavailable) => {
            "Local input channel unavailable"
        }
        None => "Unknown on this node build; not assumed ready",
    };
    let mut text = format!(
        "Node setup: {}\nConnection: {connection}\nInput: {input}\nReviewed-work transport: {receipts}\nInterpreter: {interpreter}\n",
        report.node_id
    );
    text.push_str(match report.registration {
        Some(crate::registration::State::Accepted)=>"Registration: EEF accepted this runtime on the current authenticated connection; model, permission and workload readiness are separate.\n",
        Some(crate::registration::State::AwaitingReceipt)=>"Registration: receipt not received; connected alone is not confirmation. Older coordinators may not support receipts.\n",
        Some(crate::registration::State::NotConnected)=>"Registration: no current connected session.\n",
        _=>"Registration: unknown on this runtime/build; not assumed accepted.\n",
    });
    for issue in &report.startup_issues {
        use crate::service::StartupIssue;
        text.push_str(match issue {
            StartupIssue::PythonRuntimeUnavailable => "Python did not initialize. Repair this node package if bundled Python is missing; otherwise inspect the explicit interpreter. Local core commands still work.\n",
            StartupIssue::BuiltinPluginUnavailable => "An enabled built-in plugin did not initialize. Repair this node package if its bundled plugins or dependencies are missing.\n",
            StartupIssue::CustomPluginUnavailable => "An owner-configured plugin did not initialize. Inspect its configured source and dependencies; no replacement plugin was selected.\n",
            StartupIssue::LlamacppStartupFailed => "The selected local model runtime did not start. Inspect models show and model-lifecycle show; check the selected model and repair this node package if runtime files are missing.\n",
            StartupIssue::PeerRuntimeUnavailable => "Peer transport did not initialize. Inspect the node's connection and peer configuration before retrying work.\n",
        });
    }
    for step in report.next_steps {
        text.push_str("Next: ");
        text.push_str(match step {
            NextStep::ResumeConnection => "Resume the connection when you want this node to use EEF: eefn connection resume",
            NextStep::CheckCoordinator => "Start/check EEF and your configured connection: eefn connection show",
            NextStep::PairPrivate => "Pair this node with a private EEF invitation; shared-key connection does not support reviewed work",
            NextStep::InspectExecutionTransport => "Inspect private pairing and execution receipts: eefn connection show",
            NextStep::ChooseInterpreter => "Browse installed models, select one and assign its interpreter role: eefn models installed; eefn models interpreter --help",
            NextStep::ChooseOneInterpreter => "Assign the interpreter role to exactly one model: eefn models interpreter --help",
            NextStep::RestartForSavedSettings => "Saved choices differ or settings are pending; explicitly apply them: eefn restart",
            NextStep::WaitForRuntime => "Wait for startup or inspect node diagnostics; do not assume a model is running",
            NextStep::InspectStartupIssues => "Inspect reported startup issues: eefn network diagnose",
            NextStep::WaitForInputChannel => "Wait for the node's input channel or inspect connection show; no input was sent or replayed",
            NextStep::WaitForInputCapacity => "Wait for capacity before a new explicit input decision; do not replay an uncertain request",
            NextStep::WaitForRegistrationReceipt=>"Wait for a current EEF registration receipt or inspect coordinator compatibility; no registration/work was replayed by this check",
        });
        text.push('\n');
    }
    text.push_str("This check did not change settings, enable permissions, download/load a model, restart an app or run work.\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn bundled_components_setup_guidance_is_actionable_without_readiness_or_private_paths() {
        for issue in [
            crate::service::StartupIssue::PythonRuntimeUnavailable,
            crate::service::StartupIssue::BuiltinPluginUnavailable,
            crate::service::StartupIssue::LlamacppStartupFailed,
        ] {
            let (_dir, core, request) = fixture();
            core.record_startup_issue(issue);
            let report = core.setup_check(request).unwrap();
            let text = render(&report).unwrap();
            assert!(
                text.contains("repair this node package")
                    || text.contains("Repair this node package")
            );
            assert!(!text.contains("PRIVATE"));
            assert_eq!(report["changed"], false);
            assert_eq!(report["restart_requested"], false);
        }
    }

    fn fixture() -> (tempfile::TempDir, Arc<crate::NodeService>, CheckRequest) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"node_id":"node", "psk":"PRIVATE",
            "name":"PRIVATE-NAME", "custom":{"keep":true}, "models":{"provider":"auto"}}))
            .unwrap(),
        )
        .unwrap();
        let core = crate::NodeService::new(path, "node".into());
        let request = CheckRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        core.live.lock().unwrap()["runtime_id"] = json!(request.expected_runtime_id);
        (dir, core, request)
    }
    fn engine(dir: &std::path::Path, model: Option<&str>) -> Arc<crate::NodeEngine> {
        let mut engine = crate::NodeEngine::new(false, vec![], dir.into()).unwrap();
        if let Some(model) = model {
            engine = engine
                .with_ollama(
                    "http://127.0.0.1:1".into(),
                    [crate::SelectedModel {
                        model_id: model.into(),
                        modality: "text".into(),
                        hints: crate::model_selection::ModelHints {
                            roles: Some(vec!["request_interpreter".into()]),
                            ..Default::default()
                        },
                    }],
                )
                .unwrap();
        }
        Arc::new(engine)
    }
    #[test]
    fn registration_is_separate_from_models_and_lost_receiver_is_not_accepted() {
        let (_dir, core, request) = fixture();
        let receiver = core.submissions.connect();
        let offer = crate::registration::Offer::new(&request.expected_runtime_id).unwrap();
        let session = uuid::Uuid::new_v4().to_string();
        let receipt = crate::registration::Receipt::new("node", &offer, &session).unwrap();
        {
            let mut live = core.live.lock().unwrap();
            live["connection"] = json!({"state":"connected","transport":"pinned_tls13"});
            live["registration_observation"] = crate::registration::pending(&offer, Some(&session));
            live["registration_observation"]["receipt"] = json!(receipt);
        }
        let accepted = core.setup_check(request.clone()).unwrap();
        assert!(confirmed(&accepted, &request));
        assert_eq!(accepted["registration"], "accepted");
        assert_eq!(accepted["execution_authorized"], false);
        assert!(render(&accepted).unwrap().contains("EEF accepted"));
        let mut older = accepted.clone();
        older.as_object_mut().unwrap().remove("registration");
        assert!(confirmed(&older, &request));
        assert!(
            render(&older)
                .unwrap()
                .contains("unknown on this runtime/build")
        );
        let mut corrupted = accepted;
        corrupted["connection"] = json!("paused");
        assert!(!confirmed(&corrupted, &request));
        drop(receiver);
        let closed = core.setup_check(request.clone()).unwrap();
        assert_eq!(closed["registration"], "unknown");
        assert!(
            closed["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("wait_for_registration_receipt"))
        );
        assert_eq!(core.diagnostics()["registration"], "unknown");
    }
    #[test]
    fn setup_observes_missing_runtime_and_legacy_without_mutating_or_disclosing_secrets() {
        let (dir, core, request) = fixture();
        let before = std::fs::read(&core.config_path).unwrap();
        let live = {
            let mut live = core.live.lock().unwrap();
            live["connection"] = json!({"state":"connected", "transport":"legacy_shared_key", "address":"PRIVATE-ADDRESS", "last_error":"PRIVATE-ERROR"});
            live["execution_grants"] = json!({"schema_version":1, "available":false});
            live.clone()
        };
        let value = core.setup_check(request).unwrap();
        assert_eq!(
            value["applied_interpreter"],
            json!({"state":"runtime_unavailable"})
        );
        assert_eq!(value["saved_matches_applied"], Value::Null);
        assert!(
            value["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("pair_private"))
        );
        assert!(!value.to_string().contains("PRIVATE"));
        assert_eq!(*core.live.lock().unwrap(), live);
        assert_eq!(std::fs::read(&core.config_path).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let text = render(&value).unwrap();
        assert!(text.contains("connection alone cannot enable reviewed work"));
        assert!(text.contains("Runtime unavailable"));
    }
    #[test]
    fn setup_uses_applied_interpreter_and_distinguishes_saved_pending_choices_and_guard_retirement()
    {
        let (dir, core, request) = fixture();
        let runtime = engine(dir.path(), None);
        let guard = core.attach_preview_runtime(&runtime, request.expected_runtime_id.clone());
        let initial = core.setup_check(request.clone()).unwrap();
        assert_eq!(initial["applied_interpreter"], json!({"state":"missing"}));
        assert_eq!(initial["saved_matches_applied"], true);
        let mut config = core.read_config().unwrap();
        crate::model_selection::apply(
            &mut config,
            &crate::model_selection::ModelCommand::SelectOllama {
                model: crate::SelectedModel {
                    model_id: "owner-model".into(),
                    modality: "text".into(),
                    hints: crate::model_selection::ModelHints {
                        roles: Some(vec!["request_interpreter".into()]),
                        ..Default::default()
                    },
                },
            },
        )
        .unwrap();
        core.save_configuration(config).unwrap();
        let saved = core
            .setup_check(CheckRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                ..request.clone()
            })
            .unwrap();
        assert_eq!(saved["saved_matches_applied"], false);
        assert_eq!(
            saved["saved_interpreter"]["model"]["model_id"],
            "owner-model"
        );
        assert_eq!(saved["applied_interpreter"]["state"], "missing");
        assert!(
            saved["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("restart_for_saved_settings"))
        );
        drop(guard);
        let runtime = engine(dir.path(), Some("owner-model"));
        let guard = core.attach_preview_runtime(&runtime, request.expected_runtime_id.clone());
        let applied = core
            .setup_check(CheckRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                ..request.clone()
            })
            .unwrap();
        assert_eq!(applied["saved_matches_applied"], true);
        assert!(
            render(&applied)
                .unwrap()
                .contains("inference and quality not tested")
        );
        drop(guard);
        let retired = core.setup_check(request).unwrap();
        assert_eq!(
            retired["applied_interpreter"]["state"],
            "runtime_unavailable"
        );
        assert_eq!(retired["saved_matches_applied"], Value::Null);
    }
    #[test]
    fn setup_rejects_stale_targets_and_corrupt_reports_without_prose_or_authority_fallback() {
        let (dir, core, request) = fixture();
        let runtime = engine(dir.path(), None);
        let _guard = core.attach_preview_runtime(&runtime, request.expected_runtime_id.clone());
        let value = core.setup_check(request.clone()).unwrap();
        assert!(confirmed(&value, &request));
        for (field, invalid) in [
            ("runtime_id", json!(uuid::Uuid::new_v4().to_string())),
            ("request_id", json!(uuid::Uuid::new_v4().to_string())),
            ("execution_authorized", json!(true)),
            ("model_inference_requested", json!(true)),
            ("changed", json!(true)),
            ("note", json!("PRIVATE-SUBSTITUTED-PROSE")),
            ("next_steps", json!([])),
            (
                "applied_interpreter",
                json!({"state":"missing", "model":{"backend":"ollama","model_id":"invented"}}),
            ),
            ("saved_matches_applied", json!(false)),
        ] {
            let mut bad = value.clone();
            bad[field] = invalid;
            assert!(!confirmed(&bad, &request), "{field}");
            assert!(render(&bad).is_err() || field == "runtime_id" || field == "request_id");
        }
        let mut bad = value.clone();
        bad["secret"] = json!("PRIVATE");
        assert!(!confirmed(&bad, &request));
        for field in [
            "execution_receipts_available",
            "saved_matches_applied",
            "pending_restart",
        ] {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(!confirmed(&bad, &request));
        }
        assert!(
            core.setup_check(CheckRequest {
                expected_node_id: "other".into(),
                ..request.clone()
            })
            .is_err()
        );
        assert!(
            core.setup_check(CheckRequest {
                expected_runtime_id: uuid::Uuid::new_v4().to_string(),
                ..request
            })
            .is_err()
        );
    }
    #[test]
    fn saved_private_pairing_waits_for_explicit_apply_not_reprovisioning() {
        let (dir, core, request) = fixture();
        let engine = engine(dir.path(), None);
        let _guard = core.attach_preview_runtime(&engine, request.expected_runtime_id.clone());
        let certificate = crate::tls::CoordinatorIdentity::generate()
            .unwrap()
            .certificate;
        core.save_credential_pair_for_runtime(
            "127.0.0.1:1",
            &crate::authentication::generate_credential(),
            &certificate,
            Some(&request.expected_runtime_id),
        )
        .unwrap();
        {
            let mut live = core.live.lock().unwrap();
            live["connection"] = json!({"state":"connected","transport":"legacy_shared_key"});
            live["execution_grants"] = json!({"schema_version":1,"available":false});
        }
        let report = core.setup_check(request.clone()).unwrap();
        assert_eq!(report["saved_auth_mode"], "node_credential");
        assert!(
            report["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("restart_for_saved_settings"))
        );
        assert!(
            !report["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("pair_private"))
        );
        assert!(
            !report["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("inspect_execution_transport"))
        );
        core.live.lock().unwrap()["pending_restart"] = json!(false);
        let report = core.setup_check(request).unwrap();
        assert!(
            report["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("inspect_execution_transport"))
        );
        assert!(
            !report["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("pair_private"))
        );
    }

    #[test]
    fn setup_input_observation_does_not_infer_readiness_from_a_connected_label_or_legacy_snapshot()
    {
        let (_dir, core, request) = fixture();
        core.live.lock().unwrap()["connection"]["state"] = json!("connected");
        let unavailable = core.setup_check(request.clone()).unwrap();
        assert_eq!(unavailable["input_channel"], "unavailable");
        assert!(
            unavailable["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("wait_for_input_channel"))
        );
        assert_eq!(core.status()["input_channel"], "unavailable");
        assert_eq!(core.diagnostics()["input_channel"], "unavailable");
        let receiver = core.submissions.connect();
        let ready = core.setup_check(request.clone()).unwrap();
        assert_eq!(ready["input_channel"], "ready");
        assert!(
            render(&ready)
                .unwrap()
                .contains("not delivery, approval or execution readiness")
        );
        assert_eq!(core.status()["input_channel"], "ready");
        assert_eq!(core.diagnostics()["input_channel"], "ready");
        let mut busy = ready.clone();
        busy["input_channel"] = json!("busy");
        busy["next_steps"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!("wait_for_input_capacity"));
        assert!(confirmed(&busy, &request));
        assert!(render(&busy).unwrap().contains("Local queue busy"));
        let mut legacy = ready.clone();
        legacy.as_object_mut().unwrap().remove("input_channel");
        assert!(confirmed(&legacy, &request));
        assert!(
            render(&legacy)
                .unwrap()
                .contains("Unknown on this node build")
        );
        let mut bad = ready;
        bad["input_channel"] = json!("verified_delivery");
        assert!(!confirmed(&bad, &request));
        drop(receiver);
        assert_eq!(
            core.setup_check(request).unwrap()["input_channel"],
            "unavailable"
        );
    }

    #[test]
    fn paused_and_ambiguous_setup_stays_explicit_not_ready_or_automatic_restart() {
        let (dir, core, request) = fixture();
        let mut engine = crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap();
        let models = ["a", "b"].map(|id| crate::SelectedModel {
            model_id: id.into(),
            modality: "text".into(),
            hints: crate::model_selection::ModelHints {
                roles: Some(vec!["request_interpreter".into()]),
                ..Default::default()
            },
        });
        engine = engine
            .with_ollama("http://127.0.0.1:1".into(), models)
            .unwrap();
        let engine = Arc::new(engine);
        let _guard = core.attach_preview_runtime(&engine, request.expected_runtime_id.clone());
        core.live.lock().unwrap()["connection"]["state"] = json!("paused");
        let value = core.setup_check(request).unwrap();
        assert_eq!(
            value["applied_interpreter"],
            json!({"state":"ambiguous", "count":2})
        );
        assert_eq!(value["saved_matches_applied"], Value::Null);
        assert!(
            value["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("choose_one_interpreter"))
        );
        assert!(
            value["next_steps"]
                .as_array()
                .unwrap()
                .contains(&json!("resume_connection"))
        );
        assert_eq!(value["restart_requested"], false);
        assert!(
            render(&value)
                .unwrap()
                .contains("More than one interpreter selected")
        );
    }
}
