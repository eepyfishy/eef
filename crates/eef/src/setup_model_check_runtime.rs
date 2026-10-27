//! Own bounded setup observation; inference stays in the existing grant driver.
use super::*;
use crate::{Runtime, jobs::JobStore};
use anyhow::Context;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    task::JoinSet,
    time::Instant,
};

#[cfg(test)]
#[path = "setup_model_check_runtime_tests.rs"]
mod tests;

struct Active {
    operation: String,
    cancel: watch::Sender<bool>,
    reason: &'static str,
}
pub(crate) struct Manager {
    active: Mutex<BTreeMap<String, Active>>,
    tasks: AsyncMutex<JoinSet<()>>,
    closing: AtomicBool,
    max_active: usize,
}
struct Guard {
    manager: Arc<Manager>,
    node: String,
    operation: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut active = self.manager.active.lock().expect("setup check workers");
        if active
            .get(&self.node)
            .is_some_and(|entry| entry.operation == self.operation)
        {
            active.remove(&self.node);
        }
    }
}
struct Work {
    definition: Definition,
    deadline: Instant,
    cancelled: watch::Receiver<bool>,
    _guard: Guard,
}
pub(super) fn valid_reason(reason: &str) -> bool {
    matches!(
        reason,
        "waiting_for_setup"
            | "waiting_for_exact_model"
            | "setup_unconfirmed"
            | "permissions_unconfirmed"
            | "management_consent_required"
            | "awaiting_private_transport"
            | "waiting_for_applied_selection"
            | "waiting_for_runtime"
            | "awaiting_setup_approval"
    )
}
impl Manager {
    pub(crate) fn new(config: &crate::config::Config) -> Result<Arc<Self>> {
        let maximum = validate_settings(config.get("setup_model_checks"))?;
        Ok(Arc::new(Self {
            active: Mutex::new(BTreeMap::new()),
            tasks: AsyncMutex::new(JoinSet::new()),
            closing: AtomicBool::new(false),
            max_active: maximum,
        }))
    }
    fn prepare(
        self: &Arc<Self>,
        store: &JobStore,
        runtime: &str,
        definition: Definition,
    ) -> Result<Option<Work>> {
        definition.validate()?;
        // Active ownership and the durable commit have no async gap. A lost
        // caller cannot recreate a worker from retained consent after restart.
        let mut active = self.active.lock().expect("setup check workers");
        if let Some(existing) =
            store.setup_model_check(&definition.node_id, &definition.operation_id)?
        {
            if existing.definition != definition {
                bail!("Setup-check consent conflicts")
            }
            return Ok(None);
        }
        let mut retired = Vec::new();
        for (node, entry) in active.iter() {
            let record = store
                .setup_model_check(node, &entry.operation)?
                .context("Active setup-check consent missing; no replacement worker")?;
            if record.coordinator_runtime_id != runtime {
                bail!("Active setup-check owner changed")
            }
            if !matches!(record.state, State::Waiting | State::Launching) {
                retired.push(node.clone());
            }
        }
        for node in retired {
            active.remove(&node);
        }
        if self.closing.load(Ordering::SeqCst)
            || active.len() >= self.max_active
            || active.contains_key(&definition.node_id)
        {
            bail!("Setup check capacity unavailable; no consent saved or worker queued")
        }
        let deadline = Instant::now() + Duration::from_millis(definition.max_wait_ms);
        let (cancel, receiver) = watch::channel(false);
        let (_, created) = store.arm_setup_model_check(definition.clone(), runtime)?;
        if !created {
            return Ok(None);
        }
        active.insert(
            definition.node_id.clone(),
            Active {
                operation: definition.operation_id.clone(),
                cancel,
                reason: "waiting_for_setup",
            },
        );
        drop(active);
        Ok(Some(Work {
            _guard: Guard {
                manager: self.clone(),
                node: definition.node_id.clone(),
                operation: definition.operation_id.clone(),
            },
            definition,
            deadline,
            cancelled: receiver,
        }))
    }
    fn arm(self: &Arc<Self>, runtime: &Arc<Runtime>, definition: Definition) -> Result<bool> {
        let Some(work) = self.prepare(&runtime.engine.store, runtime.runtime_id(), definition)?
        else {
            return Ok(false);
        };
        // A committed but unstarted consent is retained and interrupted, never
        // queued or restored into authority by another request.
        let mut tasks = self
            .tasks
            .try_lock()
            .context("Setup-check task ownership unavailable; inspect retained consent")?;
        if self.closing.load(Ordering::SeqCst) {
            bail!("Setup checks are closing; inspect retained consent")
        }
        while tasks.try_join_next().is_some() {}
        let runtime = runtime.clone();
        tasks.spawn(async move { run(runtime, work).await });
        Ok(true)
    }
    fn snapshot(
        &self,
        store: &JobStore,
        runtime: &str,
        node: &str,
        operation: &str,
    ) -> Result<(Option<Record>, bool, Option<String>)> {
        // Same lock order as admission. Prevent a stale waiting record combined
        // with a newer retired worker from inventing interruption.
        let active = self.active.lock().expect("setup check workers");
        let record = store.setup_model_check(node, operation)?;
        let entry = active
            .get(node)
            .filter(|entry| entry.operation == operation);
        let owned = record.as_ref().is_some_and(|r| {
            r.coordinator_runtime_id == runtime
                && matches!(r.state, State::Waiting | State::Launching)
        }) && entry.is_some();
        let reason = if owned && record.as_ref().is_some_and(|r| r.state == State::Waiting) {
            entry.map(|entry| entry.reason.into())
        } else {
            None
        };
        Ok((record, owned, reason))
    }
    fn reason(&self, definition: &Definition, reason: &'static str) {
        if let Some(entry) = self
            .active
            .lock()
            .expect("setup check workers")
            .get_mut(&definition.node_id)
            .filter(|entry| entry.operation == definition.operation_id)
        {
            entry.reason = reason;
        }
    }
    fn cancel(&self, node: &str, operation: &str) {
        if let Some(entry) = self
            .active
            .lock()
            .expect("setup check workers")
            .get(node)
            .filter(|entry| entry.operation == operation)
        {
            entry.cancel.send_replace(true);
        }
    }
    pub(crate) async fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.tasks.lock().await.shutdown().await;
    }
}

