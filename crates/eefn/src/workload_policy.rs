//! Owner-controlled continuous-workload limits and live revocation.
use crate::workload_leases::Policy;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;
use tokio::sync::watch;

pub fn from_config(config: &Value) -> Result<Policy> {
    let policy: Policy =
        serde_json::from_value(config.get("workload_policy").cloned().unwrap_or(json!({})))?;
    policy.validate()?;
    Ok(policy)
}
pub fn revision(config: &Value) -> Result<u64> {
    match config.get("workload_policy_revision") {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .filter(|v| *v <= i64::MAX as u64)
            .context("Invalid workload policy revision"),
    }
}
pub(crate) fn preserve(old: &Value, next: &mut Value, authorized: bool) -> Result<()> {
    let before = from_config(old)?;
    let after = if next.get("workload_policy").is_some() {
        from_config(next)?
    } else {
        before.clone()
    };
    ensure!(
        authorized || before == after || !after.enabled,
        "Use the workload policy command to enable or change continuous-workload limits"
    );
    let previous = revision(old)?;
    let current = previous
        .checked_add(u64::from(before != after))
        .filter(|v| *v <= i64::MAX as u64)
        .context("Workload policy revision exhausted")?;
    if old.get("workload_policy").is_some() || next.get("workload_policy").is_some() || authorized {
        next["workload_policy"] = json!(after);
    }
    if current != 0 || old.get("workload_policy_revision").is_some() {
        next["workload_policy_revision"] = json!(current);
    } else if let Some(object) = next.as_object_mut() {
        object.remove("workload_policy_revision");
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct State {
    epoch: u64,
    blocked: bool,
}
pub(crate) struct Gate(watch::Sender<State>);
impl Default for Gate {
    fn default() -> Self {
        Self(watch::channel(State::default()).0)
    }
}
impl Gate {
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
    fn allow(&self) -> Result<()> {
        ensure!(
            self.0.borrow().epoch < u64::MAX,
            "Workload policy epoch exhausted"
        );
        self.0.send_modify(|state| state.blocked = false);
        Ok(())
    }
    fn permit(&self) -> Result<Permit> {
        let epoch = self.0.borrow().epoch;
        let permit = Permit {
            epoch,
            state: self.0.subscribe(),
        };
        permit.check()?;
        Ok(permit)
    }
}
pub(crate) struct Permit {
    epoch: u64,
    state: watch::Receiver<State>,
}
impl Permit {
    fn check(&self) -> Result<()> {
        let state = *self.state.borrow();
        ensure!(
            !state.blocked && state.epoch == self.epoch,
            "Workload policy revoked or changed"
        );
        Ok(())
    }
    pub(crate) async fn supervise<F, T>(mut self, operation: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        self.check()?;
        let mut operation = Box::pin(operation);
        loop {
            let result = tokio::select! {
                biased;
                changed = self.state.changed() => {
                    changed.context("Workload policy owner retired")?;
                    self.check()?;
                    continue;
                }
                result = &mut operation => result,
            };
            self.check()?;
            return result;
        }
    }
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum Action {
    Show,
    Enable {
        #[arg(long)]
        max_session_ms: Option<u64>,
        #[arg(long)]
        max_lease_ms: Option<u64>,
    },
    Disable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Show {},
    Set {
        policy: Policy,
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
            "Unsupported workload policy command schema"
        );
        if let Command::Set {
            policy,
            expected_revision,
            ..
        } = &self.command
        {
            policy.validate()?;
            ensure!(
                *expected_revision <= i64::MAX as u64,
                "Invalid workload policy revision"
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
    success: bool,
    node_id: String,
    runtime_id: String,
    command: Command,
    policy: Policy,
    revision: u64,
    runtime_epoch: u64,
    runtime_blocked: bool,
    changed: bool,
    execution_authorized: bool,
    capture_started: bool,
    restart_requested: bool,
    automatically_retried: bool,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    request.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "node_workload_policy"
        && r.success
        && r.node_id == request.expected_node_id
        && r.runtime_id == request.expected_runtime_id
        && r.command == request.command
        && r.policy.validate().is_ok()
        && r.revision <= i64::MAX as u64
        && !r.execution_authorized
        && !r.capture_started
        && !r.restart_requested
        && !r.automatically_retried
        && match &request.command {
            Command::Show {} => !r.changed,
            Command::Set {
                policy,
                expected_revision,
                expected_epoch,
            } => {
                r.policy == *policy
                    && r.revision == expected_revision.saturating_add(u64::from(r.changed))
                    && (r.runtime_epoch == *expected_epoch
                        || r.runtime_epoch == expected_epoch.saturating_add(1))
                    && r.runtime_blocked != policy.enabled
            }
        }
}
impl crate::NodeService {
    pub fn workload_policy_command(&self, request: Request, remote: bool) -> Result<Value> {
        request.validate()?;
        ensure!(
            request.expected_node_id == self.node_id,
            "Workload policy targets another node"
        );
        self.with_permission_runtime(&request.expected_runtime_id, |_| {
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == request.expected_runtime_id,
                    "Workload policy runtime changed"
                );
                let mut policy = from_config(config)?;
                let mut current = revision(config)?;
                let mut changed = false;
                if let Command::Set {
                    policy: next,
                    expected_revision,
                    expected_epoch,
                } = &request.command
                {
                    ensure!(
                        !remote || config.pointer("/management/allow_remote") == Some(&json!(true)),
                        "Remote node management is disabled by the node owner"
                    );
                    ensure!(
                        current == *expected_revision
                            && self.workload_gate.0.borrow().epoch == *expected_epoch,
                        "Workload policy changed; inspect before deciding what to do"
                    );
                    changed = policy != *next;
                    if !next.enabled {
                        self.workload_gate.deny();
                    }
                    if changed {
                        let mut value = config.clone();
                        value["workload_policy"] = json!(next);
                        self.save_workload_config_locked(value)?;
                        current = current
                            .checked_add(1)
                            .context("Workload policy revision exhausted")?;
                        policy = next.clone();
                    }
                    if next.enabled {
                        self.workload_gate.allow()?;
                    }
                }
                let state = *self.workload_gate.0.borrow();
                let report = json!(Report {
                    schema_version: 1,
                    report_type: "node_workload_policy".into(),
                    success: true,
                    node_id: self.node_id.clone(),
                    runtime_id: request.expected_runtime_id.clone(),
                    command: request.command.clone(),
                    policy,
                    revision: current,
                    runtime_epoch: state.epoch,
                    runtime_blocked: state.blocked,
                    changed,
                    execution_authorized: false,
                    capture_started: false,
                    restart_requested: false,
                    automatically_retried: false,
                });
                ensure!(
                    confirmed(&report, &request),
                    "Workload policy outcome unconfirmed; inspect current state"
                );
                Ok(report)
            })
        })
    }
    pub(crate) fn workload_policy_permit(
        &self,
        runtime: &str,
        definition: &crate::workload_leases::definition::Definition,
    ) -> Result<(Policy, Permit)> {
        self.with_permission_runtime(runtime, |_| {
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == runtime,
                    "Workload policy runtime changed"
                );
                let policy = from_config(config)?;
                definition.lease_contract()?.permitted(&policy)?;
                Ok((policy, self.workload_gate.permit()?))
            })
        })
    }
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    let request = Request {
        schema_version: 1,
        expected_node_id: r.node_id.clone(),
        expected_runtime_id: r.runtime_id.clone(),
        command: r.command.clone(),
    };
    ensure!(
        confirmed(value, &request),
        "Unconfirmed workload policy report"
    );
    Ok(format!(
        "Node: {}\nContinuous workloads: {}\nMaximum session: {} ms\nMaximum lease: {} ms\nRuntime blocked: {}\nListening consent, microphone permission and an approved workload are still required.\n",
        r.node_id,
        if r.policy.enabled {
            "enabled"
        } else {
            "disabled"
        },
        r.policy.max_session_ms,
        r.policy.max_lease_ms,
        r.runtime_blocked
    ))
}

#[cfg(test)]
#[path = "workload_policy_tests.rs"]
mod tests;
