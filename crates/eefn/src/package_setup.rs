//! Opt-in, one-attempt-per-owner-policy repair of missing managed package files.
//! Presence is not dependency health. No model, restart, upgrade or retry authority.
use anyhow::{Context as _, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const POLICY_MAX: usize = 64 * 1024;
const STATE_MAX: usize = 256 * 1024;
const RECORD_MAX: usize = 128;

#[path = "package_setup_policy.rs"]
pub mod policy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Off,
    Prompt,
    Auto,
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    /// Read saved policy and historical attempts; never start or repair a service.
    Show,
    /// Explicitly configure missing-bundled-file maintenance for this exact build.
    Configure {
        #[arg(long, value_enum)]
        mode: Mode,
        /// Explicit consent for one automatic repair attempt per saved revision.
        #[arg(long)]
        allow_automatic_repair: bool,
        /// Optional public HTTPS descriptor; no credentials, query or fragment.
        #[arg(long)]
        manifest: Option<String>,
        #[arg(long)]
        expected_revision: Option<u64>,
    },
    /// Run one evaluation of the saved policy without requiring a running daemon.
    Run,
    /// Approve one exact retained prompt attempt, not a new source or retry.
    Approve {
        #[arg(long)]
        operation_id: String,
    },
}

/// Trusted embedding context. App entrypoints always use current(); this is not
/// deserialized from owner/peer input. The explicit constructor supports fixtures.
#[derive(Clone)]
pub struct Context {
    install: PathBuf,
    executable: PathBuf,
    program: String,
    version: String,
}
impl Context {
    pub fn current(program: &str) -> Result<Self> {
        Self::for_executable(std::env::current_exe()?, program, crate::VERSION)
    }
    pub fn for_executable(
        executable: impl AsRef<Path>,
        program: &str,
        version: &str,
    ) -> Result<Self> {
        if !matches!(program, "eef" | "eefn") {
            bail!("unsupported package role")
        }
        crate::updater::validate_version(version)?;
        let executable = executable.as_ref().canonicalize()?;
        Ok(Self {
            install: crate::updater::installation_root(&executable)?,
            executable,
            program: program.into(),
            version: version.into(),
        })
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.install
            .join(format!(".package-repair-{}-{suffix}", self.program))
    }
    fn policy(&self) -> PathBuf {
        self.path("policy.json")
    }
    fn policy_lock(&self) -> PathBuf {
        self.path("policy.lock")
    }
    fn state(&self) -> PathBuf {
        self.path("setup.json")
    }
    fn lease(&self) -> PathBuf {
        self.path("setup.lock")
    }
    fn leases(&self) -> PathBuf {
        self.path("setup-leases")
    }
    fn operation_lease(&self, id: &str) -> PathBuf {
        self.leases().join(format!("{id}.lock"))
    }
    fn bundle(&self) -> &Path {
        self.executable.parent().expect("validated executable")
    }
    fn build(&self) -> Result<Build> {
        let slot =
            crate::bundle_selection::running_slot(&self.install, &self.version, &self.executable)?;
        if crate::updater::installed_version(&self.install)?.is_some_and(|v| v != self.version)
            || crate::bundle_selection::pending_executable(
                &self.install,
                &self.version,
                &self.program,
                &self.executable,
            )?
            .is_some()
        {
            bail!("package selection is pending")
        }
        Ok(Build {
            version: self.version.clone(),
            slot,
            executable_sha256: crate::updater::sha256_file(&self.executable)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Build {
    version: String,
    slot: String,
    executable_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    build: Build,
    public_source: String,
    components: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema_version: u32,
    revision: u64,
    policy_id: Option<String>,
    program: String,
    mode: Mode,
    binding: Option<Binding>,
}
impl Policy {
    fn off(ctx: &Context) -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            policy_id: None,
            program: ctx.program.clone(),
            mode: Mode::Off,
            binding: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Checking,
    WaitingApproval,
    Approving,
    Selected,
    Failed,
    Interrupted,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Checking | Self::Approving)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: String,
    policy_id: String,
    revision: u64,
    /// Immutable admission intent, retained even after the current policy changes.
    grant: Policy,
    phase: Phase,
    plan_id: Option<String>,
    missing: Vec<String>,
    owner_approved: bool,
    outcome_unknown: bool,
    created_at_ms: u64,
    updated_at_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    program: String,
    records: Vec<Record>,
}

fn raw(path: &Path, max: usize) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = vec![];
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        bail!("bounded package setup data exceeded")
    };
    Ok(Some(bytes))
}
fn write_checked(path: &Path, before: Option<&[u8]>, bytes: &[u8], max: usize) -> Result<()> {
    if bytes.len() > max || raw(path, max)?.as_deref() != before {
        bail!("package setup data changed or exceeded bounds")
    }
    if before.is_some() {
        crate::atomic_file::write(path, bytes)?;
    } else {
        let mut pending = tempfile::Builder::new()
            .prefix(".package-setup-")
            .tempfile_in(path.parent().context("missing parent")?)?;
        pending.write_all(bytes)?;
        pending.as_file().sync_all()?;
        pending.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}
fn digest_valid(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())
}
fn component_valid(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.chars().any(char::is_control))
}
fn public_source(url: &str) -> Result<()> {
    crate::updater::validate_https_artifact(url)?;
    if reqwest::Url::parse(url)?.query().is_some() {
        bail!("automatic policy requires a public source without a query")
    };
    Ok(())
}
fn load_policy(ctx: &Context) -> Result<(Policy, Option<Vec<u8>>)> {
    let bytes = raw(&ctx.policy(), POLICY_MAX)?;
    let policy: Policy = match &bytes {
        Some(b) => serde_json::from_slice(b)?,
        None => return Ok((Policy::off(ctx), None)),
    };
    validate_policy(ctx, &policy)?;
    Ok((policy, bytes))
}
fn validate_policy(ctx: &Context, policy: &Policy) -> Result<()> {
    validate_policy_role(&ctx.program, policy)
}
fn validate_policy_role(program: &str, policy: &Policy) -> Result<()> {
    if policy.schema_version != 1 || policy.program != program || policy.revision == 0 {
        bail!("unsupported package repair policy")
    }
    crate::package_repair::validate_operation_id(
        policy
            .policy_id
            .as_deref()
            .context("policy identity missing")?,
    )?;
    match (&policy.binding, policy.mode) {
        (None, Mode::Off) => {}
        (Some(binding), Mode::Prompt | Mode::Auto) => {
            crate::updater::validate_version(&binding.build.version)?;
            crate::bundle_selection::validate_slot(&binding.build.slot, &binding.build.version)?;
            public_source(&binding.public_source)?;
            let unique: BTreeSet<_> = binding.components.iter().collect();
            if !digest_valid(&binding.build.executable_sha256)
                || binding.components.is_empty()
                || binding.components.len() > 128
                || unique.len() != binding.components.len()
                || binding.components.iter().any(|p| !component_valid(p))
            {
                bail!("invalid repair binding")
            }
        }
        _ => bail!("invalid repair mode/binding"),
    }
    Ok(())
}
fn load_state(ctx: &Context) -> Result<(State, Option<Vec<u8>>)> {
    let bytes = raw(&ctx.state(), STATE_MAX)?;
    let state: State = match &bytes {
        Some(b) => serde_json::from_slice(b)?,
        None => {
            if ctx.lease().try_exists()? || ctx.leases().try_exists()? {
                bail!("missing repair setup journal with retained lease evidence")
            }
            return Ok((
                State {
                    schema_version: 1,
                    program: ctx.program.clone(),
                    records: vec![],
                },
                None,
            ));
        }
    };
    if state.schema_version != 1 || state.program != ctx.program || state.records.len() > RECORD_MAX
    {
        bail!("unsupported repair setup state")
    }
    let mut ids = BTreeSet::new();
    let mut policies = BTreeSet::new();
    for record in &state.records {
        validate_policy(ctx, &record.grant)?;
        crate::package_repair::validate_operation_id(&record.id)?;
        crate::package_repair::validate_operation_id(&record.policy_id)?;
        if let Some(id) = &record.plan_id {
            crate::package_repair::validate_operation_id(id)?
        }
        if !ids.insert(&record.id)
            || !policies.insert(&record.policy_id)
            || record.revision == 0
            || record.grant.mode == Mode::Off
            || record.grant.revision != record.revision
            || record.grant.policy_id.as_deref() != Some(record.policy_id.as_str())
            || record.missing.is_empty()
            || record.missing.len() > 128
            || record.missing.iter().any(|p| !component_valid(p))
            || record.missing.iter().any(|p| {
                !record
                    .grant
                    .binding
                    .as_ref()
                    .is_some_and(|b| b.components.contains(p))
            })
            || record.updated_at_ms < record.created_at_ms
            || (matches!(
                record.phase,
                Phase::WaitingApproval | Phase::Approving | Phase::Selected
            ) && record.plan_id.is_none())
        {
            bail!("invalid repair setup record")
        }
    }
    Ok((state, bytes))
}
fn lease_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?)
}
fn runner_lease(ctx: &Context) -> Result<File> {
    let (state, before) = load_state(ctx)?;
    if before.is_none() {
        write_checked(&ctx.state(), None, &serde_json::to_vec(&state)?, STATE_MAX)?
    }
    let lease = lease_file(&ctx.lease())?;
    lease
        .try_lock_exclusive()
        .context("package setup already active")?;
    Ok(lease)
}
fn operation_lease(ctx: &Context, id: &str, new: bool) -> Result<File> {
    crate::package_repair::validate_operation_id(id)?;
    if new {
        std::fs::create_dir_all(ctx.leases())?;
        if lease_ids(ctx)?.len() >= RECORD_MAX {
            bail!("repair setup lease capacity reached")
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(new)
        .open(ctx.operation_lease(id))?;
    file.try_lock_exclusive()
        .context("repair setup operation already active")?;
    Ok(file)
}
fn lease_ids(ctx: &Context) -> Result<BTreeSet<String>> {
    let entries = match std::fs::read_dir(ctx.leases()) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(e.into()),
    };
    let mut ids = BTreeSet::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            bail!("unexpected repair setup lease data")
        }
        let name = entry.file_name();
        let id = name
            .to_str()
            .and_then(|n| n.strip_suffix(".lock"))
            .context("invalid lease name")?;
        crate::package_repair::validate_operation_id(id)?;
        ids.insert(id.to_owned());
        if ids.len() > RECORD_MAX {
            bail!("repair setup lease bound exceeded")
        }
    }
    Ok(ids)
}
fn lease_observation(ctx: &Context, id: &str) -> &'static str {
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(ctx.operation_lease(id))
    {
        Ok(f) => f,
        Err(_) => return "unavailable",
    };
    match file.try_lock_exclusive() {
        Ok(()) => "released",
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            "active"
        }
        Err(_) => "unavailable",
    }
}
fn missing(ctx: &Context, paths: &[String]) -> Result<Vec<String>> {
    let mut result = vec![];
    for path in paths {
        match std::fs::symlink_metadata(ctx.bundle().join(path)) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
            Ok(_) => result.push(path.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => result.push(path.clone()),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(result)
}
fn components(ctx: &Context) -> Result<Vec<String>> {
    if crate::runtime_layout::RuntimeLayout::from_executable(&ctx.executable)?
        .repair_source_hint(&ctx.program)?
        .is_none()
    {
        bail!("managed bundle required")
    }
    let marker =
        raw(&ctx.bundle().join("bundle.json"), 16 * 1024)?.context("bundle marker missing")?;
    let marker: Value = serde_json::from_slice(&marker)?;
    if marker["name"] != ctx.program || marker["version"] != ctx.version {
        bail!("bundle product/version mismatch")
    }
    let mut paths: BTreeSet<_> = crate::package_repair::required_bundle_files(&ctx.program)?
        .into_iter()
        .collect();
    paths.insert("files.sha256.json".into());
    if let Some(bytes) = raw(&ctx.bundle().join("files.sha256.json"), 4 * 1024 * 1024)? {
        let inventory: Value = serde_json::from_slice(&bytes)?;
        let rows = inventory.as_array().context("invalid package inventory")?;
        if rows.len() > 20_000 {
            bail!("excessive inventory")
        }
        for row in rows {
            let path = row["path"].as_str().context("invalid inventory path")?;
            if !component_valid(path) {
                bail!("unsafe inventory path")
            }
            let parts: Vec<_> = path.split('/').collect();
            let name = parts.last().unwrap().to_ascii_lowercase();
            if (parts.len() == 1 && name.ends_with(".dll"))
                || (parts.len() == 2
                    && matches!(parts[0], "python" | "tools")
                    && [".exe", ".dll", ".pyd", ".zip", "._pth", ".py"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix)))
            {
                paths.insert(path.into());
            }
        }
    }
    if paths.len() > 128 {
        bail!("too many top-level package components")
    };
    Ok(paths.into_iter().collect())
}

fn configure(
    ctx: &Context,
    mode: Mode,
    allow: bool,
    source: Option<&str>,
    expected: Option<u64>,
) -> Result<Value> {
    configure_checked(ctx, mode, allow, source, expected, None)
}

fn configure_checked(
    ctx: &Context,
    mode: Mode,
    allow: bool,
    source: Option<&str>,
    expected: Option<u64>,
    expected_digest: Option<&str>,
) -> Result<Value> {
    if (mode == Mode::Auto) != allow {
        bail!("auto mode requires its distinct explicit consent flag")
    }
    if mode == Mode::Auto && ctx.version.contains('-') {
        bail!("automatic prerelease repairs are blocked")
    }
    if mode == Mode::Off && source.is_some() {
        bail!("off does not accept a source")
    }
    let lock = lease_file(&ctx.policy_lock())?;
    lock.try_lock_exclusive()
        .context("policy publication in progress; no save confirmed")?;
    let (old, before) = load_policy(ctx)?;
    if expected.is_some_and(|r| r != old.revision) {
        bail!("stale repair policy revision")
    }
    if expected_digest
        .is_some_and(|digest| policy::version(&old).map_or(true, |v| v.digest != digest))
    {
        bail!("stale repair policy digest")
    }
    let binding = if mode == Mode::Off {
        None
    } else {
        let source = match source {
            Some(s) => s.to_owned(),
            None => crate::repair_sources::hint_status(&ctx.executable, &ctx.program, &ctx.version)
                ["publisher_source"]
                .as_str()
                .context("public source unavailable; choose --manifest")?
                .to_owned(),
        };
        public_source(&source)?;
        Some(Binding {
            build: ctx.build()?,
            public_source: source,
            components: components(ctx)?,
        })
    };
    let policy = Policy {
        schema_version: 1,
        revision: old
            .revision
            .checked_add(1)
            .context("policy revision exhausted")?,
        policy_id: Some(uuid::Uuid::new_v4().to_string()),
        program: ctx.program.clone(),
        mode,
        binding,
    };
    write_checked(
        &ctx.policy(),
        before.as_deref(),
        &serde_json::to_vec(&policy)?,
        POLICY_MAX,
    )?;
    Ok(
        json!({"schema_version":1,"report_type":"package_setup","success":true,"action":"configure","policy":policy,
        "saved":true,"running_service_inspected":false,"restart_requested":false,"runtime_health_checked":false,
        "note":"Saved policy, not observed background adoption or runtime health. Auto is one attempt per explicit policy identity; no alpha, retry, restart or model authority."}),
    )
}

fn update_record(
    ctx: &Context,
    record: &mut Record,
    phase: Phase,
    plan_id: Option<String>,
) -> Result<()> {
    update_record_result(ctx, record, phase, plan_id, phase == Phase::Interrupted)
}

fn update_record_result(
    ctx: &Context,
    record: &mut Record,
    phase: Phase,
    plan_id: Option<String>,
    outcome_unknown: bool,
) -> Result<()> {
    let (mut state, before) = load_state(ctx)?;
    let index = state
        .records
        .iter()
        .position(|r| r.id == record.id)
        .context("repair setup record disappeared")?;
    if state.records[index] != *record || !record.phase.active() {
        bail!("repair setup record changed or terminal")
    }
    let allowed = matches!(
        (record.phase, phase),
        (
            Phase::Checking,
            Phase::WaitingApproval | Phase::Approving | Phase::Failed | Phase::Interrupted
        ) | (
            Phase::Approving,
            Phase::Selected | Phase::Failed | Phase::Interrupted
        )
    );
    if !allowed {
        bail!("invalid package setup transition")
    }
    let mut next = record.clone();
    next.phase = phase;
    next.outcome_unknown = outcome_unknown;
    next.updated_at_ms = crate::protocol::now_ms().max(next.updated_at_ms);
    if let Some(id) = plan_id {
        crate::package_repair::validate_operation_id(&id)?;
        if next.plan_id.as_ref().is_some_and(|v| v != &id) {
            bail!("repair proposal changed")
        };
        next.plan_id = Some(id)
    }
    state.records[index] = next.clone();
    write_checked(
        &ctx.state(),
        before.as_deref(),
        &serde_json::to_vec(&state)?,
        STATE_MAX,
    )?;
    *record = next;
    Ok(())
}

#[derive(Clone)]
struct Control {
    alive: Arc<AtomicBool>,
    stopped: Option<tokio::sync::watch::Receiver<bool>>,
}
impl Control {
    fn local() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            stopped: None,
        }
    }
    fn active(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && self
                .stopped
                .as_ref()
                .is_none_or(|s| !*s.borrow() && s.has_changed().is_ok())
    }
}
struct Fence {
    ctx: Context,
    policy: Policy,
    record: Record,
    artifact: bool,
    control: Control,
}
impl Fence {
    fn policy_guard(&self) -> Result<File> {
        if !self.control.active() {
            bail!("package setup owner is retiring")
        }
        let lock = lease_file(&self.ctx.policy_lock())?;
        FileExt::try_lock_shared(&lock)?;
        let (current, _) = load_policy(&self.ctx)?;
        if current != self.policy || self.ctx.version.contains('-') {
            bail!("repair setup policy or process lifetime changed")
        }
        Ok(lock)
    }
    fn authorize(&self) -> Result<File> {
        let lock = self.policy_guard()?;
        let binding = self
            .policy
            .binding
            .as_ref()
            .context("repair binding missing")?;
        let state = load_state(&self.ctx)?.0;
        if binding.build != self.ctx.build()?
            || self.record.grant != self.policy
            || !state.records.iter().any(|r| r == &self.record)
            || !self.record.phase.active()
            || (self.artifact && self.policy.mode != Mode::Auto && !self.record.owner_approved)
            || missing(&self.ctx, &self.record.missing)?.is_empty()
        {
            bail!("repair setup authority or missing-file cause changed")
        }
        Ok(lock)
    }
}