fn reason(issue: crate::model_verification::ReadinessIssue) -> &'static str {
    use crate::model_verification::ReadinessIssue as R;
    match issue {
        R::SetupUnconfirmed => "setup_unconfirmed",
        R::PermissionsUnconfirmed => "permissions_unconfirmed",
        R::ManagementConsentRequired => "management_consent_required",
        R::ExecutionTransportUnavailable => "awaiting_private_transport",
        R::ApplySavedInterpreter | R::AmbiguousInterpreter => "waiting_for_applied_selection",
        R::RuntimeUnavailable => "waiting_for_runtime",
        R::SetupApprovalRequired => "awaiting_setup_approval",
        _ => "waiting_for_setup",
    }
}

async fn run(runtime: Arc<Runtime>, mut work: Work) {
    let deadline = work.deadline;
    let operation = async {
        loop {
            if *work.cancelled.borrow() {
                return Ok(None);
            }
            let selected = tokio::select! {
                biased;
                _ = work.cancelled.changed() => return Ok(None),
                selected = crate::model_verification::selected_on(&runtime.node_server, &work.definition.node_id, &work.definition.operation_id) => selected,
            };
            match selected {
                Ok(definition) if definition.model == work.definition.model => {
                    if Instant::now() >= deadline {
                        bail!("Setup check waiting deadline elapsed")
                    }
                    if !runtime
                        .engine
                        .store
                        .launch_setup_model_check(&work.definition, runtime.runtime_id())?
                    {
                        return Ok(None);
                    }
                    return infer(&runtime, &definition).await.map(Some);
                }
                Ok(_) => runtime
                    .setup_model_checks
                    .reason(&work.definition, "waiting_for_exact_model"),
                Err(issue) => runtime
                    .setup_model_checks
                    .reason(&work.definition, reason(issue)),
            }
            tokio::select! {
                biased;
                _ = work.cancelled.changed() => return Ok(None),
                _ = tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)) => {},
            }
        }
    };
    let (outcome, expired) = match tokio::time::timeout_at(deadline, Box::pin(operation)).await {
        Ok(Ok(None)) => return,
        Ok(Ok(Some(outcome))) => (Some(outcome), false),
        Ok(Err(_)) => (None, Instant::now() >= deadline),
        Err(_) => (None, true),
    };
    if runtime
        .engine
        .store
        .finish_setup_model_check(&work.definition, runtime.runtime_id(), outcome, expired)
        .is_err()
    {
        tracing::warn!(
            "Setup model check checkpoint unconfirmed; inspect retained operation, no retry"
        );
    }
}

