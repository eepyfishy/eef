//! Explicit local-owner checks of invoked bundled runtimes, never live-node health.
//! Default inspection launches nothing. No PATH fallback, repair, model or restart.
use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::{Instant, timeout, timeout_at},
};

const CORE: &[&str] = &["json", "base64", "ctypes", "importlib.util"];
const MEDIA: &[&str] = &[
    "PIL.Image",
    "pyautogui",
    "sounddevice",
    "soundfile",
    "cv2",
    "pyttsx3",
];
const LIMIT: usize = 64 * 1024;
const EXECUTABLE_MAX: u64 = 512 * 1024 * 1024;
const PYTHON: &str = r#"import sys, json, importlib
results = []
for name in json.loads(sys.argv[1]):
    try:
        importlib.import_module(name)
        ok = True
    except BaseException:
        ok = False
    results.append({'name': name, 'imported': ok})
print(json.dumps({'schema_version': 1, 'version': list(sys.version_info[:3]), 'bits': 64 if sys.maxsize > 2**32 else 32, 'modules': results}))
"#;

#[derive(Clone, Debug, clap::Args)]
pub struct Args {
    /// Explicitly launch bundled Python/core imports and node llama.cpp --version.
    #[arg(long)]
    pub probe: bool,
    /// Also import stock node media packages; may initialize libraries, never capture/play.
    #[arg(long, requires = "probe")]
    pub media: bool,
    /// Absolute per-probe deadline, including pipe/root/descendant waits.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=30))]
    pub timeout_seconds: u64,
    /// Windows owned-job commit ceiling, not working-set RAM/VRAM or a sandbox.
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u64).range(64..=2048))]
    pub max_job_memory_mib: u64,
}
impl Default for Args {
    fn default() -> Self {
        Self {
            probe: false,
            media: false,
            timeout_seconds: 10,
            max_job_memory_mib: 512,
        }
    }
}
impl Args {
    fn validate(&self, program: &str) -> Result<()> {
        if !(1..=30).contains(&self.timeout_seconds)
            || !(64..=2048).contains(&self.max_job_memory_mib)
            || (self.media && (!self.probe || program != "eefn"))
        {
            bail!("invalid bundled doctor request")
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Context {
    executable: PathBuf,
    bundle: PathBuf,
    install: PathBuf,
    program: String,
    version: String,
}
impl Context {
    pub fn current(program: &str) -> Result<Self> {
        Self::for_executable(std::env::current_exe()?, program, crate::VERSION)
    }
    /// Trusted embedding/test context, not deserialized or exposed as a version override.
    pub fn for_executable(
        executable: impl AsRef<Path>,
        program: &str,
        version: &str,
    ) -> Result<Self> {
        if !matches!(program, "eef" | "eefn") {
            bail!("unsupported role")
        }
        crate::updater::validate_version(version)?;
        let executable = executable.as_ref().canonicalize()?;
        Ok(Self {
            bundle: executable
                .parent()
                .context("executable parent")?
                .to_path_buf(),
            install: crate::updater::installation_root(&executable)?,
            executable,
            program: program.into(),
            version: version.into(),
        })
    }
}
#[derive(Deserialize)]
struct Marker {
    name: String,
    version: String,
    architecture: String,
    python: Option<String>,
    llamacpp: Option<String>,
}
#[derive(Clone, PartialEq, Eq)]
struct Identity {
    bundle: PathBuf,
    app: String,
    marker: String,
    python: FileState,
    llama: Option<FileState>,
}
#[derive(Clone, PartialEq, Eq)]
enum FileState {
    Missing,
    Unsafe,
    File(String),
}
impl FileState {
    fn label(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Unsafe => "unsafe_path",
            Self::File(_) => "present",
        }
    }
}
fn executable_digest(path: &Path) -> Result<String> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > EXECUTABLE_MAX {
        bail!("executable observation bound")
    }
    let mut reader = file.take(EXECUTABLE_MAX + 1);
    // Keep every full-content observation/fence, but use the project's existing
    // native SHA-256 implementation. Large debug executables otherwise spend
    // seconds in each unoptimized hashing pass before/after a bounded probe.
    let mut sha = ring::digest::Context::new(&ring::digest::SHA256);
    let mut count = 0;
    let mut buffer = [0u8; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > EXECUTABLE_MAX {
            bail!("executable observation bound")
        }
        sha.update(&buffer[..n]);
    }
    Ok(hex::encode(sha.finish().as_ref()))
}
fn regular(path: &Path) -> Result<bool> {
    let m = std::fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Ok(false);
        }
    }
    Ok(!m.file_type().is_symlink())
}
fn target(bundle: &Path, relative: &str) -> Result<FileState> {
    let mut path = bundle.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match regular(&path) {
            Ok(true) => {}
            Ok(false) => return Ok(FileState::Unsafe),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(FileState::Missing);
            }
            Err(e) => return Err(e),
        }
    }
    if !path.is_file() || !path.canonicalize()?.starts_with(bundle.canonicalize()?) {
        return Ok(FileState::Unsafe);
    }
    Ok(FileState::File(executable_digest(&path)?))
}
fn snapshot(ctx: &Context) -> Result<(Identity, Marker)> {
    let path = ctx.bundle.join("bundle.json");
    if !regular(&path)? {
        bail!("unsafe bundle marker")
    }
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        bail!("bundle marker bound")
    }
    let marker: Marker = serde_json::from_slice(&bytes)?;
    if marker.version != ctx.version
        || marker.architecture != "x86_64-windows"
        || !(marker.name == ctx.program || marker.name == "EEF")
    {
        bail!("bundle role/version/architecture mismatch")
    }
    if marker
        .python
        .as_ref()
        .is_some_and(|v| python_version(v).is_none())
        || marker
            .llamacpp
            .as_ref()
            .is_some_and(|v| llama_build(v).is_none())
    {
        bail!("unsupported runtime version metadata")
    }
    Ok((
        Identity {
            bundle: ctx.bundle.clone(),
            app: executable_digest(&ctx.executable)?,
            marker: hex::encode(Sha256::digest(&bytes)),
            python: target(&ctx.bundle, "python/python.exe")?,
            llama: if ctx.program == "eefn" {
                Some(target(&ctx.bundle, "tools/llama-server.exe")?)
            } else {
                None
            },
        },
        marker,
    ))
}
fn python_version(value: &str) -> Option<[u32; 3]> {
    let value = value.strip_suffix("-embedded").unwrap_or(value);
    let parts = value
        .split('.')
        .map(|v| {
            if v.is_empty() || v.len() > 3 || !v.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                v.parse().ok()
            }
        })
        .collect::<Option<Vec<u32>>>()?;
    parts.try_into().ok()
}
fn llama_build(value: &str) -> Option<u64> {
    let v = value.strip_prefix('b')?;
    if v.is_empty() || v.len() > 10 || !v.bytes().all(|b| b.is_ascii_digit()) {
        None
    } else {
        v.parse().ok()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonReply {
    schema_version: u32,
    version: [u32; 3],
    bits: u32,
    modules: Vec<Module>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    name: String,
    imported: bool,
}
fn python_reply(bytes: &[u8], modules: &[&str], expected: Option<&str>) -> Result<Value> {
    if bytes.len() > LIMIT {
        bail!("reply bound")
    }
    let reply: PythonReply = serde_json::from_slice(bytes)?;
    if reply.schema_version != 1
        || reply.bits != 64
        || reply.version.iter().any(|v| *v > 999)
        || reply.modules.len() != modules.len()
        || reply
            .modules
            .iter()
            .zip(modules)
            .any(|(row, name)| row.name != *name)
    {
        bail!("invalid python probe reply")
    }
    let matches = expected.is_none_or(|s| python_version(s) == Some(reply.version));
    let imported = reply.modules.iter().all(|m| m.imported);
    Ok(
        json!({"passed":matches&&imported,"version":reply.version,"pointer_bits":reply.bits,"expected_version_matches":expected.map(|_|matches),
        "modules":reply.modules.iter().map(|m|json!({"name":m.name,"imported":m.imported})).collect::<Vec<_>>(),
        "failure_code":if !matches {Some("python_version_mismatch")}else if !imported {Some("python_module_import_failed")}else{None}}),
    )
}
fn llama_reply(stdout: &[u8], stderr: &[u8], expected: Option<&str>) -> Result<Value> {
    if stdout.len() > LIMIT || stderr.len() > LIMIT {
        bail!("reply bound")
    }
    let text = format!(
        "{}\n{}",
        std::str::from_utf8(stdout)?,
        std::str::from_utf8(stderr)?
    );
    let modern = regex::Regex::new(
        r"(?m)^version: ([0-9A-Za-z_.-]{1,64}) \(build ([0-9]{1,10}), commit ([0-9a-fA-F]{6,40})\)\r?$",
    )?;
    let legacy = regex::Regex::new(r"(?m)^version: ([0-9]{1,10}) \(([0-9a-fA-F]{6,40})\)\r?$")?;
    let mut found = vec![];
    for c in modern.captures_iter(&text) {
        found.push((c[2].parse::<u64>()?, c[3].to_lowercase()));
    }
    for c in legacy.captures_iter(&text) {
        found.push((c[1].parse::<u64>()?, c[2].to_lowercase()));
    }
    if found.len() != 1 {
        bail!("ambiguous llama version reply")
    }
    let (build, commit) = found.pop().unwrap();
    let matches = expected.is_none_or(|s| llama_build(s) == Some(build));
    Ok(
        json!({"passed":matches,"build":build,"commit":commit,"expected_build_matches":expected.map(|_|matches),"failure_code":if matches{None}else{Some("llama_build_mismatch")}}),
    )
}

struct Captured {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
fn selected_context(origin: &Context) -> Result<Context> {
    let version = crate::updater::installed_version(&origin.install)?;
    match crate::bundle_selection::pending_executable(
        &origin.install,
        &origin.version,
        &origin.program,
        &origin.executable,
    )? {
        Some(executable) => Context::for_executable(
            executable,
            &origin.program,
            &version.context("selected semantic version missing")?,
        ),
        None => Ok(origin.clone()),
    }
}
fn selected_snapshot(origin: &Context) -> Result<Identity> {
    let target = selected_context(origin)?;
    snapshot(&target).map(|(id, _)| id)
}
#[derive(Debug)]
enum Failure {
    #[cfg(not(windows))]
    Unsupported,
    Spawn,
    Deadline,
    Output,
    Process,
    Retirement,
}
impl Failure {
    fn code(&self) -> &'static str {
        match self {
            #[cfg(not(windows))]
            Self::Unsupported => "probe_platform_unsupported",
            Self::Spawn => "probe_spawn_failed",
            Self::Deadline => "probe_deadline",
            Self::Output => "probe_output_bound",
            Self::Process => "probe_process_failed",
            Self::Retirement => "probe_retirement_unconfirmed",
        }
    }
}
trait Runner: Sync {
    fn run(
        &self,
        path: &Path,
        args: &[String],
        bundle: &Path,
        limits: &Args,
    ) -> impl std::future::Future<Output = std::result::Result<Captured, Failure>> + Send;
}
struct RealRunner;
fn probe_command(path: &Path, args: &[String], bundle: &Path) -> Result<Command> {
    let runtime = path.parent().context("runtime directory missing")?;
    if !runtime.starts_with(bundle) {
        bail!("runtime directory outside selected bundle")
    }
    let mut command = Command::new(path);
    // Inherited PATH/CWD can mask a broken package by supplying an older DLL.
    // Keep only ordinary OS/profile/temp variables; no provider/plugin overrides,
    // PSKs, API tokens or development-toolchain search path enter the probe.
    command.env_clear();
    for key in [
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "COMSPEC",
        "PATHEXT",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("PATH", runtime)
        .args(args)
        .current_dir(runtime)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok(command)
}
async fn read(mut pipe: impl AsyncRead + Unpin) -> std::result::Result<Vec<u8>, Failure> {
    let mut out = vec![];
    (&mut pipe)
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut out)
        .await
        .map_err(|_| Failure::Output)?;
    if out.len() > LIMIT {
        return Err(Failure::Output);
    }
    Ok(out)
}
#[cfg(windows)]
fn spawn(
    command: &mut Command,
    memory: u64,
) -> Result<(tokio::process::Child, crate::process_lifetime::ProcessTree)> {
    // Local CLI only: no app service has been initialized. Keep process-wide
    // loader-error suppression synchronous, serialized and restored before await.
    // Child inherits error mode; this is not UAC/Defender/security suppression.
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetErrorMode, SEM_FAILCRITICALERRORS, SetErrorMode,
    };
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("probe error-mode guard poisoned"))?;
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                SetErrorMode(self.0);
            }
        }
    }
    // SAFETY: this process-only setting has no pointer inputs; guard restores it
    // on every return/unwind and never spans async scheduling.
    let _restore = Restore(unsafe { SetErrorMode(GetErrorMode() | SEM_FAILCRITICALERRORS) });
    crate::process_lifetime::spawn_with_job_memory_limit(command, Some(memory))
}
impl Runner for RealRunner {
    async fn run(
        &self,
        path: &Path,
        args: &[String],
        bundle: &Path,
        limits: &Args,
    ) -> std::result::Result<Captured, Failure> {
        #[cfg(not(windows))]
        {
            let _ = (path, args, bundle, limits);
            return Err(Failure::Unsupported);
        }
        #[cfg(windows)]
        {
            let deadline = Instant::now() + Duration::from_secs(limits.timeout_seconds);
            let mut cmd = probe_command(path, args, bundle).map_err(|_| Failure::Spawn)?;
            let (mut child, tree) =
                spawn(&mut cmd, limits.max_job_memory_mib).map_err(|_| Failure::Spawn)?;
            let stdout = child.stdout.take().ok_or(Failure::Spawn)?;
            let stderr = child.stderr.take().ok_or(Failure::Spawn)?;
            let outcome = timeout_at(deadline, async {
                let (stdout, stderr, status) =
                    tokio::try_join!(read(stdout), read(stderr), async {
                        let status = child.wait().await.map_err(|_| Failure::Process)?;
                        tree.terminate_and_observe_until(deadline)
                            .await
                            .map_err(|_| Failure::Retirement)?;
                        Ok(status)
                    })?;
                if !status.success() {
                    return Err(Failure::Process);
                }
                Ok(Captured { stdout, stderr })
            })
            .await
            .unwrap_or(Err(Failure::Deadline));
            drop(tree);
            if outcome.is_err() {
                let _ = child.start_kill();
                let _ = timeout(Duration::from_secs(1), child.wait()).await;
            }
            outcome
        }
    }
}
async fn execute_with(ctx: &Context, args: &Args, runner: &impl Runner) -> Result<Value> {
    args.validate(&ctx.program)?;
    let origin = ctx;
    let (target, selection_resolved) = match selected_context(origin) {
        Ok(target) => (target, true),
        Err(_) => (origin.clone(), false),
    };
    let ctx = &target;
    let (identity, marker) = snapshot(ctx)?;
    let mut probes = vec![];
    let mut ids = vec![(
        "python_core",
        "python/python.exe",
        &identity.python,
        CORE,
        marker.python.as_deref(),
    )];
    if let Some(llama) = &identity.llama {
        ids.push((
            "llama_version",
            "tools/llama-server.exe",
            llama,
            &[] as &[&str],
            marker.llamacpp.as_deref(),
        ));
    }
    if args.media {
        ids.push((
            "python_media",
            "python/python.exe",
            &identity.python,
            MEDIA,
            marker.python.as_deref(),
        ));
    }
    let mut identity_unchanged = selection_resolved;
    for (id, path, state, modules, expected) in ids {
        let mut row = json!({"id":id,"file_state":state.label(),"requested":args.probe,"expected_runtime_version":expected,"state":"not_requested","passed":null});
        if args.probe {
            if selected_snapshot(origin).map_or(true, |current| current != identity) {
                identity_unchanged = false;
            }
            if !identity_unchanged {
                row["state"] = json!("unconfirmed");
                row["failure_code"] = json!("probe_identity_changed");
            } else if !matches!(state, FileState::File(_)) {
                row["state"] = json!("failed");
                row["failure_code"] = json!(if matches!(state, FileState::Missing) {
                    "bundled_runtime_missing"
                } else {
                    "bundled_runtime_unsafe_path"
                });
            } else {
                let argv = if id == "llama_version" {
                    vec!["--version".into()]
                } else {
                    vec![
                        "-I".into(),
                        "-B".into(),
                        "-X".into(),
                        "utf8".into(),
                        "-c".into(),
                        PYTHON.into(),
                        serde_json::to_string(modules)?,
                    ]
                };
                let outcome = runner
                    .run(&ctx.bundle.join(path), &argv, &ctx.bundle, args)
                    .await;
                match outcome {
                    Ok(bytes) => {
                        let detail = if id == "llama_version" {
                            llama_reply(&bytes.stdout, &bytes.stderr, expected)
                        } else {
                            python_reply(&bytes.stdout, modules, expected)
                        };
                        match detail {
                            Ok(detail) => {
                                row["state"] = json!(if detail["passed"] == true {
                                    "passed"
                                } else {
                                    "failed"
                                });
                                row["passed"] = detail["passed"].clone();
                                row["observation"] = detail;
                            }
                            Err(_) => {
                                row["state"] = json!("failed");
                                row["failure_code"] = json!("probe_reply_invalid");
                            }
                        }
                    }
                    Err(failure) => {
                        row["state"] = json!("unconfirmed");
                        row["failure_code"] = json!(failure.code());
                    }
                }
                if selected_snapshot(origin).map_or(true, |current| current != identity) {
                    identity_unchanged = false;
                    row["state"] = json!("unconfirmed");
                    row["passed"] = Value::Null;
                    row["failure_code"] = json!("probe_identity_changed");
                }
            }
        }
        probes.push(row);
    }
    if args.probe && !identity_unchanged {
        for row in &mut probes {
            row["state"] = json!("unconfirmed");
            row["passed"] = Value::Null;
            row["failure_code"] = json!(if selection_resolved {
                "probe_identity_changed"
            } else {
                "selected_bundle_unavailable"
            });
        }
    }
    let all_passed = args
        .probe
        .then(|| identity_unchanged && probes.iter().all(|p| p["state"] == "passed"));
    let pending = crate::updater::installed_version(&origin.install)
        .and_then(|v| {
            if v.is_some_and(|v| v != origin.version) {
                return Ok(true);
            }
            crate::bundle_selection::pending_executable(
                &origin.install,
                &origin.version,
                &origin.program,
                &origin.executable,
            )
            .map(|p| p.is_some())
        })
        .ok();
    Ok(
        json!({"schema_version":1,"report_type":"package_doctor","success":all_passed!=Some(false),"inspection_completed":true,
        "scope":if !selection_resolved{"invoked_metadata_only"}else if ctx.executable!=origin.executable{"selected_package"}else{"invoked_package"},
        "selected_target_resolved":selection_resolved,"inspector_application_version":origin.version,"target_application_launch_checked":false,
        "program":ctx.program,"application_version":ctx.version,"application_sha256":identity.app,"bundle_marker_sha256":identity.marker,"selection_pending":pending,
        "probes_requested":args.probe,"media_imports_requested":args.media,"timeout_seconds_per_probe":args.timeout_seconds,"max_job_memory_mib":args.max_job_memory_mib,
        "probes":probes,"all_requested_probes_passed":all_passed,"probe_identity_unchanged":args.probe.then_some(identity_unchanged),"running_service_inspected":false,
        "model_inference_checked":false,"ollama_checked":false,"hardware_operations_requested":false,"permissions_changed":false,"models_changed":false,
        "repair_requested":false,"restart_requested":false,"automatically_retried":false,"publisher_signature_verified":false,"system_ready":null,
        "note":"Point-in-time selected-package runtime/import observations, not live service, application launch, model inference, feature permission, device health or safe restart certification. Broken selection permits invoked metadata only, never old-root runtime probing. No PATH fallback. Media imports can initialize libraries. Use repair --status for selection and explicit repair commands for broken package files."}),
    )
}
pub async fn execute(ctx: &Context, args: &Args) -> Result<Value> {
    Ok(execute_with(ctx,args,&RealRunner).await.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"package_doctor","success":false,"inspection_completed":false,
        "scope":"invoked_package","failure_code":"bundle_doctor_unavailable","probes_requested":false,"running_service_inspected":false,"model_inference_checked":false,
        "repair_requested":false,"restart_requested":false,"automatically_retried":false,"system_ready":null,
        "note":"A current managed Windows package/role/version and valid diagnostic options are required. No fallback, reset, repair or runtime launch was authorized from invalid metadata."})))
}
pub fn render(report: &Value) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Bundled runtime doctor: {}",
        if report["inspection_completed"] == true {
            "inspection completed"
        } else {
            "not confirmed"
        }
    );
    let _ = writeln!(
        out,
        "Scope: selected package runtime files when resolvable; not an inspected running node/coordinator."
    );
    if report["inspection_completed"] != true {
        let _ = writeln!(
            out,
            "Managed bundle metadata/options are unavailable or mismatched. Inspect repair --status or the installation; no fallback runtime was started."
        );
    }
    if report["selection_pending"] != false {
        let _ = writeln!(
            out,
            "Package selection differs or is unknown; inspect repair --status before assuming an update was adopted."
        );
    }
    if let Some(rows) = report["probes"].as_array() {
        for row in rows {
            let _ = writeln!(
                out,
                "{}: {} (file {}).",
                match row["id"].as_str() {
                    Some("python_core") => "Bundled Python core imports",
                    Some("python_media") => "Optional Python media imports",
                    Some("llama_version") => "Bundled llama.cpp version",
                    _ => "Bundled runtime",
                },
                match row["state"].as_str() {
                    Some("not_requested") => "not checked",
                    Some("unconfirmed") => "not confirmed",
                    Some("passed") => "passed",
                    Some("failed") => "failed",
                    _ => "unknown",
                },
                row["file_state"].as_str().unwrap_or("unknown")
            );
            if let Some(code) = row["failure_code"].as_str() {
                let _ = writeln!(out, "  Diagnostic: {code}.");
                let _ = writeln!(
                    out,
                    "  {}",
                    match code {
                        "bundled_runtime_missing" =>
                            "A bundled executable is missing. Inspect repair --status and the trusted installation source.",
                        "bundled_runtime_unsafe_path" =>
                            "The runtime path redirects outside ordinary package files; no fallback was started.",
                        "probe_spawn_failed" | "probe_process_failed" =>
                            "The bundled program could not start or finish. Dependencies, compatibility or system protection may be involved; keep Windows protections enabled.",
                        "probe_deadline" =>
                            "The probe exceeded its deadline; its result is unconfirmed and was not retried.",
                        "probe_output_bound" | "probe_reply_invalid" =>
                            "The program returned unexpected or excessive output; it was not accepted as healthy.",
                        "probe_identity_changed" =>
                            "Package/runtime identity changed while checking; these observations are not current verification.",
                        "selected_bundle_unavailable" =>
                            "The selected package is incomplete or unreadable. No old-package runtime was started; inspect repair --status.",
                        _ =>
                            "The bounded check was not confirmed. Inspect the package and retained diagnostics before trying again.",
                    }
                );
            }
            if let Some(detail) = row["observation"]["failure_code"].as_str() {
                let _ = writeln!(out, "  Diagnostic: {detail}.");
            }
            if let Some(modules) = row["observation"]["modules"].as_array() {
                for m in modules.iter().filter(|m| m["imported"] != true) {
                    let _ = writeln!(
                        out,
                        "  Import unavailable: {}.",
                        m["name"].as_str().unwrap_or("module")
                    );
                }
            }
        }
    }
    if report["inspection_completed"] == true && report["probes_requested"] != true {
        let _ = writeln!(
            out,
            "No runtime started. Add --probe for bounded bundled-runtime checks; node-only --media additionally imports optional media libraries."
        );
    }
    let _ = writeln!(
        out,
        "No model inference, hardware operation, permission change, repair or restart requested. A passing probe is not system readiness."
    );
    let _ = writeln!(
        out,
        "Ollama and owner-selected runtime overrides are not inspected by this bundled-package check."
    );
    out
}

#[cfg(test)]
#[path = "package_health_tests.rs"]
mod tests;
