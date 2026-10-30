//! Local ambient-input consent and revocation, never workload authority.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use tokio::sync::watch;

const NOTE: &str = "Local ambient-input consent only. Microphone permission and an admitted workload are still required; no capture, workload, model, restart or retry was started. Capture integration is not available yet. Revoking a supervised future is not proof that external effects stopped.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Consent {
    pub enabled: bool,
}
impl Consent {
    pub fn from_config(config: &Value) -> Result<Self> {
        Ok(serde_json::from_value(
            config.get("listening").cloned().unwrap_or(json!({})),
        )?)
    }
}
pub fn revision(config: &Value) -> Result<u64> {
    match config.get("listening_revision") {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or_else(|| anyhow::anyhow!("Invalid listening revision")),
    }
}

/// All generic saves/restores pass here. Only the dedicated local command may
/// turn a denial into consent. Incoming revisions cannot roll back a later CAS.
pub(crate) fn preserve_revision(old: &Value, next: &mut Value, local_enable: bool) -> Result<()> {
    let before = Consent::from_config(old)?;
    let after = if next.get("listening").is_some() {
        Consent::from_config(next)?
    } else {
        before
    };
    ensure!(
        !after.enabled || before.enabled || local_enable,
        "Enable ambient-input consent with the local node listening command"
    );
    let previous = revision(old)?;
    let current = if before != after {
        previous
            .checked_add(1)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or_else(|| anyhow::anyhow!("Listening revision exhausted"))?
    } else {
        previous
    };
    if old.get("listening").is_some() || next.get("listening").is_some() || local_enable {
        next["listening"] = json!(after);
    }
    if current != 0 || old.get("listening_revision").is_some() {
        next["listening_revision"] = json!(current);
    } else if let Some(object) = next.as_object_mut() {
        object.remove("listening_revision");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    epoch: u64,
    blocked: bool,
    runtime_generation: u64,
    runtime_retired: bool,
}
pub(crate) struct Gate(watch::Sender<State>);
impl Default for Gate {
    fn default() -> Self {
        Self(watch::channel(State::default()).0)
    }
}
impl Gate {
    pub(crate) fn snapshot(&self) -> (u64, bool) {
        let state = self.state();
        (state.epoch, state.blocked || state.runtime_retired)
    }
    pub(crate) fn deny(&self) {
        self.0.send_if_modified(|state| {
            if state.blocked {
                return false;
            }
            state.epoch = state.epoch.saturating_add(1);
            state.blocked = true;
            true
        });
    }
    pub(crate) fn retire_runtime(&self) {
        self.0.send_modify(|state| {
            state.runtime_retired = true;
            if let Some(epoch) = state.epoch.checked_add(1) {
                state.epoch = epoch;
            } else {
                state.blocked = true;
            }
        });
    }
    pub(crate) fn runtime_generation(&self) -> u64 {
        self.state().runtime_generation
    }
    pub(crate) fn bind_runtime(&self) -> u64 {
        let mut generation = 0;
        self.0.send_modify(|state| {
            if state.runtime_generation == u64::MAX || state.epoch == u64::MAX {
                state.blocked = true;
                state.runtime_retired = true;
            } else {
                if state.runtime_generation > 0 {
                    state.epoch += 1;
                }
                state.runtime_generation += 1;
                state.runtime_retired = false;
            }
            generation = state.runtime_generation;
        });
        generation
    }
    pub(crate) fn retire_generation(&self, expected: u64) {
        self.0.send_if_modified(|state| {
            if state.runtime_generation != expected || state.runtime_retired {
                return false;
            }
            state.epoch = state.epoch.saturating_add(1);
            state.runtime_retired = true;
            true
        });
    }
    fn allow(&self) -> Result<()> {
        let mut exhausted = false;
        self.0.send_if_modified(|state| {
            if state.epoch == u64::MAX || state.runtime_generation == u64::MAX {
                exhausted = true;
                return false;
            }
            if !state.blocked {
                return false;
            }
            state.blocked = false;
            true
        });
        ensure!(!exhausted, "Listening revocation epoch exhausted");
        Ok(())
    }
    fn state(&self) -> State {
        *self.0.borrow()
    }
    fn permit(&self) -> Result<Permit> {
        let receiver = self.0.subscribe();
        let state = *receiver.borrow();
        ensure!(
            !state.blocked && !state.runtime_retired,
            "Listening consent revoked or runtime is retiring"
        );
        Ok(Permit {
            receiver,
            epoch: state.epoch,
        })
    }
}
struct Permit {
    receiver: watch::Receiver<State>,
    epoch: u64,
}
impl Permit {
    async fn supervise<F, T>(mut self, operation: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        // The owned operation drops inside this scope before the caller performs
        // synchronous final persistence. Off/on cannot revive an old epoch.
        let outcome = {
            tokio::pin!(operation);
            loop {
                let state = *self.receiver.borrow_and_update();
                if state.blocked || state.runtime_retired || state.epoch != self.epoch {
                    break Err(anyhow::anyhow!("Local listening consent revoked"));
                }
                tokio::select! {
                    biased;
                    changed=self.receiver.changed()=> {
                        if changed.is_err() { break Err(anyhow::anyhow!("Listening control unavailable")); }
                    },
                    result=&mut operation=> {
                        let current = *self.receiver.borrow();
                        if current.blocked || current.runtime_retired || current.epoch != self.epoch {
                            break Err(anyhow::anyhow!("Local listening authority changed before result"));
                        }
                        break result;
                    },
                }
            }
        };
        outcome
    }
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    Show,
    Enable,
    Disable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show {},
    Set {
        enabled: bool,
        expected_revision: u64,
        expected_epoch: u64,
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
        ensure!(
            self.schema_version == 1,
            "Unsupported listening command schema"
        );
        if let Command::Set {
            expected_revision, ..
        } = self.command
        {
            ensure!(
                expected_revision <= i64::MAX as u64,
                "Invalid listening revision"
            );
        }
        Ok(())
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
    consent: Consent,
    revision: u64,
    runtime_epoch: u64,
    runtime_blocked: bool,
    saved_microphone_permission: bool,
    applied_microphone_permission: bool,
    changed: bool,
    capture_integration_available: bool,
    capture_started: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "node_listening_consent"
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && r.command == request.command
        && value.pointer("/consent/enabled").and_then(Value::as_bool) == Some(r.consent.enabled)
        && r.success
        && r.revision <= i64::MAX as u64
        && !r.capture_integration_available
        && !r.capture_started
        && !r.execution_authorized
        && !r.restart_requested
        && !r.automatically_retried
        && r.note == NOTE
        && match request.command {
            Command::Show {} => !r.changed,
            Command::Set {
                enabled,
                expected_revision,
                expected_epoch,
            } => {
                r.consent.enabled == enabled
                    && r.revision == expected_revision.saturating_add(u64::from(r.changed))
                    && (r.runtime_epoch == expected_epoch
                        || r.runtime_epoch == expected_epoch.saturating_add(1))
                    && if enabled {
                        !r.runtime_blocked
                    } else {
                        r.runtime_blocked
                    }
            }
        }
}
impl crate::NodeService {
    pub fn listening_command(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Listening command targets another node"
        );
        if remote && matches!(request.command, Command::Set { .. }) {
            bail!("Ambient-input consent can only be changed by the local node owner");
        }
        self.with_permission_runtime(&request.expected_runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                    "Listening runtime/identity changed"
                );
                let mut consent = Consent::from_config(config)?;
                let mut current_revision = revision(config)?;
                let mut changed = false;
                if let Command::Set {
                    enabled,
                    expected_revision,
                    expected_epoch,
                } = request.command
                {
                    ensure!(
                        current_revision == expected_revision
                            && self.listening_gate.state().epoch == expected_epoch,
                        "Listening state changed; inspect before deciding what to do"
                    );
                    changed = consent.enabled != enabled;
                    if !enabled {
                        self.listening_gate.deny();
                    }
                    if changed {
                        let mut next = config.clone();
                        next["listening"] = json!(Consent { enabled });
                        self.save_listening_config_locked(next)?;
                        current_revision = expected_revision + 1;
                        consent.enabled = enabled;
                    }
                    if enabled {
                        self.listening_gate.allow()?;
                    }
                }
                let state = self.listening_gate.state();
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_listening_consent".into(),
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    success: true,
                    consent,
                    revision: current_revision,
                    runtime_epoch: state.epoch,
                    runtime_blocked: state.blocked || state.runtime_retired,
                    saved_microphone_permission: config.pointer("/permissions/media/microphone")
                        == Some(&json!(true)),
                    applied_microphone_permission: engine.applied_permissions().media.microphone,
                    changed,
                    capture_integration_available: false,
                    capture_started: false,
                    execution_authorized: false,
                    restart_requested: false,
                    automatically_retried: false,
                    note: NOTE.into(),
                });
                ensure!(
                    confirmed(&report, &request),
                    "Listening result unconfirmed; inspect current state"
                );
                Ok(report)
            })
        })
    }

    /// Trusted-library consent wrapper, NOT admission or a network entrypoint.
    /// Product callers still need a validated durable workload, deadline, exact
    /// resource and ongoing permission/runtime checks. Startup never calls this.
    pub async fn supervise_listening<F, T>(
        &self,
        runtime_id: &str,
        deadline: tokio::time::Instant,
        operation: F,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        self.supervise_listening_epoch(runtime_id, deadline, None, operation)
            .await
    }
    pub(crate) async fn supervise_listening_epoch<F, T>(
        &self,
        runtime_id: &str,
        deadline: tokio::time::Instant,
        expected_epoch: Option<u64>,
        operation: F,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Listening workload deadline expired"
        );
        let permit = self.with_permission_runtime(runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == runtime_id,
                    "Listening runtime/identity changed"
                );
                ensure!(
                    Consent::from_config(config)?.enabled
                        && config.pointer("/permissions/media/microphone") == Some(&json!(true))
                        && engine.applied_permissions().media.microphone,
                    "Listening requires local consent and microphone permission"
                );
                let permit = self.listening_gate.permit()?;
                ensure!(
                    expected_epoch.is_none_or(|epoch| epoch == permit.epoch),
                    "Prepared source listening epoch changed"
                );
                Ok(permit)
            })
        })?;
        tokio::time::timeout_at(deadline, permit.supervise(operation))
            .await
            .context("Listening workload deadline expired")?
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    ensure!(
        confirmed(
            value,
            &Request {
                schema_version: 1,
                expected_node_id: r.node_id.clone(),
                expected_runtime_id: r.runtime_id.clone(),
                command: r.command.clone()
            }
        ),
        "Unconfirmed listening report"
    );
    Ok(format!(
        "Node {} ambient-input consent: {}\nRuntime blocked: {}\nMicrophone permission (saved/applied): {}/{}\n{}\n",
        r.node_id,
        r.consent.enabled,
        r.runtime_blocked,
        r.saved_microphone_permission,
        r.applied_microphone_permission,
        NOTE
    ))
}

#[cfg(test)]
#[path = "listening_tests.rs"]
mod tests;
