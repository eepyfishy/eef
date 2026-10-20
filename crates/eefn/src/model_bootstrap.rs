//! Bootstrap preferences and exact owner-catalog bindings, not a setup runner.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NOTE: &str = "Bootstrap preferences only. Automatic setup and a project-approved default are not available in this build. No download, probe, model selection/load, permission change, restart, workload or retry was requested. Catalog metadata is not license review, runtime compatibility or model-quality approval.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Missing legacy policy never opts an existing installation into downloads.
    #[default]
    Preserve,
    Off,
    Prompt,
    Auto,
    Managed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPin {
    pub schema_version: u32,
    pub model_id: String,
    pub definition_sha256: String,
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    pub source_uri_sha256: String,
    pub source_page_sha256: String,
    pub declared_license: String,
}
impl CatalogPin {
    pub fn validate(&self) -> Result<()> {
        crate::model_selection::validate_id(&self.model_id)?;
        if self.schema_version != 1
            || [
                &self.definition_sha256,
                &self.artifact_sha256,
                &self.source_uri_sha256,
                &self.source_page_sha256,
            ]
            .into_iter()
            .any(|s| !crate::request_result::valid_digest(s))
            || !(1..=i64::MAX as u64).contains(&self.artifact_bytes)
            || self.declared_license.is_empty()
            || self.declared_license.len() > 256
            || self.declared_license.trim() != self.declared_license
            || self.declared_license.chars().any(char::is_control)
        {
            bail!("Invalid saved bootstrap catalog binding; retain configuration")
        }
        Ok(())
    }
    pub fn from_config(config: &Value, model_id: &str) -> Result<Self> {
        crate::model_selection::validate_id(model_id)?;
        let catalog = crate::model_manager::catalog(config);
        let entries = catalog
            .as_array()
            .filter(|a| a.len() <= 64)
            .ok_or_else(|| anyhow::anyhow!("Bootstrap requires a bounded catalog"))?;
        if serde_json::to_vec(&catalog)?.len() > 128 * 1024 {
            bail!("Bootstrap catalog exceeds metadata bound")
        }
        let matches: Vec<_> = entries.iter().filter(|e| e["id"] == model_id).collect();
        if matches.len() != 1 {
            bail!("Choose one unique configured catalog model")
        }
        let entry = matches[0];
        if entry["modality"] != "text" {
            bail!("Bootstrap binding requires a text GGUF catalog entry")
        }
        let artifact = crate::model_manifest::GgufArtifact::from_catalog(entry)?;
        let page = entry["source_page"]
            .as_str()
            .filter(|s| s.len() <= 8192)
            .ok_or_else(|| anyhow::anyhow!("Bootstrap source page is required"))?;
        let url = reqwest::Url::parse(page)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            bail!("Bootstrap source page requires HTTPS without credentials or fragments")
        }
        let license = entry["license"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Declared bootstrap license is required"))?;
        let hash_text = |s: &str| crate::grants::definition_digest(&json!(s));
        let pin = Self {
            schema_version: 1,
            model_id: model_id.into(),
            definition_sha256: crate::grants::definition_digest(entry)?,
            artifact_sha256: artifact.sha256.to_ascii_lowercase(),
            artifact_bytes: artifact.bytes,
            source_uri_sha256: hash_text(&artifact.url)?,
            source_page_sha256: hash_text(page)?,
            declared_license: license.into(),
        };
        pin.validate()?;
        Ok(pin)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u32,
    pub mode: Mode,
    pub catalog: Option<CatalogPin>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: Mode::Preserve,
            catalog: None,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported bootstrap policy schema")
        }
        if let Some(pin) = &self.catalog {
            pin.validate()?;
        }
        Ok(())
    }
    fn from_config(config: &Value) -> Result<Self> {
        let policy: Self =
            serde_json::from_value(config.get("model_bootstrap").cloned().unwrap_or(json!({})))?;
        policy.validate()?;
        Ok(policy)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub policy: Policy,
    pub revision: u64,
    pub digest: String,
}
impl Saved {
    pub fn new(policy: Policy, revision: u64) -> Result<Self> {
        policy.validate()?;
        let digest =
            crate::grants::definition_digest(&json!({"policy":policy,"revision":revision}))?;
        Ok(Self {
            policy,
            revision,
            digest,
        })
    }
    pub fn validate(&self) -> Result<()> {
        if *self != Self::new(self.policy.clone(), self.revision)? {
            bail!("Unconfirmed bootstrap policy version")
        }
        Ok(())
    }
    pub(crate) fn from_config(config: &Value) -> Result<Self> {
        let revision = match config.get("model_bootstrap_revision") {
            None => 0,
            Some(v) => v.as_u64().ok_or_else(|| {
                anyhow::anyhow!("Invalid bootstrap policy revision; retain configuration")
            })?,
        };
        Self::new(Policy::from_config(config)?, revision)
    }
}