async fn infer(
    runtime: &Runtime,
    definition: &eefn::model_verification::Definition,
) -> Result<Outcome> {
    use crate::model_verification::{
        Command as Verify, Request as VerifyRequest, State as VerifyState,
    };
    let mut request = VerifyRequest {
        schema_version: 1,
        expected_runtime_id: runtime.runtime_id().into(),
        node_id: definition.node_id.clone(),
        operation_id: definition.operation_id.clone(),
        command: Verify::Start {
            definition: definition.clone(),
            approve_inference: true,
        },
    };
    loop {
        let value = Box::pin(runtime.model_verification_command(request.clone())).await?;
        if !crate::model_verification::confirmed(&value, &request) {
            bail!("Fixed setup inference unconfirmed")
        }
        let state: VerifyState = serde_json::from_value(value["state"].clone())?;
        if let Some(observed) = value.get("definition").filter(|v| !v.is_null()) {
            let observed: eefn::model_verification::Definition =
                serde_json::from_value(observed.clone())?;
            if observed != *definition {
                bail!("Fixed setup check definition changed")
            }
        }
        if state != VerifyState::Running {
            let optional = |key: &str| -> Result<Option<String>> {
                serde_json::from_value(value[key].clone()).map_err(Into::into)
            };
            let outcome = Outcome {
                state,
                node_runtime_id: value["definition"]["node_runtime_id"]
                    .as_str()
                    .map(str::to_owned),
                job_id: optional("job_id")?,
                grant_id: optional("grant_id")?,
                historical: value["historical"]
                    .as_bool()
                    .context("Missing fixed check history")?,
                outcome_unknown: value["outcome_unknown"]
                    .as_bool()
                    .context("Missing fixed check uncertainty")?,
                inference_returned_text: value["inference_returned_text"]
                    .as_bool()
                    .context("Missing fixed check outcome")?,
            };
            outcome.validate()?;
            return Ok(outcome);
        }
        // Poll retained evidence only. Never repeat start/approval/execute.
        request.command = Verify::Inspect;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

impl Runtime {
    pub async fn setup_model_check_command(self: &Arc<Self>, request: Request) -> Result<Value> {
        request.validate()?;
        if request.expected_runtime_id != self.runtime_id() {
            bail!("Setup check coordinator runtime changed")
        }
        let mut created = false;
        let mut reused = false;
        let mut cancelled = false;
        match &request.command {
            Command::Arm { .. } => {
                created = self.setup_model_checks.arm(self, request.definition()?)?;
                reused = !created;
            }
            Command::CancelWait {} => {
                cancelled = self
                    .engine
                    .store
                    .cancel_setup_model_check_wait(&request.node_id, &request.operation_id)?;
                if cancelled {
                    self.setup_model_checks
                        .cancel(&request.node_id, &request.operation_id);
                }
            }
            Command::Inspect {} => {}
        }
        let (record, active, waiting_reason) = self.setup_model_checks.snapshot(
            &self.engine.store,
            self.runtime_id(),
            &request.node_id,
            &request.operation_id,
        )?;
        let effective = record
            .as_ref()
            .map(|r| effective_state(r, self.runtime_id(), active));
        let report = Report {
            schema_version: 1,
            report_type: "coordinator_setup_model_check".into(),
            coordinator_runtime_id: self.runtime_id().into(),
            node_id: request.node_id.clone(),
            operation_id: request.operation_id.clone(),
            command: request.command.clone(),
            success: true,
            record,
            effective_state: effective,
            worker_active: active,
            waiting_reason,
            consent_created: created,
            waiting_cancelled: cancelled,
            reused,
            automatically_retried: false,
            resumed: false,
            model_selection_changed: false,
            model_quality_approved: false,
            current_setup_verified: false,
        };
        let value = serde_json::to_value(report)?;
        if !confirmed(&value, &request) {
            bail!("Setup model check report unconfirmed")
        }
        Ok(value)
    }
}
