//! Owner-controlled Python bridge budgets. Configuration is not execution authority.
use crate::python::BridgeLimits;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const NOTE: &str = "Python bridge preferences only. Saved limits apply after a separate node restart; no permissions, plugins, pairing, model, workload or update setting changed. No probe, process, restart or retry was started. Job memory means Windows owned-job commit, not RAM/VRAM usage or a sandbox.";
const CPU_NOTE: &str = " CPU means an optional hard rate ceiling per owned Python job including descendants, relative to any enclosing job's CPU allotment; not node-wide allocation, reserved capacity or adaptive reclamation. Runtime support must be confirmed before each process starts.";

fn cpu_relevant(saved: &BridgeLimits, applied: Option<&BridgeLimits>) -> bool {
    saved.max_job_cpu_percent.is_some()
        || applied.is_some_and(|limits| limits.max_job_cpu_percent.is_some())
}
fn note(cpu: bool) -> String {
    if cpu {
        format!("{NOTE}{CPU_NOTE}")
    } else {
        NOTE.into()
    }
}

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
    /// Windows-only hard CPU rate ceiling per owned Python process job (1-100%).
    #[arg(long, conflicts_with="default_cpu", value_parser=clap::value_parser!(u32).range(1..=100))]
    pub cpu_percent: Option<u32>,
    /// Remove only the optional CPU ceiling; other limits remain unchanged.
    #[arg(long, conflicts_with = "cpu_percent")]
    pub default_cpu: bool,
}
impl Settings {
    pub fn merge(&self, mut limits: BridgeLimits) -> Result<BridgeLimits> {
        if self.timeout_seconds.is_none()
            && self.max_request_bytes.is_none()
            && self.max_stdout_bytes.is_none()
            && self.max_stderr_bytes.is_none()
            && self.memory_mib.is_none()
            && !self.default_memory
            && self.cpu_percent.is_none()
            && !self.default_cpu
        {
            bail!("Choose at least one Python limit; use show to inspect")
        }
        if self.memory_mib.is_some() && self.default_memory {
            bail!("Choose a memory ceiling or explicit default, not both")
        }
        if self.cpu_percent.is_some() && self.default_cpu {
            bail!("Choose a CPU ceiling or explicit default, not both")
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
        if self.default_cpu {
            limits.max_job_cpu_percent = None;
        } else if let Some(value) = self.cpu_percent {
            limits.max_job_cpu_percent = Some(value);
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
    // Omitted for legacy/default policies so existing serialized reports stay stable.
    // Platform API availability is not proof that a particular job accepted the cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cpu_rate_control_supported: Option<bool>,
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
        && (r.cpu_rate_control_supported.is_some() == cpu_relevant(&r.saved, r.applied.as_ref()))
        && (r.cpu_rate_control_supported == Some(true)
            || r.applied
                .as_ref()
                .is_none_or(|l| l.max_job_cpu_percent.is_none()))
        && (r.applied.as_ref().is_none_or(|a| a == &r.saved) || r.restart_required)
        && !r.execution_authorized
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == note(cpu_relevant(&r.saved, r.applied.as_ref()))
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
                    && (r.cpu_rate_control_supported == Some(true)
                        || r.saved.max_job_cpu_percent.is_none())
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
                    crate::process_cpu::supported(limits.max_job_cpu_percent)?;
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
                let cpu = cpu_relevant(&saved, applied.as_ref());
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
                    cpu_rate_control_supported: cpu.then_some(cfg!(windows)),
                    changed,
                    execution_authorized: false,
                    restart_requested: false,
                    automatically_retried: false,
                    note: note(cpu),
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
        let mut text = format!(
            "{}s; request {} bytes; stdout {} bytes; stderr {} bytes; job commit {}",
            l.timeout_seconds,
            l.max_request_bytes,
            l.max_stdout_bytes,
            l.max_stderr_bytes,
            l.max_job_memory_mib
                .map(|n| format!("{n} MiB"))
                .unwrap_or_else(|| "no explicit ceiling".into())
        );
        if let Some(percent) = l.max_job_cpu_percent {
            text.push_str(&format!("; owned-job CPU ceiling {percent}%"));
        }
        text
    };
    Ok(format!(
        "Node {} Python bridge\nSaved: {}\nApplied: {}\nMemory ceiling supported: {}\nRestart required: {}\n{}{}\n",
        r.node_id,
        describe(&r.saved),
        r.applied
            .as_ref()
            .map(describe)
            .unwrap_or_else(|| "Python bridge not active".into()),
        r.memory_ceiling_supported,
        r.restart_required,
        r.cpu_rate_control_supported
            .map(|supported| format!("CPU rate control platform support: {supported}\n"))
            .unwrap_or_default(),
        r.note
    ))
}

#[cfg(test)]
#[path = "python_policy_tests.rs"]
mod tests;
