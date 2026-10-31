//! Isolated bridge to the CPython runtime shipped in the Windows bundle.
//!
//! Python is an extension boundary, never a hidden implementation dependency:
//! node transport, crypto, routing, updates, and built-in capabilities are Rust.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{Instant, timeout, timeout_at};

const PLUGIN_HOST: &str = include_str!("../../../python/eef_plugin_host.py");

#[path = "python_stream.rs"]
pub mod stream;

/// Bounds the bridge exchange, plus an optional Windows owned-job commit ceiling.
/// This is not working-set RAM, VRAM, a sandbox or external-effect containment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BridgeLimits {
    pub timeout_seconds: u64,
    pub max_request_bytes: usize,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_job_memory_mib: Option<u64>,
}
impl Default for BridgeLimits {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            max_request_bytes: 36 * 1024 * 1024,
            max_stdout_bytes: 36 * 1024 * 1024,
            max_stderr_bytes: 64 * 1024,
            max_job_memory_mib: None,
        }
    }
}
impl BridgeLimits {
    pub fn validate(&self) -> Result<()> {
        crate::model_memory::validate(self.max_job_memory_mib)?;
        if !(1..=300).contains(&self.timeout_seconds)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_request_bytes)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_stdout_bytes)
            || !(1024..=1024 * 1024).contains(&self.max_stderr_bytes)
        {
            bail!("Invalid Python bridge limits")
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeFailure {
    RequestLimit,
    StdoutLimit,
    StderrLimit,
    Deadline,
    Process,
    Reply,
    Metadata,
}
impl std::fmt::Display for BridgeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RequestLimit => "Python bridge request exceeds its byte limit; no plugin started",
            Self::StdoutLimit => "Python bridge stdout exceeds its byte limit; outcome unconfirmed",
            Self::StderrLimit => "Python bridge stderr exceeds its byte limit; outcome unconfirmed",
            Self::Deadline => "Python bridge exchange exceeded its deadline; outcome unconfirmed",
            Self::Process => "Python plugin process failed; private output was not exposed",
            Self::Reply => "Python plugin returned an invalid or unsuccessful reply; private output was not exposed",
            Self::Metadata => "Python plugin returned invalid capability metadata",
        })
    }
}
impl std::error::Error for BridgeFailure {}
impl BridgeFailure {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::RequestLimit => "python_request_byte_limit",
            Self::StdoutLimit => "python_stdout_byte_limit",
            Self::StderrLimit => "python_stderr_byte_limit",
            Self::Deadline => "python_exchange_deadline",
            Self::Process => "python_process_failed",
            Self::Reply => "python_invalid_reply",
            Self::Metadata => "python_invalid_metadata",
        }
    }
}

