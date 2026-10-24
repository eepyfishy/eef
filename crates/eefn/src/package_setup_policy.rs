//! Runtime-bound policy inspection/configuration. No remote run/approval API.
use super::{Context, Mode, Policy};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const NOTE: &str = "Shared node-role installation policy, not a per-config policy. Saving an active policy creates a new one-attempt identity that the installed supervisor may evaluate. This command does not run or approve a repair, download a model, restart, upgrade or verify runtime health. Prerelease automation remains blocked. Inspect after uncertainty; no retry or replay.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub revision: u64,
    pub digest: String,
}
pub(super) fn version(policy: &Policy) -> Result<Version> {
    Ok(Version {
        revision: policy.revision,
        digest: format!("{:x}", Sha256::digest(serde_json::to_vec(policy)?)),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Configure {
        previous: Version,
        mode: Mode,
        allow_automatic_repair: bool,
        manifest: Option<String>,
    },
}
impl Command {
    pub fn validate(&self) -> Result<()> {
        if let Self::Configure {
            previous,
            mode,
            allow_automatic_repair,
            manifest,
        } = self
        {
            if !super::digest_valid(&previous.digest) {
                bail!("Exact inspected package policy digest required")
            }
            if (*mode == Mode::Auto) != *allow_automatic_repair {
                bail!("Auto requires distinct explicit repair consent")
            }
            if *mode == Mode::Off && manifest.is_some() {
                bail!("Off does not accept a source")
            }
            if let Some(source) = manifest {
                if source.len() > 8192 {
                    bail!("Public source too long")
                }
                super::public_source(source)?;
            }
        }
        Ok(())
    }
}

/// Only policy show/configure. Deliberately excludes local run and approve.
#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Read the shared node installation policy, without starting maintenance.
    Show,
    /// Save one-attempt maintenance policy; requires separate local node consent.
    Configure {
        #[arg(long, value_enum)]
        mode: Mode,
        #[arg(long)]
        allow_automatic_repair: bool,
        #[arg(long)]
        manifest: Option<String>,
    },
}
impl Action {
    pub fn command(&self, previous: Version) -> Result<Command> {
        let command = match self {
            Self::Show => Command::Show,
            Self::Configure {
                mode,
                allow_automatic_repair,
                manifest,
            } => Command::Configure {
                previous,
                mode: *mode,
                allow_automatic_repair: *allow_automatic_repair,
                manifest: manifest.clone(),
            },
        };
        command.validate()?;
        Ok(command)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub request_id: String,
    pub command: Command,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported package policy schema")
        }
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        crate::automation_occurrence::uuid(&self.request_id)?;
        self.command.validate()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    request_id: String,
    command: Command,
    remote: bool,
    success: bool,
    policy: Policy,
    version: Version,
    running_version: String,
    prerelease_automation_blocked: bool,
    applicability: Applicability,
    remote_management_allowed: bool,
    remote_policy_allowed: bool,
    saved: bool,
    repair_started_by_command: bool,
    repair_approved: bool,
    restart_requested: bool,
    runtime_health_checked: bool,
    model_inference_requested: bool,
    automatically_retried: bool,
    note: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Applicability {
    Off,
    PrereleaseBlocked,
    InvokedBuildMatches,
    ReauthorizationRequired,
    InvokedBuildUnavailable,
}
fn applicability(ctx: &Context, policy: &Policy) -> Applicability {
    if policy.mode == Mode::Off {
        return Applicability::Off;
    }
    if ctx.version.contains('-') {
        return Applicability::PrereleaseBlocked;
    }
    match ctx.build() {
        Ok(build) if policy.binding.as_ref().is_some_and(|b| b.build == build) => {
            Applicability::InvokedBuildMatches
        }
        Ok(_) => Applicability::ReauthorizationRequired,
        Err(_) => Applicability::InvokedBuildUnavailable,
    }
}

fn valid_policy(policy: &Policy) -> bool {
    if policy.revision == 0 {
        policy.schema_version == 1
            && policy.program == "eefn"
            && policy.mode == Mode::Off
            && policy.policy_id.is_none()
            && policy.binding.is_none()
    } else {
        super::validate_policy_role("eefn", policy).is_ok()
    }
}
pub fn confirmed(value: &Value, request: &Request, remote: bool) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if request.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "node_repair_setup_policy"
        || !r.success
        || r.node_id != request.expected_node_id
        || r.runtime_id != request.expected_runtime_id
        || r.request_id != request.request_id
        || r.command != request.command
        || r.remote != remote
        || !valid_policy(&r.policy)
        || version(&r.policy).map_or(true, |v| v != r.version)
        || crate::updater::validate_version(&r.running_version).is_err()
        || r.prerelease_automation_blocked != r.running_version.contains('-')
        || r.repair_started_by_command
        || r.repair_approved
        || r.restart_requested
        || r.runtime_health_checked
        || r.model_inference_requested
        || r.automatically_retried
        || r.note != NOTE
    {
        return false;
    }
    let valid_applicability = if r.policy.mode == Mode::Off {
        r.applicability == Applicability::Off
    } else if r.prerelease_automation_blocked {
        r.applicability == Applicability::PrereleaseBlocked
    } else {
        matches!(
            r.applicability,
            Applicability::ReauthorizationRequired | Applicability::InvokedBuildUnavailable
        ) || r.applicability == Applicability::InvokedBuildMatches
            && r.policy
                .binding
                .as_ref()
                .is_some_and(|b| b.build.version == r.running_version)
    };
    if !valid_applicability {
        return false;
    }
    match &request.command {
        Command::Show => !r.saved,
        Command::Configure {
            previous,
            mode,
            manifest,
            ..
        } => {
            r.saved
                && (!remote || r.remote_management_allowed && r.remote_policy_allowed)
                && previous.revision.checked_add(1) == Some(r.policy.revision)
                && r.policy.mode == *mode
                && r.version.digest != previous.digest
                && (*mode != Mode::Auto || !r.prerelease_automation_blocked)
                && manifest.as_ref().is_none_or(|source| {
                    r.policy
                        .binding
                        .as_ref()
                        .is_some_and(|binding| binding.public_source == *source)
                })
                && r.policy
                    .binding
                    .as_ref()
                    .is_none_or(|binding| binding.build.version == r.running_version)
        }
    }
}

fn execute(
    service: &crate::NodeService,
    ctx: &Context,
    request: &Request,
    remote: bool,
) -> Result<Value> {
    request.validate()?;
    if request.expected_node_id != service.node_id || ctx.program != "eefn" {
        bail!("Package policy command targets another node or role")
    }
    // Runtime retirement and local consent saves serialize with publication.
    // Never hold these locks across network I/O (configure performs none).
    service.with_permission_runtime(&request.expected_runtime_id, |_| {
        service.inspect_grant_configuration(|config| {
            if config["node_id"] != service.node_id
                || service.live.lock().unwrap()["runtime_id"] != request.expected_runtime_id
            {
                bail!("Package policy runtime or identity changed")
            }
            let approval = |key: &str| -> Result<bool> {
                match config.pointer(key) {
                    None => Ok(false),
                    Some(Value::Bool(value)) => Ok(*value),
                    _ => bail!("Invalid local package policy consent"),
                }
            };
            if config
                .get("management")
                .is_some_and(|value| !value.is_object())
            {
                bail!("Invalid local management settings")
            }
            let management = approval("/management/allow_remote")?;
            let consent = approval("/management/allow_remote_repair_setup_policy")?;
            let policy = match &request.command {
                Command::Show => super::load_policy(ctx)?.0,
                Command::Configure {
                    previous,
                    mode,
                    allow_automatic_repair,
                    manifest,
                } => {
                    if remote && !(management && consent) {
                        bail!("Local approval of shared installation policy management required")
                    }
                    let saved = super::configure_checked(
                        ctx,
                        *mode,
                        *allow_automatic_repair,
                        manifest.as_deref(),
                        Some(previous.revision),
                        Some(&previous.digest),
                    )?;
                    serde_json::from_value(saved["policy"].clone())?
                }
            };
            let report = json!(Report {
                schema_version: 1,
                report_type: "node_repair_setup_policy".into(),
                node_id: service.node_id.clone(),
                runtime_id: request.expected_runtime_id.clone(),
                request_id: request.request_id.clone(),
                command: request.command.clone(),
                remote,
                success: true,
                applicability: applicability(ctx, &policy),
                version: version(&policy)?,
                policy,
                running_version: ctx.version.clone(),
                prerelease_automation_blocked: ctx.version.contains('-'),
                remote_management_allowed: management,
                remote_policy_allowed: consent,
                saved: !matches!(request.command, Command::Show),
                repair_started_by_command: false,
                repair_approved: false,
                restart_requested: false,
                runtime_health_checked: false,
                model_inference_requested: false,
                automatically_retried: false,
                note: NOTE.into(),
            });
            if !confirmed(&report, request, remote) {
                bail!("Package policy response unconfirmed; inspect, do not retry")
            }
            Ok(report)
        })
    })
}

impl crate::NodeService {
    pub async fn repair_setup_policy(
        self: &std::sync::Arc<Self>,
        request: Request,
        remote: bool,
    ) -> Result<Value> {
        request.validate()?;
        let service = self.clone();
        self.operate_package_policy(move || {
            execute(&service, &Context::current("eefn")?, &request, remote)
        })
        .await
    }
    async fn operate_package_policy<T: Send + 'static>(
        self: &std::sync::Arc<Self>,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let owned = self
            .package_policy_operation
            .clone()
            .try_lock_owned()
            .map_err(|_| anyhow::anyhow!("Package policy operation busy; no queue or retry"))?;
        tokio::task::spawn_blocking(move || {
            let _owned = owned;
            operation()
        })
        .await
        .map_err(|_| anyhow::anyhow!("Package policy worker unavailable; inspect, no retry"))?
    }
}

pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    let request = Request {
        schema_version: 1,
        expected_node_id: r.node_id.clone(),
        expected_runtime_id: r.runtime_id.clone(),
        request_id: r.request_id.clone(),
        command: r.command.clone(),
    };
    if !confirmed(value, &request, r.remote) {
        bail!("Unconfirmed package policy report")
    }
    Ok(format!(
        "Node {} - shared installation maintenance policy\nMode: {:?}; revision: {}\nApplicability: {:?} (not runtime health)\nRemote management: {}; maintenance policy consent: {}\nPrerelease automation blocked: {}\nSaved: {}\n{}\n",
        r.node_id,
        r.policy.mode,
        r.version.revision,
        r.applicability,
        r.remote_management_allowed,
        r.remote_policy_allowed,
        r.prerelease_automation_blocked,
        r.saved,
        NOTE
    ))
}

#[cfg(test)]
#[path = "package_setup_policy_tests.rs"]
mod tests;
