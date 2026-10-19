//! Owner-controlled Python bridge budgets. Configuration is not execution authority.
use crate::python::BridgeLimits;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NOTE: &str = "Python bridge preferences only. Saved limits apply after a separate node restart; no permissions, plugins, pairing, model, workload or update setting changed. No probe, process, restart or retry was started. Job memory means Windows owned-job commit, not RAM/VRAM usage or a sandbox.";

#[derive(Clone, Debug, clap::Args)]
pub struct Settings {
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=300))]
    pub timeout_seconds: Option<u64>,
    #[arg(long, value_parser=clap::value_parser!(u64).range(1024..=67108864))]
    pub max_request_bytes: Option<u64>,
    #[arg(long, value_parser=clap::value_parser!(u64).range(1024..=67108864))]
    pub max_stdout_bytes: Option<u64>,
    #[arg(long, value_parser=clap::value_parser!(u64).range(1024..=1048576))]
    pub max_stderr_bytes: Option<u64>,
    /// Windows-only aggregate commit ceiling for each bridge process tree.
    #[arg(long, conflicts_with="default_memory", value_parser=clap::value_parser!(u64).range(64..=1048576))]
    pub memory_mib: Option<u64>,
    /// Remove the optional commit ceiling; other limits remain unchanged.
    #[arg(long, conflicts_with = "memory_mib")]
    pub default_memory: bool,
}
impl Settings {
    pub fn merge(&self, mut limits: BridgeLimits) -> Result<BridgeLimits> {
        if self.timeout_seconds.is_none()
            && self.max_request_bytes.is_none()
            && self.max_stdout_bytes.is_none()
            && self.max_stderr_bytes.is_none()
            && self.memory_mib.is_none()
            && !self.default_memory
        {
            bail!("Choose at least one Python limit; use show to inspect")
        }
        if self.memory_mib.is_some() && self.default_memory {
            bail!("Choose a memory ceiling or explicit default, not both")
        }
        if let Some(value) = self.timeout_seconds {
            limits.timeout_seconds = value;
        }
        for (target, value) in [
            (&mut limits.max_request_bytes, self.max_request_bytes),
            (&mut limits.max_stdout_bytes, self.max_stdout_bytes),
            (&mut limits.max_stderr_bytes, self.max_stderr_bytes),
        ] {
            if let Some(value) = value {
                *target = usize::try_from(value)?;
            }
        }
        if self.default_memory {
            limits.max_job_memory_mib = None;
        } else if let Some(value) = self.memory_mib {
            limits.max_job_memory_mib = Some(value);
        }
        limits.validate()?;
        Ok(limits)
    }
}
#[derive(Debug, clap::Subcommand)]
pub enum Action {
    /// Inspect saved/applied limits without running Python or opening devices.
    Show,
    /// Save bridge budgets only; restart separately to apply all pending settings.
    Configure(Settings),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show,
    Configure {
        limits: BridgeLimits,
        expected_digest: String,
    },
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
            bail!("Unsupported Python policy schema")
        }
        if let Command::Configure {
            limits,
            expected_digest,
        } = &self.command
        {
            limits.validate()?;
            if !crate::request_result::valid_digest(expected_digest) {
                bail!("Exact saved Python limit digest is required")
            }
        }
        Ok(())
    }
}
fn digest(limits: &BridgeLimits) -> Result<String> {
    crate::grants::definition_digest(&json!(limits))
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
    saved: BridgeLimits,
    #[serde(deserialize_with = "required_option")]
    applied: Option<BridgeLimits>,
    saved_digest: String,
    memory_ceiling_supported: bool,
    changed: bool,
    restart_required: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
fn required_option<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<BridgeLimits>, D::Error> {
    Option::<BridgeLimits>::deserialize(d)
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "node_python_policy"
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && r.command == request.command
        && r.success
        && r.saved.validate().is_ok()
        && r.applied
            .as_ref()
            .is_none_or(|limits| limits.validate().is_ok())
        && digest(&r.saved).is_ok_and(|digest| digest == r.saved_digest)
        && (r.memory_ceiling_supported
            || r.applied
                .as_ref()
                .is_none_or(|l| l.max_job_memory_mib.is_none()))
        && (r.applied.as_ref().is_none_or(|a| a == &r.saved) || r.restart_required)
        && !r.execution_authorized
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == NOTE
        && match &request.command {
            Command::Show => !r.changed,
            Command::Configure {
                limits,
                expected_digest,
            } => {
                r.saved == *limits
                    && r.changed == (r.saved_digest != *expected_digest)
                    && (!r.changed || r.restart_required)
                    && (r.memory_ceiling_supported || r.saved.max_job_memory_mib.is_none())
            }
        }
}
impl crate::NodeService {
    pub fn python_policy(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        if request.expected_node_id != self.node_id {
            bail!("Python policy targets another node")
        }
        self.with_permission_runtime(&request.expected_runtime_id, |_engine| {
            self.inspect_grant_configuration(|config| {
                let mut live = self.live.lock().unwrap();
                if config["node_id"] != self.node_id
                    || live["runtime_id"] != request.expected_runtime_id
                {
                    bail!("Python policy runtime/identity changed")
                }
                let mut saved: BridgeLimits = serde_json::from_value(
                    config.get("python_bridge").cloned().unwrap_or(json!({})),
                )?;
                saved.validate()?;
                let applied: Option<BridgeLimits> = serde_json::from_value(
                    live.get("python_bridge").cloned().unwrap_or(Value::Null),
                )?;
                if let Some(limits) = &applied {
                    limits.validate()?;
                }
                let mut changed = false;
                if let Command::Configure {
                    limits,
                    expected_digest,
                } = &request.command
                {
                    if remote && config.pointer("/management/allow_remote") != Some(&json!(true)) {
                        bail!(
                            "Current local owner approval required for remote Python policy changes"
                        )
                    }
                    crate::model_memory::supported(limits.max_job_memory_mib)?;
                    if digest(&saved)? != *expected_digest {
                        bail!("Python policy changed; inspect before deciding what to do")
                    }
                    changed = saved != *limits;
                    if changed {
                        let mut next = config.clone();
                        next["python_bridge"] = json!(limits);
                        self.save_config_locked(next)?;
                        live["pending_restart"] = json!(true);
                        saved = limits.clone();
                    }
                }
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_python_policy".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    success: true,
                    saved_digest: digest(&saved)?,
                    restart_required: live["pending_restart"] == true
                        || applied.as_ref().is_some_and(|a| a != &saved),
                    saved,
                    applied,
                    memory_ceiling_supported: cfg!(windows),
                    changed,
                    execution_authorized: false,
                    restart_requested: false,
                    automatically_retried: false,
                    note: NOTE.into(),
                });
                if !confirmed(&report, &request) {
                    bail!("Python policy reply unconfirmed; inspect saved state")
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
        bail!("Unconfirmed Python policy report")
    }
    let describe = |l: &BridgeLimits| {
        format!(
            "{}s; request {} bytes; stdout {} bytes; stderr {} bytes; job commit {}",
            l.timeout_seconds,
            l.max_request_bytes,
            l.max_stdout_bytes,
            l.max_stderr_bytes,
            l.max_job_memory_mib
                .map(|n| format!("{n} MiB"))
                .unwrap_or_else(|| "no explicit ceiling".into())
        )
    };
    Ok(format!(
        "Node {} Python bridge\nSaved: {}\nApplied: {}\nMemory ceiling supported: {}\nRestart required: {}\n{}\n",
        r.node_id,
        describe(&r.saved),
        r.applied
            .as_ref()
            .map(describe)
            .unwrap_or_else(|| "Python bridge not active".into()),
        r.memory_ceiling_supported,
        r.restart_required,
        NOTE
    ))
}

#[cfg(test)]
#[path = "python_policy_tests.rs"]
mod tests;