/// Fixed categories only: never expose interpreter output, paths or OS errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityFailure {
    InvalidLimits,
    UnsupportedMemoryLimit,
    Spawn,
    Wait,
    Deadline,
    Cleanup,
    Exit,
}
impl std::fmt::Display for AvailabilityFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidLimits => "Python availability limits are invalid",
            Self::UnsupportedMemoryLimit => "Python availability memory limit is unsupported",
            Self::Spawn => "Python availability process could not start",
            Self::Wait => "Python availability process wait failed",
            Self::Deadline => "Python availability deadline exceeded",
            Self::Cleanup => "Python availability owned-process cleanup unconfirmed",
            Self::Exit => "Python availability process returned an unsuccessful exit",
        })
    }
}
impl std::error::Error for AvailabilityFailure {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum InterpreterStartup {
    // Empty struct variants enforce unknown-field rejection; Serde's tagged
    // unit variants otherwise accept extra map entries despite the annotation.
    NotRequested {},
    Checking {},
    NotConfigured {},
    InvalidLimits {},
    Available {},
    Failed { reason: AvailabilityFailure },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginStartupFailure {
    Lookup {},
    Bridge {
        reason: BridgeFailure,
    },
    /// Includes untyped file/process/registration errors; no inferred cause.
    Other {},
}
impl PluginStartupFailure {
    pub fn from_error(error: &anyhow::Error) -> Self {
        error
            .downcast_ref::<BridgeFailure>()
            .map_or(Self::Other {}, |reason| Self::Bridge { reason: *reason })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginStartupCounts {
    pub requested: u32,
    pub loaded: u32,
    pub failed: u32,
    pub last_failure: Option<PluginStartupFailure>,
}
impl PluginStartupCounts {
    pub fn record(&mut self, failure: Option<PluginStartupFailure>) {
        if let Some(failure) = failure {
            self.failed = self.failed.saturating_add(1);
            self.last_failure = Some(failure);
        } else {
            self.loaded = self.loaded.saturating_add(1);
        }
    }
}

/// Startup observations, not ongoing health, readiness or permission authority.
/// Constant-size enums/counters; no plugin names, paths, output or credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonStartup {
    pub complete: bool,
    pub interpreter: InterpreterStartup,
    pub builtin_plugins: PluginStartupCounts,
    pub custom_plugins: PluginStartupCounts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PythonPlugin {
    pub path: PathBuf,
    pub capability: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug)]
pub struct PythonRuntime {
    executable: PathBuf,
    limits: BridgeLimits,
    #[cfg(all(test, windows, feature = "native-test-fixtures"))]
    stream_test_prelude: Option<String>,
}

impl PythonRuntime {
    pub fn discover(explicit: Option<PathBuf>) -> Option<Self> {
        Self::discover_in(
            explicit,
            std::env::var_os("EEF_PYTHON").map(PathBuf::from),
            crate::runtime_layout::RuntimeLayout::current()
                .ok()
                .as_ref(),
        )
    }
    fn discover_in(
        explicit: Option<PathBuf>,
        environment: Option<PathBuf>,
        layout: Option<&crate::runtime_layout::RuntimeLayout>,
    ) -> Option<Self> {
        let executable = explicit
            .or(environment)
            .or_else(|| layout.and_then(|layout| layout.bundled_python()))
            .or_else(|| {
                layout.map(|_| {
                    PathBuf::from(if cfg!(windows) {
                        "python.exe"
                    } else {
                        "python3"
                    })
                })
            })?;
        Some(Self {
            executable,
            limits: BridgeLimits::default(),
            #[cfg(all(test, windows, feature = "native-test-fixtures"))]
            stream_test_prelude: None,
        })
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn with_limits(mut self, limits: BridgeLimits) -> Result<Self> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }
    pub fn limits(&self) -> &BridgeLimits {
        &self.limits
    }

    #[cfg(all(test, windows, feature = "native-test-fixtures"))]
    pub(crate) fn with_stream_test_prelude(mut self, prelude: String) -> Self {
        self.stream_test_prelude = Some(prelude);
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        // Import caches do not belong beside installed payload/plugin sources.
        // This is not a sandbox: trusted plugin code can still write files.
        command.arg("-I").arg("-B").arg("-X").arg("utf8");
        command
    }

    /// Probe only the configured interpreter, without importing a device plugin.
    pub async fn check_available(&self) -> Result<()> {
        self.check_availability().await?;
        Ok(())
    }

    /// Typed counterpart for bounded startup diagnostics. Budgets are unchanged.
    pub async fn check_availability(&self) -> std::result::Result<(), AvailabilityFailure> {
        self.limits
            .validate()
            .map_err(|_| AvailabilityFailure::InvalidLimits)?;
        crate::model_memory::supported(self.limits.max_job_memory_mib)
            .map_err(|_| AvailabilityFailure::UnsupportedMemoryLimit)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut command = self.command();
        command
            .arg("-c")
            .arg("pass")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let (mut child, tree) = crate::process_lifetime::spawn_with_job_memory_limit_before(
            &mut command,
            self.limits.max_job_memory_mib,
            deadline,
        )
        .map_err(|error| availability_spawn_failure(&error))?;
        let status = timeout_at(deadline, child.wait())
            .await
            .map_err(|_| AvailabilityFailure::Deadline)?
            .map_err(|_| AvailabilityFailure::Wait)?;
        #[cfg(windows)]
        tree.terminate_and_observe_until(deadline)
            .await
            .map_err(|_| AvailabilityFailure::Cleanup)?;
        drop(tree);
        if !status.success() {
            return Err(AvailabilityFailure::Exit);
        }
        Ok(())
    }