trait SourceExecutor: Sync {
    fn execute(
        &self,
        ctx: &Context,
        args: &crate::package_repair::RepairArgs,
        fence: &Fence,
    ) -> impl std::future::Future<Output = Result<Value>> + Send;
}
struct RealSource;
impl SourceExecutor for RealSource {
    async fn execute(
        &self,
        ctx: &Context,
        args: &crate::package_repair::RepairArgs,
        fence: &Fence,
    ) -> Result<Value> {
        crate::repair_sources::execute_for(
            args,
            &ctx.install,
            &ctx.executable,
            &ctx.program,
            &ctx.version,
            || fence.authorize(),
        )
        .await
    }
}
async fn guarded<T: std::future::Future<Output = Result<Value>>>(
    future: T,
    fence: &Fence,
) -> Result<Value> {
    tokio::pin!(future);
    loop {
        tokio::select! {result=&mut future=>return result,_=tokio::time::sleep(Duration::from_millis(500))=>{drop(fence.policy_guard()?);}}
    }
}
fn source_args(policy: &Policy, approve: Option<String>) -> crate::package_repair::RepairArgs {
    crate::package_repair::RepairArgs {
        url: None,
        sha256: None,
        size_bytes: None,
        allow_prerelease: false,
        rollback: false,
        status: false,
        operation_id: None,
        manifest: Some(
            policy
                .binding
                .as_ref()
                .expect("checked binding")
                .public_source
                .clone(),
        ),
        check: approve.is_none(),
        approve,
    }
}

