//! Backend-owned, once-per-policy setup of an explicitly pinned owner model.
//! Configuration is not readiness; interrupted/uncertain attempts never replay.
use crate::{
    NodeService,
    model_bootstrap::{CatalogPin, Mode, Saved},
    model_selection::{Backend, ModelCommand},
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct Fence {
    pub saved: Saved,
    pub owner_approved: bool,
    pub backend: Backend,
    pub runtime_id: String,
    pub models_sha256: String,
}
impl Fence {
    pub(crate) fn check(&self, service: &NodeService, config: &Value) -> Result<()> {
        if config["node_id"] != service.node_id
            || service.live.lock().unwrap()["runtime_id"] != self.runtime_id
            || Saved::from_config(config)? != self.saved
            || !self.saved.policy.automation
            || !authorized_mode(self.saved.policy.mode, self.owner_approved)
            || crate::grants::definition_digest(&config["models"])? != self.models_sha256
            || !crate::model_selection::selection_view(config)?.is_empty()
        {
            bail!("Setup authority or model settings changed")
        }
        let pin = self
            .saved
            .policy
            .catalog
            .as_ref()
            .context("Owner catalog binding required")?;
        if CatalogPin::from_config(config, &pin.model_id)? != *pin {
            bail!("Setup catalog changed")
        }
        if service.live.lock().unwrap()["setup_existing_applied_models"] == true {
            bail!("An applied model setup already exists")
        }
        if service.live.lock().unwrap()["models"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            bail!("An applied model setup already exists")
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Approved,
    Inspecting,
    Downloading,
    Verifying,
    Committing,
    Configured,
    RestartRequested,
    RestartDraining,
    RuntimeConfigured,
    ApplyBlocked,
    Stopped,
    Failed,
    Unknown,
}
impl Phase {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Configured
                | Self::RuntimeConfigured
                | Self::ApplyBlocked
                | Self::Stopped
                | Self::Failed
                | Self::Unknown
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Preflight,
    ProviderMetadata,
    TransferFailed,
    StateChanged,
    CacheVerification,
    ConfigurationUncertain,
    RuntimeInterrupted,
    PendingSettings,
    ActivationUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationEvidence {
    pub models_sha256: String,
    pub replacement_runtime_id: Option<String>,
}

/// Sanitized retained evidence, never provider URLs, paths, credentials or raw errors.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub schema_version: u32,
    pub node_id: String,
    pub id: String,
    pub runtime_id: String,
    pub saved: Saved,
    pub models_sha256: String,
    pub backend: Backend,
    pub phase: Phase,
    pub reason: Option<Reason>,
    pub download_admitted: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub owner_approved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationEvidence>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
fn is_false(value: &bool) -> bool {
    !*value
}
fn authorized_mode(mode: Mode, owner_approved: bool) -> bool {
    matches!(
        (mode, owner_approved),
        (Mode::Auto, false) | (Mode::Prompt, true)
    )
}
impl Attempt {
    fn validate(&self, node: &str) -> Result<()> {
        self.saved.validate()?;
        crate::automation_occurrence::uuid(&self.id)?;
        crate::automation_occurrence::uuid(&self.runtime_id)?;
        crate::network::validate_remote_node_id(&self.node_id)?;
        if self.schema_version != 1
            || self.node_id != node
            || self.created_at_ms == 0
            || self.updated_at_ms < self.created_at_ms
            || !self.saved.policy.automation
            || !authorized_mode(self.saved.policy.mode, self.owner_approved)
            || (self.phase == Phase::Approved && !self.owner_approved)
            || (self.phase == Phase::Approved
                && (self.download_admitted || self.activation.is_some()))
            || self.saved.policy.catalog.is_none()
            || self.models_sha256.len() != 64
            || !self.models_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || (matches!(self.phase, Phase::Configured | Phase::RuntimeConfigured)
                && self.reason.is_some())
            || (matches!(
                self.phase,
                Phase::Approved
                    | Phase::Inspecting
                    | Phase::Downloading
                    | Phase::Verifying
                    | Phase::Committing
                    | Phase::RestartRequested
                    | Phase::RestartDraining
            ) && self.reason.is_some())
            || (matches!(
                self.phase,
                Phase::Stopped | Phase::Failed | Phase::Unknown | Phase::ApplyBlocked
            ) && self.reason.is_none())
        {
            bail!("Invalid retained model setup evidence")
        }
        if matches!(
            self.phase,
            Phase::RestartRequested
                | Phase::RestartDraining
                | Phase::RuntimeConfigured
                | Phase::ApplyBlocked
        ) && !self.saved.policy.restart_after_setup
        {
            bail!("Automatic activation was not authorized")
        }
        if matches!(
            self.phase,
            Phase::RestartRequested | Phase::RestartDraining | Phase::RuntimeConfigured
        ) && self.activation.is_none()
        {
            bail!("Activation evidence missing")
        }
        if let Some(activation) = &self.activation {
            if !self.saved.policy.restart_after_setup
                || activation.models_sha256.len() != 64
                || !activation
                    .models_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
            {
                bail!("Invalid activation scope")
            }
            if let Some(runtime) = &activation.replacement_runtime_id {
                crate::automation_occurrence::uuid(runtime)?;
                if runtime == &self.runtime_id
                    || !matches!(self.phase, Phase::RuntimeConfigured | Phase::Unknown)
                {
                    bail!("Invalid replacement runtime evidence")
                }
            } else if self.phase == Phase::RuntimeConfigured {
                bail!("Replacement runtime missing")
            }
        }
        Ok(())
    }
    fn phase(&mut self, phase: Phase, reason: Option<Reason>) {
        self.phase = phase;
        self.reason = reason;
        self.updated_at_ms = crate::protocol::now_ms().max(self.updated_at_ms);
    }
}

struct Journal {
    path: PathBuf,
    node: String,
}
impl Journal {
    fn new(service: &NodeService) -> Self {
        Self {
            path: service.config_path.with_extension("bootstrap.sqlite3"),
            node: service.node_id.clone(),
        }
    }
    fn connection(&self, write: bool) -> Result<Option<Connection>> {
        let exists = match std::fs::symlink_metadata(&self.path) {
            Ok(m) if m.file_type().is_file() && m.len() <= 256 * 1024 => true,
            Ok(_) => bail!("Setup evidence path is not a regular file"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if !exists && !write {
            return Ok(None);
        }
        let flags = if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let db = Connection::open_with_flags(&self.path, flags)?;
        db.busy_timeout(Duration::from_millis(200))?;
        if !exists {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE setup(id INTEGER PRIMARY KEY CHECK(id=1), evidence TEXT NOT NULL CHECK(length(evidence)<=16384)); PRAGMA user_version=1; COMMIT;")?;
        }
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != 1 {
            bail!("Unsupported setup evidence schema; no initialization or repair")
        }
        Ok(Some(db))
    }
    fn read(&self) -> Result<Option<Attempt>> {
        let Some(db) = self.connection(false)? else {
            return Ok(None);
        };
        self.read_from(&db)
    }
    fn read_from(&self, db: &Connection) -> Result<Option<Attempt>> {
        let count: i64 = db.query_row("SELECT count(*) FROM setup", [], |r| r.get(0))?;
        if !(0..=1).contains(&count) {
            bail!("Ambiguous setup evidence")
        }
        let raw: Option<String> = db
            .query_row("SELECT evidence FROM setup WHERE id=1", [], |r| r.get(0))
            .optional()?;
        if count != 0 && raw.is_none() {
            bail!("Setup evidence identity is missing")
        }
        raw.map(|raw| {
            if raw.len() > 16384 {
                bail!("Oversized setup evidence")
            }
            let record: Attempt = serde_json::from_str(&raw)?;
            record.validate(&self.node)?;
            Ok(record)
        })
        .transpose()
    }
    fn write(&self, record: &Attempt) -> Result<()> {
        record.validate(&self.node)?;
        let raw = serde_json::to_string(record)?;
        if raw.len() > 16384 {
            bail!("Setup evidence limit exceeded")
        }
        let mut db = self
            .connection(true)?
            .context("Setup evidence unavailable")?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(old) = self.read_from(&tx)? {
            if old.id != record.id {
                if !old.phase.terminal()
                    || record.saved.revision <= old.saved.revision
                    || !matches!(record.phase, Phase::Approved | Phase::Inspecting)
                {
                    bail!(
                        "A new setup attempt requires a new explicit policy version after the prior attempt"
                    )
                }
            } else {
                if old.saved != record.saved
                    || old.owner_approved != record.owner_approved
                    || old.runtime_id != record.runtime_id
                    || old.models_sha256 != record.models_sha256
                    || old.created_at_ms != record.created_at_ms
                    || record.updated_at_ms < old.updated_at_ms
                    || (old.download_admitted && !record.download_admitted)
                    || (old.backend != record.backend && old.phase != Phase::Inspecting)
                {
                    bail!("Setup evidence scope changed")
                }
                match (&old.activation, &record.activation) {
                    (Some(a), Some(b))
                        if a.models_sha256 == b.models_sha256
                            && (a.replacement_runtime_id == b.replacement_runtime_id
                                || a.replacement_runtime_id.is_none()) => {}
                    (None, None) => {}
                    (None, Some(_))
                        if old.phase == Phase::Committing
                            && record.phase == Phase::RestartRequested => {}
                    _ => bail!("Activation evidence scope changed"),
                }
                let allowed = match old.phase {
                    Phase::Approved => matches!(
                        record.phase,
                        Phase::Inspecting | Phase::Stopped | Phase::Unknown
                    ),
                    Phase::Inspecting => matches!(
                        record.phase,
                        Phase::Downloading
                            | Phase::Verifying
                            | Phase::Failed
                            | Phase::Stopped
                            | Phase::Unknown
                    ),
                    Phase::Downloading => matches!(
                        record.phase,
                        Phase::Verifying | Phase::Failed | Phase::Stopped | Phase::Unknown
                    ),
                    Phase::Verifying => matches!(
                        record.phase,
                        Phase::Committing | Phase::Failed | Phase::Stopped | Phase::Unknown
                    ),
                    Phase::Committing => matches!(
                        record.phase,
                        Phase::Configured
                            | Phase::RestartRequested
                            | Phase::ApplyBlocked
                            | Phase::Unknown
                    ),
                    Phase::RestartRequested => matches!(
                        record.phase,
                        Phase::RestartDraining | Phase::ApplyBlocked | Phase::Unknown
                    ),
                    Phase::RestartDraining => {
                        matches!(record.phase, Phase::RuntimeConfigured | Phase::Unknown)
                    }
                    _ => false,
                };
                if !allowed {
                    bail!("Setup lifecycle cannot replay or overwrite terminal evidence")
                }
            }
        }
        tx.execute("INSERT INTO setup(id,evidence) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET evidence=excluded.evidence",params![raw])?;
        tx.commit()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Disabled,
    Preserved,
    Managed,
    AwaitingApproval,
    NoApprovedDefault,
    CatalogChanged,
    ExistingSetup,
    Waiting,
    Attempt,
    EvidenceUnavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub schema_version: u32,
    pub node_id: String,
    pub policy_digest: String,
    pub state: State,
    pub attempt: Option<Attempt>,
    pub current_policy_attempt: bool,
    pub restart_required: bool,
    pub readiness_verified: bool,
    pub automatically_retried: bool,
}
impl Status {
    pub(crate) fn validate(&self, node: &str, saved: &Saved) -> Result<()> {
        if self.schema_version != 1
            || self.node_id != node
            || self.policy_digest != saved.digest
            || self.readiness_verified
            || self.automatically_retried
        {
            bail!("Unconfirmed setup status")
        }
        if let Some(record) = &self.attempt {
            record.validate(node)?;
        }
        let current = self.attempt.as_ref().is_some_and(|r| r.saved == *saved);
        if self.current_policy_attempt != current
            || (self.restart_required
                && !self.attempt.as_ref().is_some_and(|r| {
                    current
                        && matches!(
                            r.phase,
                            Phase::Configured
                                | Phase::ApplyBlocked
                                | Phase::RestartRequested
                                | Phase::RestartDraining
                        )
                }))
        {
            bail!("Inconsistent setup evidence")
        }
        let valid_state = match self.state {
            State::EvidenceUnavailable => self.attempt.is_none(),
            State::Disabled => !saved.policy.automation || saved.policy.mode == Mode::Off,
            State::Preserved => saved.policy.automation && saved.policy.mode == Mode::Preserve,
            State::Managed => saved.policy.automation && saved.policy.mode == Mode::Managed,
            State::Attempt => current,
            State::NoApprovedDefault => saved.policy.catalog.is_none() && !current,
            State::AwaitingApproval => {
                saved.policy.automation
                    && saved.policy.mode == Mode::Prompt
                    && saved.policy.catalog.is_some()
                    && !current
            }
            State::Waiting => {
                saved.policy.automation
                    && saved.policy.mode == Mode::Auto
                    && saved.policy.catalog.is_some()
                    && !current
            }
            State::CatalogChanged | State::ExistingSetup => {
                saved.policy.automation
                    && matches!(saved.policy.mode, Mode::Auto | Mode::Prompt)
                    && !current
            }
        };
        if !valid_state {
            bail!("Setup status does not match saved policy")
        }
        Ok(())
    }
}
pub(crate) fn status(service: &NodeService, config: &Value, saved: &Saved, live: &Value) -> Status {
    let record = Journal::new(service).read();
    let unavailable = record.is_err();
    let attempt = record.ok().flatten();
    let current = attempt.as_ref().is_some_and(|r| r.saved == *saved);
    let state = if unavailable {
        State::EvidenceUnavailable
    } else if !saved.policy.automation {
        State::Disabled
    } else {
        match saved.policy.mode {
            Mode::Off => State::Disabled,
            Mode::Preserve => State::Preserved,
            Mode::Managed => State::Managed,
            _ if current => State::Attempt,
            _ if live["setup_existing_applied_models"] == true
                || crate::model_selection::selection_view(config)
                    .map_or(true, |s| !s.is_empty()) =>
            {
                State::ExistingSetup
            }
            _ if saved.policy.catalog.is_none() => State::NoApprovedDefault,
            _ if saved.policy.catalog.as_ref().is_some_and(|p| {
                !CatalogPin::from_config(config, &p.model_id).is_ok_and(|q| q == *p)
            }) =>
            {
                State::CatalogChanged
            }
            Mode::Prompt => State::AwaitingApproval,
            Mode::Auto => State::Waiting,
        }
    };
    Status {
        schema_version: 1,
        node_id: service.node_id.clone(),
        policy_digest: saved.digest.clone(),
        state,
        restart_required: attempt.as_ref().is_some_and(|r| {
            current
                && matches!(
                    r.phase,
                    Phase::Configured
                        | Phase::ApplyBlocked
                        | Phase::RestartRequested
                        | Phase::RestartDraining
                )
        }) && live["pending_restart"] == true,
        attempt,
        current_policy_attempt: current,
        readiness_verified: false,
        automatically_retried: false,
    }
}

fn active_snapshot(service: &NodeService, runtime: &str) -> Result<Option<(Value, Saved)>> {
    service.inspect_grant_configuration(|config| {
        let saved = Saved::from_config(config)?;
        if !saved.policy.automation
            || !matches!(saved.policy.mode, Mode::Auto | Mode::Prompt)
            || saved.policy.catalog.is_none()
        {
            return Ok(None);
        }
        if !crate::model_selection::selection_view(config)?.is_empty() {
            return Ok(None);
        }
        if service.live.lock().unwrap()["setup_existing_applied_models"] == true {
            return Ok(None);
        }
        let pin = saved.policy.catalog.as_ref().unwrap();
        if CatalogPin::from_config(config, &pin.model_id)? != *pin {
            return Ok(None);
        }
        if service.live.lock().unwrap()["runtime_id"] != runtime {
            bail!("Setup runtime changed")
        }
        if service.live.lock().unwrap()["models"]
            .as_array()
            .is_some_and(|m| !m.is_empty())
        {
            return Ok(None);
        }
        Ok(Some((config.clone(), saved)))
    })
}

/// One backend future per applied node runtime. Configuration saves only wake it.
pub async fn run(service: Arc<NodeService>, runtime: String) -> Result<()> {
    let _owner = match service.setup_supervisor.try_lock() {
        Ok(owner) => owner,
        Err(_) => return std::future::pending().await,
    };
    let journal = Journal::new(&service);
    // Any unfinished old runtime may have admitted a transfer or config write.
    // Recovery records uncertainty, never resumes/replays the operation.
    let recovered = journal.read().and_then(|old| {
        if let Some(mut record) = old.filter(|r| {
            !r.phase.terminal() && !(r.phase == Phase::Approved && r.runtime_id == runtime)
        }) {
            let _ = crate::model_manager::cancel(&service, Some(&record.id));
            record.phase(Phase::Unknown, Some(Reason::RuntimeInterrupted));
            journal.write(&record)?;
        }
        Ok(())
    });
    if recovered.is_err() {
        std::future::pending::<()>().await;
    }
    let mut warned = false;
    loop {
        let result = tick(service.clone(), &runtime, &journal).await;
        if result.is_err() && !warned {
            // Fail closed without taking deterministic node controls down.
            // No raw provider errors or private paths enter public status.
            tracing::warn!(
                "model setup attempt did not complete; inspect model-bootstrap-policy show"
            );
        }
        warned = result.is_err();
        tokio::select! {_=service.setup_changed.notified()=>{},_=tokio::time::sleep(Duration::from_secs(5))=>{}}
    }
}

async fn tick(service: Arc<NodeService>, runtime: &str, journal: &Journal) -> Result<()> {
    let retained = journal.read()?;
    if let Some(mut record) = retained.clone().filter(|r| r.phase == Phase::Approved) {
        let fence = Fence {
            saved: record.saved.clone(),
            owner_approved: record.owner_approved,
            backend: record.backend,
            runtime_id: record.runtime_id.clone(),
            models_sha256: record.models_sha256.clone(),
        };
        if record.runtime_id != runtime
            || service
                .inspect_grant_configuration(|c| fence.check(&service, c))
                .is_err()
        {
            record.phase(Phase::Stopped, Some(Reason::StateChanged));
            journal.write(&record)?;
            return Ok(());
        }
    }
    let Some((snapshot, saved)) = active_snapshot(&service, runtime)? else {
        return Ok(());
    };
    let approved = retained
        .as_ref()
        .is_some_and(|r| r.saved == saved && r.phase == Phase::Approved && r.owner_approved);
    if saved.policy.mode == Mode::Prompt && !approved {
        return Ok(());
    }
    if retained.as_ref().is_some_and(|r| r.saved == saved) && !approved {
        return Ok(());
    }
    // Never preempt somebody else's transfer or turn a busy admission into a retry.
    if service.live.lock().unwrap()["download"]["state"] == "downloading" {
        return Ok(());
    }
    let id = saved.policy.catalog.as_ref().unwrap().model_id.clone();
    let mut record = if approved {
        retained.unwrap()
    } else {
        new_attempt(&service, &snapshot, saved, runtime, false)?
    };
    // Consume prompt consent atomically with its last synchronous configuration
    // fence. A revocation that won this lock must not start provider inspection.
    if approved {
        let proceed = service.inspect_grant_configuration(|config| {
            let fence = Fence {
                saved: record.saved.clone(),
                owner_approved: record.owner_approved,
                backend: record.backend,
                runtime_id: record.runtime_id.clone(),
                models_sha256: record.models_sha256.clone(),
            };
            if fence.check(&service, config).is_err() {
                record.phase(Phase::Stopped, Some(Reason::StateChanged));
                journal.write(&record)?;
                return Ok(false);
            }
            record.phase(Phase::Inspecting, None);
            journal.write(&record)?;
            Ok(true)
        })?;
        if !proceed {
            return Ok(());
        }
    } else {
        // Persist inspection intent before asynchronous provider metadata or installation.
        journal.write(&record)?;
    }
    let outcome = Box::pin(perform(
        service.clone(),
        &snapshot,
        &id,
        &mut record,
        journal,
    ))
    .await;
    if let Err(reason) = outcome {
        if record.download_admitted {
            let _ = crate::model_manager::cancel(&service, Some(&record.id));
        }
        let phase = if reason == Reason::ConfigurationUncertain
            || matches!(record.phase, Phase::Committing | Phase::Configured)
        {
            Phase::Unknown
        } else if reason == Reason::StateChanged {
            Phase::Stopped
        } else {
            Phase::Failed
        };
        record.phase(phase, Some(reason));
        journal.write(&record)?;
    }
    Ok(())
}

fn new_attempt(
    service: &NodeService,
    snapshot: &Value,
    saved: Saved,
    runtime: &str,
    owner_approved: bool,
) -> Result<Attempt> {
    let now = crate::protocol::now_ms();
    Ok(Attempt {
        schema_version: 1,
        node_id: service.node_id.clone(),
        id: uuid::Uuid::new_v4().to_string(),
        runtime_id: runtime.into(),
        models_sha256: crate::grants::definition_digest(&snapshot["models"])?,
        saved,
        backend: Backend::Llamacpp,
        phase: if owner_approved {
            Phase::Approved
        } else {
            Phase::Inspecting
        },
        reason: None,
        download_admitted: false,
        owner_approved,
        activation: None,
        created_at_ms: now,
        updated_at_ms: now,
    })
}

/// Caller holds the runtime binding and configuration mutex. Consent does not
/// change policy, probe providers, or dispatch an installation in this call.
pub(crate) fn approve_locked(
    service: &NodeService,
    config: &Value,
    saved: &Saved,
    runtime: &str,
) -> Result<()> {
    if !saved.policy.automation || saved.policy.mode != Mode::Prompt {
        bail!("Approval requires enabled prompt mode; inspect model-bootstrap-policy show")
    }
    let record = new_attempt(service, config, saved.clone(), runtime, true)?;
    Fence {
        saved: saved.clone(),
        owner_approved: true,
        backend: record.backend,
        runtime_id: runtime.into(),
        models_sha256: record.models_sha256.clone(),
    }
    .check(service, config)?;
    let journal = Journal::new(service);
    if journal.read()?.is_some_and(|old| old.saved == *saved) {
        bail!("This policy already has a retained attempt; inspect show, do not repeat approval")
    }
    journal.write(&record)?;
    service.setup_changed.notify_one();
    Ok(())
}

async fn perform(
    service: Arc<NodeService>,
    snapshot: &Value,
    id: &str,
    record: &mut Attempt,
    journal: &Journal,
) -> std::result::Result<(), Reason> {
    let plan = Box::pin(crate::model_manager::install_plan(&service, id))
        .await
        .map_err(|_| Reason::Preflight)?;
    record.backend = match plan["backend"].as_str() {
        Some("ollama") => Backend::Ollama,
        Some("llamacpp") => Backend::Llamacpp,
        _ => return Err(Reason::Preflight),
    };
    let fence = Fence {
        saved: record.saved.clone(),
        owner_approved: record.owner_approved,
        backend: record.backend,
        runtime_id: record.runtime_id.clone(),
        models_sha256: record.models_sha256.clone(),
    };
    service
        .inspect_grant_configuration(|c| fence.check(&service, c))
        .map_err(|_| Reason::StateChanged)?;
    let provider_id = if record.backend == Backend::Ollama {
        let catalog = crate::model_manager::catalog(snapshot);
        let name = catalog
            .as_array()
            .and_then(|a| a.iter().find(|e| e["id"] == id))
            .and_then(|e| e["ollama"].as_str())
            .ok_or(Reason::Preflight)?;
        crate::model_selection::validate_id(name).map_err(|_| Reason::Preflight)?;
        Some(name.to_owned())
    } else {
        None
    };
    let existing = if let Some(name) = &provider_id {
        Box::pin(crate::model_manager::inspect_config(snapshot, name))
            .await
            .ok()
    } else {
        None
    };
    if let Some(details) = &existing {
        text_provider(details)?;
    }
    if existing.is_none() {
        if plan["can_request"] != true {
            return Err(Reason::Preflight);
        }
        Box::pin(crate::model_manager::install_bootstrap(
            service.clone(),
            fence.clone(),
            record.id.clone(),
        ))
        .await
        .map_err(|_| Reason::Preflight)?;
        record.download_admitted = true;
        record.phase(Phase::Downloading, None);
        journal
            .write(record)
            .map_err(|_| Reason::ConfigurationUncertain)?;
        loop {
            service
                .inspect_grant_configuration(|c| fence.check(&service, c))
                .map_err(|_| Reason::StateChanged)?;
            let receipt = service
                .download_receipts
                .lock()
                .map_err(|_| Reason::TransferFailed)?
                .get(&record.id)
                .map_err(|_| Reason::TransferFailed)?
                .ok_or(Reason::TransferFailed)?;
            match receipt.state {
                crate::download_receipts::State::Installed => break,
                crate::download_receipts::State::Downloading => {}
                _ => return Err(Reason::TransferFailed),
            }
            tokio::select! {_=service.setup_changed.notified()=>{},_=tokio::time::sleep(Duration::from_millis(200))=>{}}
        }
    }
    record.phase(Phase::Verifying, None);
    journal
        .write(record)
        .map_err(|_| Reason::ConfigurationUncertain)?;
    let _verification = service
        .installed_selection
        .try_lock()
        .map_err(|_| Reason::CacheVerification)?;
    let prepared = if record.backend == Backend::Llamacpp {
        Some(
            Box::pin(crate::installed_selection::prepare(
                service.model_directory(),
                snapshot.clone(),
                id.into(),
                true,
            ))
            .await
            .map_err(|_| Reason::CacheVerification)?,
        )
    } else {
        let details = Box::pin(crate::model_manager::inspect_config(
            snapshot,
            provider_id.as_deref().unwrap(),
        ))
        .await
        .map_err(|_| Reason::ProviderMetadata)?;
        text_provider(&details)?;
        None
    };
    commit(
        &service,
        &fence,
        record,
        journal,
        prepared.as_ref(),
        provider_id.as_deref(),
    )
    .map_err(|_| {
        if matches!(record.phase, Phase::Committing | Phase::Configured) {
            Reason::ConfigurationUncertain
        } else {
            Reason::StateChanged
        }
    })
}
fn text_provider(details: &Value) -> std::result::Result<(), Reason> {
    if details["capabilities_known"] != true
        || !details["capabilities"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c == "llm.infer"))
    {
        return Err(Reason::ProviderMetadata);
    }
    Ok(())
}
fn commit(
    service: &NodeService,
    fence: &Fence,
    record: &mut Attempt,
    journal: &Journal,
    prepared: Option<&crate::installed_selection::Prepared>,
    provider_id: Option<&str>,
) -> Result<()> {
    service.inspect_grant_configuration(|current| {
        fence.check(service, current)?;
        let mut next = current.clone();
        let selected_id = if let Some(prepared) = prepared {
            if prepared.snapshot.get("models") != current.get("models")
                || prepared.snapshot.get("model_catalog") != current.get("model_catalog")
            {
                bail!("Setup model metadata changed")
            }
            crate::installed_selection::check_index(
                &service.model_directory(),
                &prepared.index_entry,
            )?;
            crate::model_selection::apply(
                &mut next,
                &ModelCommand::SelectGguf {
                    slot: prepared.slot.clone(),
                },
            )?;
            prepared.slot.model_id.clone()
        } else {
            let name = provider_id.context("Provider model identity missing")?;
            crate::model_selection::apply(
                &mut next,
                &ModelCommand::SelectOllama {
                    model: crate::SelectedModel {
                        model_id: name.into(),
                        modality: "text".into(),
                        hints: Default::default(),
                    },
                },
            )?;
            name.into()
        };
        crate::model_selection::apply(
            &mut next,
            &ModelCommand::Interpreter {
                backend: fence.backend,
                model_id: selected_id,
                activate_backend: true,
            },
        )?;
        record.phase(Phase::Committing, None);
        journal.write(record)?;
        service.save_config_locked(next)?;
        service.live.lock().unwrap()["pending_restart"] = json!(true);
        if record.saved.policy.restart_after_setup {
            match crate::model_activation::plan_locked(service, service.read_config()?, record) {
                Ok(startup) => {
                    record.activation = Some(ActivationEvidence {
                        models_sha256: crate::grants::definition_digest(
                            &service.read_config()?["models"],
                        )?,
                        replacement_runtime_id: None,
                    });
                    record.phase(Phase::RestartRequested, None);
                    journal.write(record)?;
                    crate::model_activation::enqueue_locked(service, startup)?;
                }
                Err(reason) => {
                    record.phase(Phase::ApplyBlocked, Some(reason));
                    journal.write(record)?;
                }
            }
        } else {
            record.phase(Phase::Configured, None);
            journal.write(record)?;
        }
        Ok(())
    })
}

/// Called after the runtime's setup future is dropped. Only its exact admitted
/// transfer may be cancelled; a concurrent manual transfer is never targeted.
pub fn interrupt(service: &NodeService, runtime: &str) {
    let journal = Journal::new(service);
    let Ok(Some(mut record)) = journal.read() else {
        return;
    };
    if record.runtime_id != runtime || record.phase.terminal() {
        return;
    }
    if record.phase == Phase::RestartDraining
        && crate::model_activation::draining(service, &record.id, runtime)
    {
        return;
    }
    let _ = crate::model_manager::cancel(service, Some(&record.id));
    record.phase(Phase::Unknown, Some(Reason::RuntimeInterrupted));
    let _ = journal.write(&record);
}

pub(crate) fn activation_transition(
    service: &NodeService,
    scope: &crate::model_activation::Scope,
    phase: Phase,
    reason: Option<Reason>,
    runtime: Option<&str>,
) -> Result<()> {
    let journal = Journal::new(service);
    let mut record = journal
        .read()?
        .context("Retained activation evidence missing; no initialization")?;
    if record.id != scope.id
        || record.runtime_id != scope.origin_runtime
        || record.saved != scope.saved
        || record.activation.as_ref().map(|a| &a.models_sha256) != Some(&scope.models_sha256)
    {
        bail!("Activation scope changed")
    }
    if let Some(runtime) = runtime {
        record.activation.as_mut().unwrap().replacement_runtime_id = Some(runtime.into());
    }
    record.phase(phase, reason);
    journal.write(&record)
}

#[cfg(test)]
#[path = "model_setup_tests.rs"]
mod tests;