    pub async fn inspect(&self, plugin_path: impl AsRef<Path>) -> Result<PythonPlugin> {
        let path = plugin_path.as_ref().to_path_buf();
        let value = self
            .call(&path, json!({"op": "inspect"}), 64 * 1024, None)
            .await?;
        let mut plugin: PythonPlugin =
            serde_json::from_value(value).map_err(|_| BridgeFailure::Metadata)?;
        plugin.path = path;
        if plugin.capability.trim().is_empty()
            || plugin.capability.len() > 128
            || plugin.actions.len() > 128
            || plugin.actions.iter().any(|a| a.is_empty() || a.len() > 128)
        {
            return Err(BridgeFailure::Metadata.into());
        }
        if plugin.actions.is_empty() {
            plugin.actions.push("run".into());
        }
        Ok(plugin)
    }

    pub async fn invoke(
        &self,
        plugin: &PythonPlugin,
        action: &str,
        params: Value,
    ) -> Result<Value> {
        self.invoke_with_output_limit(plugin, action, params, self.limits.max_stdout_bytes, None)
            .await
    }

    /// A parent grant/pipeline may tighten the reply budget, never widen local
    /// policy. This is an encoded DATA allowance; the host envelope adds 4 KiB.
    pub(crate) async fn invoke_with_output_limit(
        &self,
        plugin: &PythonPlugin,
        action: &str,
        params: Value,
        output_limit: usize,
        execution_deadline: Option<Instant>,
    ) -> Result<Value> {
        self.invoke_with_capture_reservation(
            plugin,
            action,
            params,
            output_limit,
            execution_deadline,
            None,
        )
        .await
    }
    pub(crate) async fn invoke_with_capture_reservation(
        &self,
        plugin: &PythonPlugin,
        action: &str,
        params: Value,
        output_limit: usize,
        execution_deadline: Option<Instant>,
        reservation: Option<crate::capture_ownership::Reservation>,
    ) -> Result<Value> {
        if !plugin
            .actions
            .iter()
            .any(|candidate| candidate == "*" || candidate == action)
        {
            bail!(
                "Python plugin '{}' does not support action '{action}'",
                plugin.capability
            );
        }
        self.call_with_capture_reservation(
            &plugin.path,
            json!({"op": "invoke", "action": action, "params": params}),
            output_limit
                .saturating_add(4096)
                .min(self.limits.max_stdout_bytes),
            execution_deadline,
            reservation,
        )
        .await
    }