async fn drive<E: SourceExecutor>(
    ctx: &Context,
    approval: Option<&str>,
    control: Control,
    executor: &E,
) -> Result<Value> {
    if !control.active() {
        bail!("package setup owner is retiring")
    }
    let (policy, _) = load_policy(ctx)?;
    if policy.mode == Mode::Off {
        return Ok(show(ctx));
    }
    if ctx.version.contains('-') {
        bail!("automatic prerelease repair is blocked; manual source commands remain available")
    }
    let binding = policy.binding.as_ref().context("binding missing")?;
    if ctx.build()? != binding.build {
        bail!("repair policy needs explicit reauthorization for this build/slot")
    }
    let lease = runner_lease(ctx)?;
    let (mut state, before) = load_state(ctx)?;
    let _operation;
    let mut record = if let Some(id) = approval {
        crate::package_repair::validate_operation_id(id)?;
        let index = state
            .records
            .iter()
            .position(|r| r.id == id)
            .context("approval attempt missing")?;
        let mut record = state.records[index].clone();
        if record.policy_id != policy.policy_id.as_deref().unwrap_or("")
            || record.grant != policy
            || record.phase != Phase::WaitingApproval
            || record.plan_id.is_none()
        {
            bail!("attempt is not a fresh pending approval")
        }
        _operation = operation_lease(ctx, &record.id, false)?;
        record.phase = Phase::Approving;
        record.owner_approved = true;
        record.updated_at_ms = crate::protocol::now_ms().max(record.updated_at_ms);
        state.records[index] = record.clone();
        write_checked(
            &ctx.state(),
            before.as_deref(),
            &serde_json::to_vec(&state)?,
            STATE_MAX,
        )?;
        record
    } else {
        if state
            .records
            .iter()
            .any(|r| Some(&r.policy_id) == policy.policy_id.as_ref())
        {
            drop(lease);
            return Ok(show(ctx));
        }
        let absent = missing(ctx, &binding.components)?;
        if absent.is_empty() {
            drop(lease);
            return Ok(show(ctx));
        }
        if state.records.len() >= RECORD_MAX {
            bail!("repair setup history is full; no pruning or retry")
        }
        let now = crate::protocol::now_ms();
        let record = Record {
            id: uuid::Uuid::new_v4().to_string(),
            policy_id: policy
                .policy_id
                .clone()
                .context("policy identity missing")?,
            revision: policy.revision,
            grant: policy.clone(),
            phase: Phase::Checking,
            plan_id: None,
            missing: absent,
            owner_approved: false,
            outcome_unknown: false,
            created_at_ms: now,
            updated_at_ms: now,
        };
        state.records.push(record.clone());
        if serde_json::to_vec(&state)?.len() > STATE_MAX {
            bail!("repair setup byte capacity reached")
        }
        _operation = operation_lease(ctx, &record.id, true)?;
        write_checked(
            &ctx.state(),
            before.as_deref(),
            &serde_json::to_vec(&state)?,
            STATE_MAX,
        )?;
        record
    };
    if record.phase == Phase::Checking {
        let fence = Fence {
            ctx: ctx.clone(),
            policy: policy.clone(),
            record: record.clone(),
            artifact: false,
            control: control.clone(),
        };
        let args = source_args(&policy, None);
        let checked = guarded(executor.execute(ctx, &args, &fence), &fence).await;
        match checked {
            Ok(report) if report["success"] == true => {
                let plan = report["plan"]["id"]
                    .as_str()
                    .context("source proposal not confirmed")?
                    .to_owned();
                update_record(
                    ctx,
                    &mut record,
                    if policy.mode == Mode::Prompt {
                        Phase::WaitingApproval
                    } else {
                        Phase::Approving
                    },
                    Some(plan),
                )?;
                if record.phase == Phase::WaitingApproval {
                    drop(_operation);
                    drop(lease);
                    return Ok(show(ctx));
                }
            }
            Ok(_) => {
                update_record(ctx, &mut record, Phase::Failed, None)?;
                drop(_operation);
                drop(lease);
                return Ok(show(ctx));
            }
            Err(_) => {
                let _ = update_record(ctx, &mut record, Phase::Interrupted, None);
                drop(_operation);
                drop(lease);
                return Ok(show(ctx));
            }
        }
    }
    let fence = Fence {
        ctx: ctx.clone(),
        policy: policy.clone(),
        record: record.clone(),
        artifact: true,
        control,
    };
    let args = source_args(&policy, record.plan_id.clone());
    let applied = guarded(executor.execute(ctx, &args, &fence), &fence).await;
    match applied {
        Ok(report) => update_record_result(
            ctx,
            &mut record,
            if report["success"] == true {
                Phase::Selected
            } else {
                Phase::Failed
            },
            None,
            report["success"] != true && report["outcome_unknown"].as_bool().unwrap_or(true),
        )?,
        Err(_) => {
            let _ = update_record(ctx, &mut record, Phase::Interrupted, None);
        }
    }
    drop(_operation);
    drop(lease);
    Ok(show(ctx))
}