/// Central save hook: old clients cannot erase opt-outs or forge/reset CAS state.
pub(crate) fn preserve_revision(old: &Value, next: &mut Value) -> Result<()> {
    if next.get("model_bootstrap").is_none() {
        if let Some(policy) = old.get("model_bootstrap") {
            next["model_bootstrap"] = policy.clone();
        }
    }
    let previous = match Saved::from_config(old) {
        Ok(previous) => previous,
        Err(_) => {
            // A future/corrupt optional setup policy must not disable ordinary
            // owner controls. Preserve it opaquely; never repair or reset its
            // version while saving an unrelated setting.
            if next.get("model_bootstrap") != old.get("model_bootstrap") {
                bail!("Unsupported bootstrap policy cannot be replaced by a generic save")
            }
            if let Some(revision) = old.get("model_bootstrap_revision") {
                next["model_bootstrap_revision"] = revision.clone();
            } else if let Some(object) = next.as_object_mut() {
                object.remove("model_bootstrap_revision");
            }
            return Ok(());
        }
    };
    let incoming = Policy::from_config(next)?;
    let revision = if incoming == previous.policy {
        previous.revision
    } else {
        previous
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Bootstrap revision exhausted; no wrap/reset"))?
    };
    if revision != 0 || old.get("model_bootstrap_revision").is_some() {
        next["model_bootstrap_revision"] = json!(revision);
    } else if let Some(object) = next.as_object_mut() {
        object.remove("model_bootstrap_revision");
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "selection", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    #[default]
    Keep,
    ProjectDefault,
    Catalog {
        model_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Configure {
        previous: Saved,
        mode: Option<Mode>,
        target: Target,
    },
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        if let Self::Configure {
            previous,
            mode,
            target,
        } = self
        {
            previous.validate()?;
            if mode.is_none() && *target == Target::Keep {
                bail!("Choose a bootstrap preference; use show to inspect")
            }
            if let Target::Catalog { model_id } = target {
                crate::model_selection::validate_id(model_id)?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        if self.schema_version != 1 {
            bail!("Unsupported bootstrap policy command schema")
        }
        self.command.validate()
    }
}

#[derive(Clone, Debug, clap::Args)]
pub struct Settings {
    #[arg(long, value_enum)]
    pub mode: Option<Mode>,
    #[arg(long, conflicts_with = "project_default")]
    pub model: Option<String>,
    /// Clear the owner catalog binding; does not approve or install a default.
    #[arg(long, conflicts_with = "model")]
    pub project_default: bool,
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.project_default && self.model.is_some() {
            bail!("Choose a catalog binding or project default, not both")
        }
        if self.mode.is_none() && self.model.is_none() && !self.project_default {
            bail!("Choose a bootstrap preference; use show to inspect")
        }
        if let Some(id) = &self.model {
            crate::model_selection::validate_id(id)?;
        }
        Ok(())
    }
    pub fn command(&self, previous: Saved) -> Result<Command> {
        self.validate()?;
        let target = if self.project_default {
            Target::ProjectDefault
        } else if let Some(id) = &self.model {
            Target::Catalog {
                model_id: id.clone(),
            }
        } else {
            Target::Keep
        };
        let command = Command::Configure {
            previous,
            mode: self.mode,
            target,
        };
        command.validate()?;
        Ok(command)
    }
}
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    Show,
    Configure(Settings),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Binding {
    ProjectDefaultUnavailable,
    Matched,
    ChangedOrUnavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Preserved,
    Disabled,
    Managed,
    InterpreterSelected,
    AmbiguousInterpreter,
    ExistingSelections,
    NoApprovedDefault,
    CatalogChanged,
    AutomationUnavailable,
}
fn state(policy: &Policy, binding: Binding, selections: usize, interpreters: usize) -> State {
    match policy.mode {
        Mode::Preserve => State::Preserved,
        Mode::Off => State::Disabled,
        Mode::Managed => State::Managed,
        _ if interpreters > 1 => State::AmbiguousInterpreter,
        _ if interpreters == 1 => State::InterpreterSelected,
        _ if selections > 0 => State::ExistingSelections,
        _ => match binding {
            Binding::ProjectDefaultUnavailable => State::NoApprovedDefault,
            Binding::ChangedOrUnavailable => State::CatalogChanged,
            Binding::Matched => State::AutomationUnavailable,
        },
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    command: Command,
    success: bool,
    saved: Saved,
    binding: Binding,
    state: State,
    selection_count: usize,
    interpreter_count: usize,
    changed: bool,
    automatic_setup_available: bool,
    project_default_approved: bool,
    license_reviewed: bool,
    runtime_compatibility_verified: bool,
    download_started: bool,
    selection_changed: bool,
    model_inference_requested: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || r.saved.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "node_model_bootstrap_policy"
        || !r.success
        || r.node_id != request.expected_node_id
        || r.runtime_id != request.expected_runtime_id
        || r.command != request.command
        || r.selection_count > 64
        || r.interpreter_count > r.selection_count
        || r.state
            != state(
                &r.saved.policy,
                r.binding,
                r.selection_count,
                r.interpreter_count,
            )
        || (r.saved.policy.catalog.is_none() != (r.binding == Binding::ProjectDefaultUnavailable))
        || r.automatic_setup_available
        || r.project_default_approved
        || r.license_reviewed
        || r.runtime_compatibility_verified
        || r.download_started
        || r.selection_changed
        || r.model_inference_requested
        || r.execution_authorized
        || r.restart_requested
        || r.automatically_retried
        || r.note != NOTE
    {
        return false;
    }
    match &request.command {
        Command::Show => !r.changed,
        Command::Configure {
            previous,
            mode,
            target,
        } => {
            let changed = r.saved.policy != previous.policy;
            r.changed == changed
                && previous.revision.checked_add(u64::from(changed)) == Some(r.saved.revision)
                && r.saved.policy.mode == mode.unwrap_or(previous.policy.mode)
                && match target {
                    Target::Keep => r.saved.policy.catalog == previous.policy.catalog,
                    Target::ProjectDefault => r.saved.policy.catalog.is_none(),
                    Target::Catalog { model_id } => r
                        .saved
                        .policy
                        .catalog
                        .as_ref()
                        .is_some_and(|p| p.model_id == *model_id),
                }
        }
    }
}

impl crate::NodeService {
    pub fn model_bootstrap_policy(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Bootstrap preferences target another node")
        }
        self.with_permission_runtime(&request.expected_runtime_id, |_| {
            self.inspect_grant_configuration(|config| {
                let live = self.live.lock().unwrap();
                if config["node_id"] != self.node_id
                    || live["runtime_id"] != request.expected_runtime_id
                {
                    bail!("Bootstrap preferences runtime/identity changed")
                }
                let mut saved = Saved::from_config(config)?;
                let mut changed = false;
                if let Command::Configure {
                    previous,
                    mode,
                    target,
                } = &request.command
                {
                    if remote && config.pointer("/management/allow_remote") != Some(&json!(true)) {
                        bail!(
                            "Current local owner approval required for remote bootstrap preferences"
                        )
                    }
                    if *previous != saved {
                        bail!("Bootstrap preferences changed; inspect before deciding what to do")
                    }
                    let mut policy = saved.policy.clone();
                    if let Some(mode) = mode {
                        policy.mode = *mode;
                    }
                    match target {
                        Target::Keep => {}
                        Target::ProjectDefault => policy.catalog = None,
                        Target::Catalog { model_id } => {
                            policy.catalog = Some(CatalogPin::from_config(config, model_id)?)
                        }
                    }
                    policy.validate()?;
                    changed = policy != saved.policy;
                    if changed {
                        let mut next = config.clone();
                        next["model_bootstrap"] = json!(policy);
                        self.save_config_locked(next)?;
                        saved = Saved::from_config(&self.read_config()?)?;
                    }
                }
                let binding = match &saved.policy.catalog {
                    None => Binding::ProjectDefaultUnavailable,
                    Some(pin)
                        if CatalogPin::from_config(config, &pin.model_id)
                            .is_ok_and(|current| current == *pin) =>
                    {
                        Binding::Matched
                    }
                    _ => Binding::ChangedOrUnavailable,
                };
                let selections = crate::model_selection::selection_view(config)?;
                let interpreter_count = selections
                    .iter()
                    .filter(|s| {
                        serde_json::from_value::<crate::model_metadata::ModelMetadata>(
                            s["model_metadata"].clone(),
                        )
                        .is_ok_and(|m| {
                            m.supports("llm.infer")
                                && m.input_modalities.iter().any(|v| v == "text")
                                && m.output_modalities.iter().any(|v| v == "text")
                                && m.roles.as_ref().is_some_and(|roles| {
                                    roles.iter().any(|r| r == "request_interpreter")
                                })
                        })
                    })
                    .count();
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_model_bootstrap_policy".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    success: true,
                    state: state(&saved.policy, binding, selections.len(), interpreter_count),
                    saved,
                    binding,
                    selection_count: selections.len(),
                    interpreter_count,
                    changed,
                    automatic_setup_available: false,
                    project_default_approved: false,
                    license_reviewed: false,
                    runtime_compatibility_verified: false,
                    download_started: false,
                    selection_changed: false,
                    model_inference_requested: false,
                    execution_authorized: false,
                    restart_requested: false,
                    automatically_retried: false,
                    note: NOTE.into(),
                });
                if !confirmed(&report, &request) {
                    bail!("Bootstrap preferences reply unconfirmed; inspect saved state")
                }
                Ok(report)
            })
        })
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    if !confirmed(
        value,
        &Request {
            schema_version: 1,
            expected_node_id: r.node_id.clone(),
            expected_runtime_id: r.runtime_id.clone(),
            command: r.command.clone(),
        },
    ) {
        bail!("Unconfirmed bootstrap preferences report")
    }
    Ok(format!(
        "Node {} model bootstrap\nSaved mode: {:?}\nCatalog binding: {}\nState: {:?}\n{}\n",
        r.node_id,
        r.saved.policy.mode,
        r.saved
            .policy
            .catalog
            .as_ref()
            .map(|p| p.model_id.as_str())
            .unwrap_or("project default (not approved)"),
        r.state,
        NOTE
    ))
}

#[cfg(test)]
#[path = "model_bootstrap_tests.rs"]
mod tests;