    async fn call(
        &self,
        plugin_path: &Path,
        request: Value,
        stdout_limit: usize,
        execution_deadline: Option<Instant>,
    ) -> Result<Value> {
        self.call_with_capture_reservation(
            plugin_path,
            request,
            stdout_limit,
            execution_deadline,
            None,
        )
        .await
    }
    async fn call_with_capture_reservation(
        &self,
        plugin_path: &Path,
        request: Value,
        stdout_limit: usize,
        execution_deadline: Option<Instant>,
        reservation: Option<crate::capture_ownership::Reservation>,
    ) -> Result<Value> {
        self.limits.validate()?;
        // Never silently drop an explicitly requested native ceiling.
        crate::model_memory::supported(self.limits.max_job_memory_mib)?;
        let stdout_limit = stdout_limit.min(self.limits.max_stdout_bytes);
        let started = Instant::now();
        let local_deadline = started + Duration::from_secs(self.limits.timeout_seconds);
        let deadline =
            crate::process_lifetime::intersect_deadline(local_deadline, execution_deadline);
        // Bounded serialization precedes spawn. No large copy first, and an
        // oversized request cannot execute an import or acquire a device.
        let input = encode_request(&request, self.limits.max_request_bytes)?;
        if Instant::now() >= deadline {
            return Err(BridgeFailure::Deadline.into());
        }
        let metadata = timeout_at(deadline, tokio::fs::metadata(plugin_path))
            .await
            .map_err(|_| BridgeFailure::Deadline)?
            .context("Python plugin metadata unavailable")?;
        if !metadata.is_file() {
            bail!("Python plugin '{}' does not exist", plugin_path.display());
        }
        let mut command = self.command();
        command
            .arg("-c")
            .arg(PLUGIN_HOST)
            .arg(plugin_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if Instant::now() >= deadline {
            return Err(BridgeFailure::Deadline.into());
        }
        let spawn_started = Instant::now();
        let (mut child, tree) = crate::process_lifetime::spawn_with_job_memory_limit_before(
            &mut command,
            self.limits.max_job_memory_mib,
            deadline,
        )
        .map_err(bridge_spawn_failure)
        .with_context(|| {
            format!(
                "could not start bundled Python at '{}'",
                self.executable.display()
            )
        })?;
        let spawned = Instant::now();
        let tree = std::sync::Arc::new(tree);
        let mut capture = reservation.map(|reservation| reservation.bind(tree.clone()));
        let stdin = child.stdin.take().context("Python stdin unavailable")?;
        let stdout = child.stdout.take().context("Python stdout unavailable")?;
        let stderr = child.stderr.take().context("Python stderr unavailable")?;
        // Readers are owned by this future, not detached tasks. They drain
        // concurrently with writing and waiting, including pipe inheritance.
        let outcome = exchange(
            stdin,
            stdout,
            stderr,
            &input,
            stdout_limit,
            self.limits.max_stderr_bytes,
            deadline,
            async {
                #[cfg(windows)]
                let status = if capture.is_some() {
                    tree.wait_root_with_coverage_until(&mut child, deadline)
                        .await
                        .map_err(bridge_wait_failure)?
                } else {
                    child.wait().await.context("Python child wait failed")?
                };
                #[cfg(not(windows))]
                let status = child.wait().await.context("Python child wait failed")?;
                #[cfg(windows)]
                {
                    if let Some(capture) = &mut capture {
                        let exit = tree.terminate_and_witness_until(deadline).await?;
                        capture.observed_exit(&exit)?;
                    } else {
                        // Ordinary bridge calls reserve no microphone and make
                        // no typed owned-source completion claim.
                        tree.terminate_and_observe_until(deadline).await?;
                    }
                }
                Ok(status)
            },
        )
        .await;
        let exchange_ended = Instant::now();
        if outcome.is_err() {
            if let Some(capture) = &mut capture {
                let _ = capture.retire().await;
            }
        }
        // On Windows closing this exact owned job requests termination of all
        // descendants. Root-only elsewhere; no sandbox/elevation/physical-stop
        // guarantee. Keep containment through every successful pipe/root wait.
        drop(capture);
        drop(tree);
        if outcome.is_err() {
            let _ = child.start_kill();
            let _ = timeout(Duration::from_secs(1), child.wait()).await;
        }
        // Opt-in numeric diagnostics only: no paths, IDs, params or child text.
        // Transport completion is not plugin success, health or effect proof.
        tracing::debug!(
            setup_ms = spawn_started.duration_since(started).as_millis(),
            spawn_ms = spawned.duration_since(spawn_started).as_millis(),
            exchange_ms = exchange_ended.duration_since(spawned).as_millis(),
            cleanup_ms = Instant::now().duration_since(exchange_ended).as_millis(),
            budget_ms = self.limits.timeout_seconds * 1000,
            effective_budget_ms = deadline.saturating_duration_since(started).as_millis(),
            outcome_code = exchange_observation_code(outcome.as_ref().err()),
            "Python exchange timing; not plugin health"
        );
        let (status, stdout) = outcome?;
        if !status.success() {
            return Err(BridgeFailure::Process.into());
        }
        decode_reply(&stdout)
    }
}

fn availability_spawn_failure(error: &anyhow::Error) -> AvailabilityFailure {
    if error.is::<crate::process_lifetime::StartDeadlineElapsed>() {
        AvailabilityFailure::Deadline
    } else {
        AvailabilityFailure::Spawn
    }
}

fn bridge_spawn_failure(error: anyhow::Error) -> anyhow::Error {
    if error.is::<crate::process_lifetime::StartDeadlineElapsed>() {
        BridgeFailure::Deadline.into()
    } else {
        error
    }
}

fn bridge_wait_failure(error: anyhow::Error) -> anyhow::Error {
    if error.is::<crate::process_lifetime::WaitDeadlineElapsed>() {
        BridgeFailure::Deadline.into()
    } else {
        error
    }
}

fn exchange_observation_code(error: Option<&anyhow::Error>) -> &'static str {
    match error {
        None => "python_exchange_ok",
        Some(error) => error
            .downcast_ref::<BridgeFailure>()
            .map(|failure| failure.code())
            .unwrap_or("python_exchange_other"),
    }
}

fn encode_request(request: &Value, limit: usize) -> Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > self.limit)
            {
                return Err(std::io::Error::other("Python request byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: vec![],
        limit,
    };
    serde_json::to_writer(&mut output, request).map_err(|_| BridgeFailure::RequestLimit)?;
    Ok(output.bytes)
}

#[allow(clippy::too_many_arguments)]
async fn exchange<W, O, E, F, T>(
    stdin: W,
    stdout: O,
    stderr: E,
    input: &[u8],
    stdout_limit: usize,
    stderr_limit: usize,
    deadline: Instant,
    wait: F,
) -> Result<(T, Vec<u8>)>
where
    W: AsyncWrite + Unpin,
    O: AsyncRead + Unpin,
    E: AsyncRead + Unpin,
    F: std::future::Future<Output = Result<T>>,
{
    timeout_at(deadline, async {
        let (_, bytes, _, status) = tokio::try_join!(
            send_input(stdin, input),
            read_pipe(stdout, stdout_limit, BridgeFailure::StdoutLimit),
            read_pipe(stderr, stderr_limit, BridgeFailure::StderrLimit),
            wait
        )?;
        Ok((status, bytes))
    })
    .await
    .map_err(|_| anyhow::Error::from(BridgeFailure::Deadline))?
}
async fn send_input<W: AsyncWrite + Unpin>(mut writer: W, bytes: &[u8]) -> Result<()> {
    writer
        .write_all(bytes)
        .await
        .context("Python request pipe failed")?;
    writer
        .shutdown()
        .await
        .context("Python request pipe close failed")?;
    Ok(())
}
async fn read_pipe<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
    failure: BridgeFailure,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut scratch = [0u8; 8192];
    loop {
        let n = reader
            .read(&mut scratch)
            .await
            .context("Python reply pipe failed")?;
        if n == 0 {
            return Ok(output);
        }
        if output.len().checked_add(n).is_none_or(|n| n > limit) {
            return Err(failure.into());
        }
        output.extend_from_slice(&scratch[..n]);
    }
}
fn decode_reply(bytes: &[u8]) -> Result<Value> {
    let mut reply: Value = serde_json::from_slice(bytes).map_err(|_| BridgeFailure::Reply)?;
    if reply["ok"] != true {
        return Err(BridgeFailure::Reply.into());
    }
    Ok(reply
        .as_object_mut()
        .ok_or(BridgeFailure::Reply)?
        .remove("data")
        .unwrap_or(Value::Null))
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