fn show(ctx: &Context) -> Value {
    let policy = load_policy(ctx).ok().map(|p| p.0);
    let state = load_state(ctx).ok().map(|s| s.0);
    let leases = lease_ids(ctx).ok();
    let orphans = leases.as_ref().zip(state.as_ref()).map(|(ids, state)| {
        ids.iter()
            .filter(|id| !state.records.iter().any(|r| &r.id == *id))
            .count()
    });
    let records = state.as_ref().map(|s| {
        s.records
            .iter()
            .map(|r| {
                let lease = lease_observation(ctx, &r.id);
                json!({"record":r,"lease":lease,
        "outcome_unresolved":r.outcome_unknown||(r.phase.active()&&lease!="active"),"automatically_retried":false})
            })
            .collect::<Vec<_>>()
    });
    let absent = policy
        .as_ref()
        .and_then(|p| p.binding.as_ref())
        .and_then(|b| missing(ctx, &b.components).ok());
    let applicability = match policy.as_ref() {
        None => "policy_unavailable",
        Some(p) if p.mode == Mode::Off => "off",
        Some(_) if ctx.version.contains('-') => "prerelease_blocked",
        Some(p) => match ctx.build() {
            Ok(build) if p.binding.as_ref().is_some_and(|b| b.build == build) => "current_build",
            Ok(_) => "reauthorization_required",
            Err(_) => "invoked_build_unavailable",
        },
    };
    json!({"schema_version":1,"report_type":"package_setup","success":policy.is_some()&&state.is_some()&&leases.is_some(),"action":"show","scope":"invoked_executable",
        "policy":policy,"policy_applicability":applicability,"state_available":state.is_some(),"record_limit":RECORD_MAX,"state_byte_limit":STATE_MAX,"records":records,"missing_components":absent,
        "lease_store_available":leases.is_some(),"lease_slots":leases.as_ref().map(BTreeSet::len),"orphan_lease_count":orphans,
        "prerelease_automation_blocked":ctx.version.contains('-'),"running_service_inspected":false,"runtime_health_checked":false,
        "publisher_signature_verified":false,"restart_requested":false,"automatically_retried":false,
        "note":"Missing-file presence and historical attempts only, not dependency health. One attempt per owner policy identity; interrupted attempts never resume. Inspect repair --status for exact source admission/selection evidence."})
}

