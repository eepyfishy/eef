//! Explicit dependency health, separate from capture permission or readiness.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Maximum check duration, tightened by the applied Python policy.
    #[arg(long, default_value_t = 10, value_parser=clap::value_parser!(u64).range(1..=30))]
    pub timeout_seconds: u64,
    /// Windows owned-job commit ceiling; not working-set RAM or VRAM.
    #[arg(long, default_value_t = 128, value_parser=clap::value_parser!(u64).range(64..=4096))]
    pub max_job_memory_mib: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            timeout_seconds: 10,
            max_job_memory_mib: 128,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=30).contains(&self.timeout_seconds)
                && (64..=4096).contains(&self.max_job_memory_mib),
            "Invalid camera check bounds"
        );
        Ok(())
    }
    fn limits(&self, applied: &crate::python::BridgeLimits) -> crate::python::BridgeLimits {
        crate::python::BridgeLimits {
            timeout_seconds: self.timeout_seconds.min(applied.timeout_seconds),
            max_job_memory_mib: Some(
                self.max_job_memory_mib
                    .min(applied.max_job_memory_mib.unwrap_or(u64::MAX)),
            ),
            max_request_bytes: 1024,
            max_stdout_bytes: 1024,
            max_stderr_bytes: 1024,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub expected_runtime_id: String,
    pub request_id: String,
    pub settings: Settings,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "Unsupported camera check schema");
        crate::network::validate_remote_node_id(&self.expected_node_id)?;
        crate::automation_occurrence::uuid(&self.expected_runtime_id)?;
        crate::automation_occurrence::uuid(&self.request_id)?;
        self.settings.validate()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ready,
    DependencyUnavailable,
    ImageOperationFailed,
    RuntimeUnavailable,
    RuntimeChanged,
    Busy,
    UnsupportedPlatform,
    Deadline,
    ProbeFailed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    runtime_id: String,
    request_sha256: String,
    success: bool,
    status: Status,
    effective_limits: Option<crate::python::BridgeLimits>,
    owned_exit_observed: bool,
    capture_started: bool,
    physical_ready: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
}
impl Report {
    fn new(request: &Request) -> Result<Self> {
        Ok(Self {
            schema_version: 1,
            report_type: "node_camera_check".into(),
            node_id: request.expected_node_id.clone(),
            runtime_id: request.expected_runtime_id.clone(),
            request_sha256: crate::grants::definition_digest(&json!(request))?,
            success: false,
            status: Status::RuntimeUnavailable,
            effective_limits: None,
            owned_exit_observed: false,
            capture_started: false,
            physical_ready: false,
            execution_authorized: false,
            restart_requested: false,
            automatically_retried: false,
        })
    }
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "node_camera_check"
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && crate::grants::definition_digest(&json!(request)).is_ok_and(|d| d == r.request_sha256)
        && r.success == (r.status == Status::Ready)
        && r.owned_exit_observed
            == matches!(
                r.status,
                Status::Ready | Status::DependencyUnavailable | Status::ImageOperationFailed
            )
        && r.effective_limits.as_ref().is_none_or(|l| {
            l.validate().is_ok()
                && l.timeout_seconds <= request.settings.timeout_seconds
                && l.max_job_memory_mib
                    .is_some_and(|m| m <= request.settings.max_job_memory_mib)
                && l.max_request_bytes == 1024
                && l.max_stdout_bytes == 1024
                && l.max_stderr_bytes == 1024
        })
        && (!r.owned_exit_observed || r.effective_limits.is_some())
        && !r.capture_started
        && !r.physical_ready
        && !r.execution_authorized
        && !r.restart_requested
        && !r.automatically_retried
}

impl crate::NodeService {
    pub async fn camera_check(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Camera check targets another node"
        );
        if remote {
            ensure!(
                self.read_config()?.pointer("/management/allow_remote") == Some(&json!(true)),
                "Current local owner approval required for a remote camera dependency check"
            );
        }
        let mut report = Report::new(&request)?;
        let Some((runtime, _, mut generation)) = self.input_workflow_runtime() else {
            return Ok(json!(report));
        };
        if runtime != request.expected_runtime_id {
            report.status = Status::RuntimeChanged;
            return Ok(json!(report));
        }
        let selected = self.with_permission_runtime(&runtime, |engine| {
            engine.camera_check_runtime(&self.node_id, &self.camera_pool)
        });
        let Ok(selected) = selected else {
            return Ok(json!(report));
        };
        if !cfg!(windows) {
            report.status = Status::UnsupportedPlatform;
            return Ok(json!(report));
        }
        let limits = request.settings.limits(selected.limits());
        let checked = selected.as_ref().clone().with_limits(limits.clone())?;
        report.effective_limits = Some(limits);
        let Ok(reservation) = self.camera_pool.reserve() else {
            report.status = Status::Busy;
            return Ok(json!(report));
        };
        let outcome = crate::request_input::within_runtime(&mut generation, &runtime, async {
            // Recheck owner revocation after reserving but before any process.
            if remote
                && self
                    .read_config()
                    .ok()
                    .and_then(|c| c.pointer("/management/allow_remote").cloned())
                    != Some(json!(true))
            {
                return Err("remote_approval_required");
            }
            Ok(checked.check_camera_dependencies(reservation).await)
        })
        .await;
        report.status = match outcome {
            Err("runtime_changed") => Status::RuntimeChanged,
            Err(_) => Status::ProbeFailed,
            Ok(Err(error))
                if error.downcast_ref::<crate::python::BridgeFailure>()
                    == Some(&crate::python::BridgeFailure::Deadline) =>
            {
                Status::Deadline
            }
            Ok(Err(_)) => Status::ProbeFailed,
            Ok(Ok(value)) => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Probe {
                    status: Status,
                }
                match serde_json::from_value::<Probe>(value).map(|v| v.status) {
                    Ok(
                        status @ (Status::Ready
                        | Status::DependencyUnavailable
                        | Status::ImageOperationFailed),
                    ) => {
                        report.owned_exit_observed = true;
                        status
                    }
                    _ => Status::ProbeFailed,
                }
            }
        };
        report.success = report.status == Status::Ready;
        let value = json!(report);
        ensure!(
            confirmed(&value, &request),
            "Camera check outcome unconfirmed"
        );
        Ok(value)
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    let status = match r.status {
        Status::Ready => "Dependencies passed the in-memory image check.",
        Status::DependencyUnavailable => {
            "OpenCV or NumPy could not be imported within the selected process limits."
        }
        Status::ImageOperationFailed => "The in-memory OpenCV/NumPy image check failed.",
        Status::RuntimeUnavailable => {
            "The running node has no selected Python camera runtime. Inspect python show and camera show."
        }
        Status::RuntimeChanged => {
            "The node runtime changed during the check. Inspect its current status before deciding what to do."
        }
        Status::Busy => {
            "A camera operation is active or its process exit is unconfirmed. Wait for it to retire."
        }
        Status::UnsupportedPlatform => {
            "This bounded dependency check currently requires Windows owned-process support."
        }
        Status::Deadline => "The dependency check exceeded its time limit.",
        Status::ProbeFailed => {
            "The dependency check failed or its process exit was not confirmed. Inspect the selected Python environment and limits."
        }
    };
    Ok(format!(
        "{status}\nNo camera or endpoint was opened. This does not prove physical camera readiness or authorize capture.\n"
    ))
}

#[cfg(test)]
#[path = "camera_check_tests.rs"]
mod tests;
