//! Owner commands for bounded automatic fixed-input scheduling; no browser rules.
use crate::{
    Runtime,
    jobs::automation_schedules::{Arm, Schedule, State},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::atomic::Ordering};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleCommand {
    /// Enable fixed-delay automatic work under one exact existing owner policy.
    Enable {
        #[arg(long)]
        policy: String,
        #[arg(long)]
        policy_sha256: String,
        #[arg(long)]
        operation_id: String,
    },
    /// Fence future work and request ordinary job stop; not physical-stop proof.
    Stop {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: String,
    },
    /// Resolve only a retained backend pre-launch refusal; never missing work.
    ResolveRefusal {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        trigger_id: String,
        #[arg(long)]
        refusal_sha256: String,
        #[arg(long)]
        operation_id: String,
    },
    Show {
        id: String,
    },
    List {
        #[arg(long, default_value_t = 25)]
        limit: u16,
        #[arg(long)]
        after: Option<String>,
    },
}
impl ScheduleCommand {
    pub fn receipt(&self) -> Option<&str> {
        match self {
            Self::Enable { operation_id, .. }
            | Self::Stop { operation_id, .. }
            | Self::ResolveRefusal { operation_id, .. } => Some(operation_id),
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(id) = self.receipt() {
            eefn::automation_occurrence::uuid(id)?;
        }
        match self {
            Self::Enable {
                policy,
                policy_sha256,
                operation_id,
            } => {
                eefn::automation_occurrence::uuid(policy)?;
                if policy == operation_id
                    || policy_sha256.len() != 64
                    || !policy_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    bail!("Exact policy/hash/receipt required");
                }
            }
            Self::Stop { id, revision, .. } => {
                eefn::automation_occurrence::uuid(id)?;
                if *revision == 0 || *revision == u64::MAX {
                    bail!("Exact schedule revision required");
                }
            }
            Self::ResolveRefusal {
                id,
                revision,
                trigger_id,
                refusal_sha256,
                ..
            } => {
                eefn::automation_occurrence::uuid(id)?;
                eefn::automation_occurrence::uuid(trigger_id)?;
                if *revision == 0
                    || *revision == u64::MAX
                    || refusal_sha256.len() != 64
                    || !refusal_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    bail!("Exact backend refusal revision/hash/trigger required");
                }
            }
            Self::Show { id } => eefn::automation_occurrence::uuid(id)?,
            Self::List { limit, after } => {
                if !(1..=100).contains(limit) {
                    bail!("Schedule page limit must be 1-100");
                }
                if let Some(id) = after {
                    eefn::automation_occurrence::uuid(id)?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRequest {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: ScheduleCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    runtime_id: String,
    command: ScheduleCommand,
    schedule: Option<Schedule>,
    items: Vec<Value>,
    next_after: Option<String>,
    reused: bool,
    timer_active: bool,
    activation_requested: bool,
    current_trigger_id: Option<String>,
    current_job_id: Option<String>,
    current_job_status: Option<String>,
    job_stop_requested: bool,
    automatically_retried: bool,
    note: String,
}
fn summary(s: &Schedule) -> Value {
    json!({"id":s.id,"definition_id":s.definition_id,"policy_id":s.creation.policy,"runtime_id":s.runtime_id,
    "revision":s.revision,"state":s.state,"next_due_ms":s.next_due_ms,"claimed_runs":s.claimed_runs,"active_trigger":s.active_trigger,"reason":s.reason})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    id: String,
    definition_id: String,
    policy_id: String,
    runtime_id: String,
    revision: u64,
    state: State,
    next_due_ms: Option<u64>,
    claimed_runs: u16,
    active_trigger: Option<String>,
    reason: Option<String>,
}
impl Runtime {
    pub async fn automation_schedule_command(&self, request: ScheduleRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_runtime_id != self.instance_id {
            bail!("Schedule schema/coordinator runtime changed");
        }
        request.command.validate()?;
        let command = &request.command;
        let mut selected = None;
        let mut reused = false;
        let mut items = Vec::new();
        let mut next = None;
        let mut stopped = false;
        match command {
            ScheduleCommand::ResolveRefusal {
                id,
                revision,
                trigger_id,
                refusal_sha256,
                operation_id,
            } => {
                let (s, was_reused) = self.engine.resolve_schedule_refusal(
                    id,
                    *revision,
                    trigger_id,
                    refusal_sha256,
                    operation_id,
                )?;
                self.automation_timers
                    .active
                    .lock()
                    .expect("timer active")
                    .remove(id);
                selected = Some(s);
                reused = was_reused;
            }
            ScheduleCommand::Enable {
                policy,
                policy_sha256,
                operation_id,
            } => {
                let arm = Arm {
                    policy: policy.clone(),
                    policy_sha256: policy_sha256.clone(),
                    operation_id: operation_id.clone(),
                };
                if let Some(old) = self
                    .engine
                    .store
                    .schedule_list()?
                    .into_iter()
                    .find(|s| s.id == *operation_id)
                {
                    if old.creation != arm {
                        bail!("Schedule receipt conflicts; inspect original state");
                    }
                    selected = Some(old);
                    reused = true;
                } else {
                    let issuer = self
                        .schedule_enable_preflight(policy, policy_sha256)
                        .await?;
                    let manager = &self.automation_timers;
                    let mut active = manager.active.lock().expect("timer active");
                    if manager.closing.load(Ordering::SeqCst) {
                        bail!("Coordinator stopping; no activation");
                    }
                    if active.len() >= manager.max_active {
                        bail!("Active schedule limit reached; no activation");
                    }
                    let (s, was_reused) =
                        self.engine.arm_schedule(arm, &self.instance_id, &issuer)?;
                    // No await between durable activation and current-runtime
                    // management. Exact old receipts never re-register schedules.
                    if !was_reused {
                        active.insert(s.id.clone());
                    }
                    selected = Some(s);
                    reused = was_reused;
                }
            }
            ScheduleCommand::Stop {
                id,
                revision,
                operation_id,
            } => {
                let (s, was_reused) = self.engine.stop_schedule(id, *revision, operation_id)?;
                reused = was_reused;
                self.automation_timers
                    .active
                    .lock()
                    .expect("timer active")
                    .remove(id);
                if !reused {
                    if let Some(trigger) = &s.active_trigger {
                        if let Some(job) = self.engine.store.trigger_job(trigger)? {
                            if matches!(
                                job.status,
                                crate::task::PlanStatus::Review
                                    | crate::task::PlanStatus::AwaitingApproval
                                    | crate::task::PlanStatus::Approved
                                    | crate::task::PlanStatus::Running
                                    | crate::task::PlanStatus::Stopping
                                    | crate::task::PlanStatus::Interrupted
                            ) {
                                self.engine.cancel(&job.plan_id)?;
                                stopped = true;
                            }
                        }
                    }
                }
                selected = Some(s);
            }
            ScheduleCommand::Show { id } => selected = Some(self.engine.store.schedule_get(id)?),
            ScheduleCommand::List { limit, after } => {
                for s in self
                    .engine
                    .store
                    .schedule_list()?
                    .into_iter()
                    .filter(|s| after.as_ref().is_none_or(|after| s.id > *after))
                {
                    if items.len() == *limit as usize {
                        next = items
                            .last()
                            .and_then(|v: &Value| v["id"].as_str())
                            .map(str::to_owned);
                        break;
                    }
                    items.push(summary(&s));
                }
            }
        }
        let trigger = selected.as_ref().and_then(|s| s.active_trigger.clone());
        let job = trigger
            .as_ref()
            .map(|id| self.engine.store.trigger_job(id))
            .transpose()?
            .flatten();
        let timer_active = selected.as_ref().is_some_and(|s| {
            s.runtime_id == self.instance_id
                && matches!(s.state, State::Waiting | State::InFlight)
                && self
                    .automation_timers
                    .active
                    .lock()
                    .expect("timer active")
                    .contains(&s.id)
        });
        let value=json!(Report {schema_version:1,report_type:"automation_schedule".into(),runtime_id:self.instance_id.clone(),command:command.clone(),schedule:selected,
            items,next_after:next,reused,timer_active,activation_requested:matches!(command,ScheduleCommand::Enable{..})&&!reused,current_trigger_id:trigger,
            current_job_id:job.as_ref().map(|p|p.plan_id.clone()),current_job_status:job.as_ref().map(|p|serde_json::to_value(p.status).unwrap().as_str().unwrap().into()),job_stop_requested:stopped,
            automatically_retried:false,note:"Private actual schedule/job snapshot, not readiness, completion or physical-stop proof. Enable permits future fixed-scope automatic work; exact receipt reuse is inspection, never renewal/replay. Failed or unknown work requires inspection.".into()});
        if !confirmed(&value, command, &self.instance_id) {
            bail!("Schedule confirmation changed; inspect exact receipt without replacement");
        }
        Ok(value)
    }
    async fn schedule_enable_preflight(&self, id: &str, hash: &str) -> Result<String> {
        let p = self
            .engine
            .store
            .automation_authorization(id)?
            .context("Policy missing")?;
        if p.authorization_sha256 != hash {
            bail!("Exact policy digest required");
        }
        let eefn::automation_policy::PolicyCommand::Approve { definition, .. } = &p.creation else {
            unreachable!()
        };
        let reply = self.engine.store.automation_command(
            eefn::automation::AutomationCommand::Show {
                id: definition.clone(),
            },
            None,
        )?;
        let definition: eefn::automation::Definition =
            serde_json::from_value(reply["definition"].clone())?;
        let source: crate::task::Plan =
            serde_json::from_value(definition.source.context("Definition removed")?)?;
        eefn::request_submission::ConcreteInput {
            schema_version: 1,
            original_text: "Owner scheduled fixed input".into(),
            title: source.goal.description.clone(),
            input: p.scope.input.clone(),
        }
        .validate()?;
        let origin = p.scope.owner_context.origin_node();
        let (_, age) = self
            .node_server
            .peer_records(&BTreeSet::from([origin.to_owned()]))
            .await?
            .pop()
            .context("Origin node not registered")?;
        if age.as_millis() > u128::from(self.registry.scheduling_policy().freshness_seconds) * 1000
        {
            bail!("Origin node stale");
        }
        let state = self
            .node_server
            .node_state(origin)
            .await
            .context("Origin disconnected")?;
        let metadata: eefn::context::NodeMetadata = serde_json::from_value(
            state
                .registration
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        if metadata.area != p.scope.owner_context.origin_area() {
            bail!("Origin area changed; create a new explicit definition");
        }
        let driver = crate::request_execution::Driver::new(self);
        let limits = p.creation.limits().unwrap();
        for i in 0..source.tasks.len() {
            let session = driver.session(&source, i).await?;
            let bounds: eefn::grants::GrantPolicy =
                serde_json::from_value(session["limits"].clone())?;
            bounds.validate()?;
            if session["issuer_id"] != p.issuer_id
                || limits.max_execution_ms > bounds.max_duration_ms
                || limits.max_result_bytes > bounds.max_result_bytes
            {
                bail!("Exact current provider/issuer/grant bounds unavailable");
            }
        }
        Ok(p.issuer_id)
    }
}
pub fn confirmed(value: &Value, command: &ScheduleCommand, runtime: &str) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || eefn::automation_occurrence::uuid(runtime).is_err()
        || r.schema_version != 1
        || r.report_type != "automation_schedule"
        || r.runtime_id != runtime
        || r.command != *command
        || r.automatically_retried
        || r.note.len() > 1024
        || r.current_job_id
            .as_ref()
            .is_some_and(|id| eefn::request_drafts::validate_id(id).is_err())
        || r.current_job_status.as_ref().is_some_and(|status| {
            serde_json::from_value::<crate::task::PlanStatus>(json!(status)).is_err()
        })
        || r.current_job_id.is_some() != r.current_job_status.is_some()
        || (r.current_trigger_id.is_none() && r.current_job_id.is_some())
    {
        return false;
    }
    match command {
        ScheduleCommand::List { limit, after } => {
            if r.schedule.is_some()
                || r.reused
                || r.timer_active
                || r.activation_requested
                || r.job_stop_requested
                || r.items.len() > *limit as usize
            {
                return false;
            }
            let mut last = after.as_deref();
            for item in &r.items {
                let Ok(summary) = serde_json::from_value::<Summary>(item.clone()) else {
                    return false;
                };
                let Some(id) = item["id"].as_str() else {
                    return false;
                };
                if eefn::automation_occurrence::uuid(id).is_err()
                    || [
                        &summary.definition_id,
                        &summary.policy_id,
                        &summary.runtime_id,
                    ]
                    .iter()
                    .any(|id| eefn::automation_occurrence::uuid(id).is_err())
                    || summary.id != id
                    || summary.revision == 0
                    || summary.claimed_runs > 128
                    || summary
                        .active_trigger
                        .as_ref()
                        .is_some_and(|id| eefn::automation_occurrence::uuid(id).is_err())
                    || (summary.state == State::Waiting
                        && (summary.next_due_ms.is_none()
                            || summary.active_trigger.is_some()
                            || summary.reason.is_some()))
                    || (summary.state != State::Waiting && summary.next_due_ms.is_some())
                    || last.is_some_and(|last| id <= last)
                    || item.get("creation").is_some()
                {
                    return false;
                }
                last = Some(id);
            }
            r.next_after
                .as_deref()
                .is_none_or(|id| r.items.len() == *limit as usize && Some(id) == last)
        }
        _ => {
            let Some(s) = r.schedule else { return false };
            let mut raw = json!(s);
            raw.as_object_mut().unwrap().remove("record_sha256");
            if s.record_sha256 != eefn::grants::definition_digest(&raw).unwrap_or_default()
                || s.schema_version != 1
                || [&s.id, &s.definition_id, &s.runtime_id, &s.creation.policy]
                    .iter()
                    .any(|id| eefn::automation_occurrence::uuid(id).is_err())
                || s.creation.operation_id != s.id
                || s.revision == 0
                || s.claimed_runs > 128
                || s.armed_at_ms == 0
                || s.expires_at_ms <= s.armed_at_ms
                || r.items.len() != 0
                || r.next_after.is_some()
                || r.current_trigger_id != s.active_trigger
                || (r.timer_active
                    && (s.runtime_id != runtime
                        || !matches!(s.state, State::Waiting | State::InFlight)))
                || !confirmed_refusal(&s, r.current_job_id.as_deref())
            {
                return false;
            }
            match command {
                ScheduleCommand::Enable {
                    policy,
                    policy_sha256,
                    operation_id,
                } => {
                    s.creation.policy == *policy
                        && s.creation.policy_sha256 == *policy_sha256
                        && s.id == *operation_id
                        && r.activation_requested == !r.reused
                        && !r.job_stop_requested
                }
                ScheduleCommand::Stop {
                    id,
                    revision,
                    operation_id,
                } => {
                    s.id == *id
                        && matches!(s.state, State::Stopped | State::Resolved)
                        && s.stop.as_ref().is_some_and(|stop| {
                            stop.source_revision == *revision && stop.operation_id == *operation_id
                        })
                        && !r.timer_active
                        && !r.activation_requested
                        && (!r.reused || !r.job_stop_requested)
                }
                ScheduleCommand::Show { id } => {
                    s.id == *id && !r.reused && !r.activation_requested && !r.job_stop_requested
                }
                ScheduleCommand::ResolveRefusal {
                    id,
                    revision,
                    trigger_id,
                    refusal_sha256,
                    operation_id,
                } => {
                    s.id == *id
                        && s.state == State::Resolved
                        && !r.timer_active
                        && !r.activation_requested
                        && !r.job_stop_requested
                        && s.refusal_resolution.as_ref().is_some_and(|v| {
                            v.source_revision == *revision
                                && v.trigger_id == *trigger_id
                                && v.refusal_sha256 == *refusal_sha256
                                && v.operation_id == *operation_id
                        })
                        && s.refusal.as_ref().is_some_and(|v| {
                            v.trigger_id == *trigger_id && v.refusal_sha256 == *refusal_sha256
                        })
                }
                _ => false,
            }
        }
    }
}
// Check the nested no-work proof too: an outer digest is only integrity, not
// permission to accept contradictory or future refusal/lifecycle evidence.
fn confirmed_refusal(s: &Schedule, job: Option<&str>) -> bool {
    let Some(refusal) = &s.refusal else {
        return s.refusal_resolution.is_none() && s.state != State::Resolved;
    };
    let mut raw = json!(refusal);
    raw.as_object_mut().unwrap().remove("refusal_sha256");
    if refusal.schema_version != 1
        || refusal.runtime_id != s.runtime_id
        || refusal.observed_at_ms == 0
        || refusal.phase != "refused_before_launch"
        || refusal.trigger_sha256.len() != 64
        || !refusal
            .trigger_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || eefn::automation_occurrence::uuid(&refusal.trigger_id).is_err()
        || s.active_trigger.as_deref() != Some(&refusal.trigger_id)
        || s.launch_started_at_ms.is_some()
        || job.is_some()
        || s.next_due_ms.is_some()
        || !matches!(
            s.state,
            State::Interrupted | State::Stopped | State::Resolved
        )
        || refusal.refusal_sha256 != eefn::grants::definition_digest(&raw).unwrap_or_default()
    {
        return false;
    }
    match &s.refusal_resolution {
        None => s.state != State::Resolved,
        Some(r) => {
            s.state == State::Resolved
                && s.reason.as_deref() == Some("refusal_resolved")
                && r.source_revision.checked_add(1) == Some(s.revision)
                && r.trigger_id == refusal.trigger_id
                && r.refusal_sha256 == refusal.refusal_sha256
                && r.resolved_at_ms != 0
                && eefn::automation_occurrence::uuid(&r.operation_id).is_ok()
                && r.operation_id != s.id
                && r.operation_id != refusal.trigger_id
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (ScheduleCommand, Value, String) {
        let runtime = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let policy = uuid::Uuid::new_v4().to_string();
        let command = ScheduleCommand::Enable {
            policy: policy.clone(),
            policy_sha256: "a".repeat(64),
            operation_id: id.clone(),
        };
        let mut s = Schedule {
            schema_version: 1,
            id: id.clone(),
            creation: Arm {
                policy,
                policy_sha256: "a".repeat(64),
                operation_id: id,
            },
            definition_id: uuid::Uuid::new_v4().to_string(),
            runtime_id: runtime.clone(),
            issuer_id: "b".repeat(64),
            generation: 1,
            armed_at_ms: 10,
            expires_at_ms: 60000,
            every_ms: 1000,
            revision: 1,
            state: State::Waiting,
            observed_at_ms: 10,
            next_due_ms: Some(1010),
            active_trigger: None,
            launch_started_at_ms: None,
            refusal: None,
            refusal_resolution: None,
            claimed_runs: 0,
            stop: None,
            reason: None,
            record_sha256: String::new(),
        };
        let mut raw = json!(s);
        raw.as_object_mut().unwrap().remove("record_sha256");
        s.record_sha256 = eefn::grants::definition_digest(&raw).unwrap();
        let value = json!(Report {
            schema_version: 1,
            report_type: "automation_schedule".into(),
            runtime_id: runtime.clone(),
            command: command.clone(),
            schedule: Some(s),
            items: vec![],
            next_after: None,
            reused: false,
            timer_active: true,
            activation_requested: true,
            current_trigger_id: None,
            current_job_id: None,
            current_job_status: None,
            job_stop_requested: false,
            automatically_retried: false,
            note: "Actual schedule state".into()
        });
        (command, value, runtime)
    }
    #[test]
    fn refusal_resolution_reply_is_exact_and_does_not_enable_work_or_forge_stop() {
        let (_, mut value, runtime) = fixture();
        let mut s: Schedule = serde_json::from_value(value["schedule"].clone()).unwrap();
        let trigger = uuid::Uuid::new_v4().to_string();
        let op = uuid::Uuid::new_v4().to_string();
        s.state = State::Resolved;
        s.revision = 4;
        s.next_due_ms = None;
        s.active_trigger = Some(trigger.clone());
        s.reason = Some("refusal_resolved".into());
        s.claimed_runs = 1;
        s.refusal = Some(crate::jobs::automation_schedules::Refusal {
            schema_version: 1,
            trigger_id: trigger.clone(),
            trigger_sha256: "d".repeat(64),
            runtime_id: runtime.clone(),
            observed_at_ms: 20,
            phase: "refused_before_launch".into(),
            refusal_sha256: String::new(),
        });
        let mut refusal_raw = json!(s.refusal.as_ref().unwrap());
        refusal_raw
            .as_object_mut()
            .unwrap()
            .remove("refusal_sha256");
        let hash = eefn::grants::definition_digest(&refusal_raw).unwrap();
        s.refusal.as_mut().unwrap().refusal_sha256 = hash.clone();
        s.refusal_resolution = Some(crate::jobs::automation_schedules::RefusalResolution {
            operation_id: op.clone(),
            source_revision: 3,
            trigger_id: trigger.clone(),
            refusal_sha256: hash.clone(),
            resolved_at_ms: 30,
        });
        let mut raw = json!(s);
        raw.as_object_mut().unwrap().remove("record_sha256");
        s.record_sha256 = eefn::grants::definition_digest(&raw).unwrap();
        let command = ScheduleCommand::ResolveRefusal {
            id: s.id.clone(),
            revision: 3,
            trigger_id: trigger.clone(),
            refusal_sha256: hash,
            operation_id: op,
        };
        value["command"] = json!(command);
        value["schedule"] = json!(s);
        value["current_trigger_id"] = json!(trigger);
        value["activation_requested"] = json!(false);
        value["timer_active"] = json!(false);
        assert!(confirmed(&value, &command, &runtime));
        for field in [
            "timer_active",
            "activation_requested",
            "job_stop_requested",
            "automatically_retried",
        ] {
            let mut bad = value.clone();
            bad[field] = json!(true);
            assert!(!confirmed(&bad, &command, &runtime));
        }
        // Recomputed outer digests must not hide an invalid nested proof.
        for field in ["schema_version", "phase", "runtime_id", "refusal_sha256"] {
            let mut bad = value.clone();
            bad["schedule"]["refusal"][field] = if field == "schema_version" {
                json!(2)
            } else {
                json!("contradictory")
            };
            let mut raw = bad["schedule"].clone();
            raw.as_object_mut().unwrap().remove("record_sha256");
            bad["schedule"]["record_sha256"] =
                json!(eefn::grants::definition_digest(&raw).unwrap());
            assert!(!confirmed(&bad, &command, &runtime), "{field}");
        }
        for field in ["launch_started_at_ms", "revision", "reason"] {
            let mut bad = value.clone();
            bad["schedule"][field] = match field {
                "launch_started_at_ms" => json!(20),
                "revision" => json!(5),
                _ => json!("job_unresolved"),
            };
            let mut raw = bad["schedule"].clone();
            raw.as_object_mut().unwrap().remove("record_sha256");
            bad["schedule"]["record_sha256"] =
                json!(eefn::grants::definition_digest(&raw).unwrap());
            assert!(!confirmed(&bad, &command, &runtime), "{field}");
        }
        let mut bad = value.clone();
        bad["current_job_id"] = json!("existing-job");
        bad["current_job_status"] = json!("cancelled");
        assert!(!confirmed(&bad, &command, &runtime));
        value["schedule"]["refusal_resolution"]["operation_id"] =
            json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed(&value, &command, &runtime));
    }
    #[test]
    fn strict_enable_reply_is_correlated_and_rejects_substitution_or_hidden_retry() {
        let (command, value, runtime) = fixture();
        assert!(confirmed(&value, &command, &runtime));
        for field in [
            "runtime_id",
            "schedule",
            "automatically_retried",
            "activation_requested",
            "current_job_id",
        ] {
            let mut bad = value.clone();
            match field {
                "schedule" => bad["schedule"]["issuer_id"] = json!("forged"),
                "automatically_retried" => bad[field] = json!(true),
                "activation_requested" => bad[field] = json!(false),
                _ => bad[field] = json!("bad"),
            };
            assert!(!confirmed(&bad, &command, &runtime), "{field}");
        }
        let mut reused = value;
        reused["reused"] = json!(true);
        reused["activation_requested"] = json!(false);
        assert!(confirmed(&reused, &command, &runtime));
    }
    #[test]
    fn strict_commands_and_summary_pages_cannot_import_permissions_or_private_scope() {
        let (command, value, runtime) = fixture();
        let raw = json!(command);
        for field in ["execute", "force", "permissions", "renew", "retry"] {
            let mut bad = raw.clone();
            bad[field] = json!(true);
            assert!(serde_json::from_value::<ScheduleCommand>(bad).is_err());
        }
        let list = ScheduleCommand::List {
            limit: 1,
            after: None,
        };
        let record: Schedule = serde_json::from_value(value["schedule"].clone()).unwrap();
        let mut page = value;
        page["command"] = json!(list);
        page["schedule"] = Value::Null;
        page["activation_requested"] = json!(false);
        page["timer_active"] = json!(false);
        page["items"] = json!([summary(&record)]);
        page["next_after"] = json!(record.id);
        assert!(confirmed(&page, &list, &runtime));
        page["items"][0]["private_scope"] = json!({"params":"secret"});
        assert!(!confirmed(&page, &list, &runtime));
    }
}