pub async fn execute(ctx: &Context, action: &Action) -> Result<Value> {
    let result = match action {
        Action::Show => Ok(show(ctx)),
        Action::Configure {
            mode,
            allow_automatic_repair,
            manifest,
            expected_revision,
        } => configure(
            ctx,
            *mode,
            *allow_automatic_repair,
            manifest.as_deref(),
            *expected_revision,
        ),
        Action::Run => drive(ctx, None, Control::local(), &RealSource).await,
        Action::Approve { operation_id } => {
            drive(ctx, Some(operation_id), Control::local(), &RealSource).await
        }
    };
    Ok(result.unwrap_or_else(|_|json!({"schema_version":1,"report_type":"package_setup","success":false,"error_code":"package_setup_not_confirmed",
        "scope":"invoked_executable","restart_requested":false,"runtime_health_checked":false,"automatically_retried":false,
        "note":"Inspect repair-setup show and repair --status. Auto needs distinct consent and a stable exact managed build/public source. No unsupported history reset, source fallback or execution retry."})))
}

pub fn render(report: &Value) -> String {
    let mut text = String::new();
    use std::fmt::Write;
    let _ = writeln!(
        text,
        "Package repair setup: {}",
        if report["success"] == true {
            "inspection/save completed"
        } else {
            "not confirmed; inspect policy, build and retained evidence"
        }
    );
    if let Some(policy) = report["policy"].as_object() {
        let _ = writeln!(
            text,
            "Saved mode: {}. Revision: {}.",
            report["policy"]["mode"].as_str().unwrap_or("unavailable"),
            policy.get("revision").unwrap_or(&Value::Null)
        );
        if let Some(source) = report["policy"]["binding"]["public_source"].as_str() {
            let _ = writeln!(text, "Owner-pinned public publisher: {source}");
        }
    }
    if let Some(eligibility) = report["policy_applicability"].as_str() {
        let _ = writeln!(text, "Policy applicability: {eligibility}.");
    }
    if let Some(files) = report["missing_components"].as_array() {
        let _ = writeln!(
            text,
            "Missing required managed files: {} (presence only, not dependency health).",
            files.len()
        );
    }
    if report["prerelease_automation_blocked"] == true {
        let _ = writeln!(
            text,
            "Prerelease automation blocked. Use explicit manual repair commands with one-shot consent."
        );
    }
    if let Some(records) = report["records"].as_array() {
        for row in records {
            let record = &row["record"];
            let _ = writeln!(
                text,
                "Attempt {}: {}{}",
                record["id"].as_str().unwrap_or("unknown"),
                record["phase"],
                if row["outcome_unresolved"] == true {
                    " (interrupted/unknown; never resumed)"
                } else {
                    ""
                }
            );
            if record["phase"] == "waiting_approval" {
                let _ = writeln!(
                    text,
                    "Approve this exact attempt: repair-setup approve --operation-id {}",
                    record["id"].as_str().unwrap_or("")
                );
            }
        }
    }
    let _ = writeln!(
        text,
        "Files present/selected is not runtime health. No automatic restart, retry, model setup or prerelease update."
    );
    text
}

pub struct Supervisor {
    alive: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.task.abort();
    }
}
pub fn spawn_until(
    program: &str,
    mut stopped: tokio::sync::watch::Receiver<bool>,
) -> Option<Supervisor> {
    let ctx = Context::current(program).ok()?;
    let alive = Arc::new(AtomicBool::new(true));
    let running = Control {
        alive: alive.clone(),
        stopped: Some(stopped.clone()),
    };
    let task = tokio::spawn(async move {
        loop {
            if !running.active() {
                break;
            }
            let tick = async {
                if let Ok((policy, _)) = load_policy(&ctx) {
                    if policy.mode != Mode::Off
                        && !ctx.version.contains('-')
                        && load_state(&ctx).is_ok_and(|(state, _)| {
                            !state
                                .records
                                .iter()
                                .any(|r| Some(&r.policy_id) == policy.policy_id.as_ref())
                        })
                        && policy.binding.as_ref().is_some_and(|b| {
                            missing(&ctx, &b.components).is_ok_and(|m| !m.is_empty())
                        })
                    {
                        let _ = drive(&ctx, None, running.clone(), &RealSource).await;
                    }
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            };
            tokio::select! {_=Box::pin(tick)=>{},_=stopped.changed()=>{if !running.active(){break}}}
        }
    });
    Some(Supervisor { alive, task })
}

#[cfg(test)]
#[path = "package_setup_tests.rs"]
mod tests;
