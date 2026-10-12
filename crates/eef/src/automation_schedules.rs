//! Durable trigger ledger and authority fences for the existing timer/grant driver.
use super::*;
use eefn::automation_occurrence::{TriggerLink, uuid};
use eefn::automation_policy::{Authorization, PolicyCommand};
const SCHEDULE_COST: usize = 4096;

// These records never fabricate LM history, grant dispatch or completion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arm {
    pub policy: String,
    pub policy_sha256: String,
    pub operation_id: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Waiting,
    InFlight,
    Stopped,
    Interrupted,
    Exhausted,
    Resolved,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stop {
    pub operation_id: String,
    pub source_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Refusal {
    pub schema_version: u32,
    pub trigger_id: String,
    pub trigger_sha256: String,
    pub runtime_id: String,
    pub observed_at_ms: u64,
    pub phase: String,
    pub refusal_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefusalResolution {
    pub operation_id: String,
    pub source_revision: u64,
    pub trigger_id: String,
    pub refusal_sha256: String,
    pub resolved_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Schedule {
    pub schema_version: u32,
    pub id: String,
    pub creation: Arm,
    pub definition_id: String,
    pub runtime_id: String,
    pub issuer_id: String,
    pub generation: u64,
    pub armed_at_ms: u64,
    pub expires_at_ms: u64,
    pub every_ms: u64,
    pub revision: u64,
    pub state: State,
    pub observed_at_ms: u64,
    pub next_due_ms: Option<u64>,
    pub active_trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_resolution: Option<RefusalResolution>,
    pub claimed_runs: u16,
    pub stop: Option<Stop>,
    pub reason: Option<String>,
    pub record_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Trigger {
    pub schema_version: u32,
    pub id: String,
    pub schedule_id: String,
    pub ordinal: u16,
    pub due_at_ms: u64,
    pub claimed_at_ms: u64,
    pub request_context: eefn::context::RequestContext,
    pub reservation_id: String,
    pub execution_id: String,
    pub record_sha256: String,
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    let mut value = serde_json::to_value(value)?;
    value.as_object_mut().unwrap().remove("record_sha256");
    eefn::grants::definition_digest(&value)
}
pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS automation_schedules(schedule_id TEXT PRIMARY KEY,record_json TEXT NOT NULL,reserved_bytes INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS automation_triggers(trigger_id TEXT PRIMARY KEY,schedule_id TEXT NOT NULL,ordinal INTEGER NOT NULL,request_id TEXT UNIQUE NOT NULL,reservation_id TEXT UNIQUE NOT NULL,record_json TEXT NOT NULL,UNIQUE(schedule_id,ordinal));")?;
    Ok(())
}
fn policy(connection: &Connection, record: &Schedule) -> Result<Authorization> {
    automation_authorizations::read(connection, &record.creation.policy)?
        .context("Trigger policy missing")
}
fn validate_schedule(connection: &Connection, s: &Schedule) -> Result<()> {
    for id in [&s.id, &s.definition_id, &s.runtime_id, &s.creation.policy] {
        uuid(id)?;
    }
    let p = policy(connection, s)?;
    let PolicyCommand::Approve {
        definition,
        max_runs,
        ..
    } = &p.creation
    else {
        unreachable!()
    };
    let d = automations::read_definition(connection, definition)?;
    let eefn::automation::AutomationCommand::Define { every_ms, .. } = d.creation else {
        unreachable!()
    };
    if s.schema_version != 1
        || s.creation.operation_id != s.id
        || s.creation.policy_sha256 != p.authorization_sha256
        || s.id == p.id
        || s.id == s.definition_id
        || s.id == s.runtime_id
        || s.definition_id != *definition
        || s.runtime_id != p.runtime_id
        || s.issuer_id != p.issuer_id
        || s.generation != p.generation
        || s.expires_at_ms != p.expires_at_ms
        || s.armed_at_ms < p.approved_at_ms
        || s.armed_at_ms >= s.expires_at_ms
        || s.every_ms != every_ms
        || s.revision == 0
        || s.revision < u64::from(s.claimed_runs) * 2
        || s.claimed_runs > *max_runs
        || s.observed_at_ms == 0
        || s.record_sha256 != digest(s)?
        || serde_json::to_vec(s)?.len() > 4096
    {
        bail!("Damaged/future schedule; retain without activation");
    }
    if let Some(id) = &s.active_trigger {
        uuid(id)?;
    }
    if let Some(stop) = &s.stop {
        uuid(&stop.operation_id)?;
        if stop.operation_id == s.id
            || (s.refusal_resolution.is_none()
                && stop.source_revision.checked_add(1) != Some(s.revision))
            || (s.refusal_resolution.is_some()
                && stop.source_revision.saturating_add(1) >= s.revision)
            || !matches!(s.state, State::Stopped | State::Resolved)
        {
            bail!("Damaged schedule stop receipt");
        }
    }
    if let Some(refusal) = &s.refusal {
        uuid(&refusal.trigger_id)?;
        uuid(&refusal.runtime_id)?;
        let mut value = json!(refusal);
        value.as_object_mut().unwrap().remove("refusal_sha256");
        if refusal.schema_version != 1
            || refusal.runtime_id != s.runtime_id
            || refusal.observed_at_ms == 0
            || s.active_trigger.as_deref() != Some(&refusal.trigger_id)
            || s.launch_started_at_ms.is_some()
            || !matches!(
                s.state,
                State::Interrupted | State::Stopped | State::Resolved
            )
            || refusal.phase != "refused_before_launch"
            || refusal.refusal_sha256 != eefn::grants::definition_digest(&value)?
        {
            bail!("Damaged backend refusal evidence; no no-work inference");
        }
    }
    if let Some(resolution) = &s.refusal_resolution {
        uuid(&resolution.operation_id)?;
        let refusal = s
            .refusal
            .as_ref()
            .context("Resolution lacks actual backend refusal")?;
        if s.state != State::Resolved
            || resolution.operation_id == s.id
            || resolution.operation_id == refusal.trigger_id
            || resolution.source_revision.checked_add(1) != Some(s.revision)
            || resolution.resolved_at_ms == 0
            || resolution.refusal_sha256 != refusal.refusal_sha256
            || resolution.trigger_id != refusal.trigger_id
        {
            bail!("Damaged exact refusal-resolution receipt");
        }
    } else if s.state == State::Resolved {
        bail!("Resolved schedule lacks its exact receipt");
    }
    let shape = match s.state {
        State::Waiting => {
            s.stop.is_none()
                && s.reason.is_none()
                && s.active_trigger.is_none()
                && s.launch_started_at_ms.is_none()
                && s.observed_at_ms >= s.armed_at_ms
                && s.next_due_ms
                    .is_some_and(|n| n > s.observed_at_ms && n < s.expires_at_ms)
        }
        State::InFlight => {
            s.stop.is_none()
                && s.reason.is_none()
                && s.active_trigger.is_some()
                && s.next_due_ms.is_none()
                && s.observed_at_ms >= s.armed_at_ms
                && s.claimed_runs > 0
        }
        State::Stopped => {
            s.stop.is_some() && s.next_due_ms.is_none() && s.reason.as_deref() == Some("owner_stop")
        }
        State::Interrupted => {
            s.stop.is_none()
                && s.next_due_ms.is_none()
                && matches!(
                    s.reason.as_deref(),
                    Some("restart" | "backward_clock" | "missed_trigger" | "policy_changed")
                        | Some(
                            "launch_failed"
                                | "source_present"
                                | "shutdown"
                                | "clock_discontinuity"
                                | "job_unresolved"
                                | "preflight_refused"
                        )
                )
        }
        State::Exhausted => {
            s.stop.is_none()
                && s.next_due_ms.is_none()
                && s.active_trigger.is_none()
                && s.launch_started_at_ms.is_none()
                && matches!(
                    s.reason.as_deref(),
                    Some("run_budget" | "time_budget" | "policy_budget")
                )
        }
        State::Resolved => {
            s.refusal_resolution.is_some()
                && s.refusal.is_some()
                && s.next_due_ms.is_none()
                && s.active_trigger.is_some()
                && s.reason.as_deref() == Some("refusal_resolved")
        }
    };
    if !shape {
        bail!("Schedule lifecycle is inconsistent; no catch-up or replay");
    }
    Ok(())
}
fn read_schedule(connection: &Connection, id: &str) -> Result<Option<Schedule>> {
    let raw: Option<(String, i64)> = connection
        .query_row(
            "SELECT record_json,reserved_bytes FROM automation_schedules WHERE schedule_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((raw, reserved)) = raw else {
        return Ok(None);
    };
    let s: Schedule = serde_json::from_str(&raw).context("Damaged/future schedule retained")?;
    validate_schedule(connection, &s)?;
    if s.id != id
        || raw.len() > SCHEDULE_COST
        || usize::try_from(reserved)? != SCHEDULE_COST - raw.len()
    {
        bail!("Schedule row identity/headroom differs");
    }
    Ok(Some(s))
}
fn schedules(connection: &Connection) -> Result<Vec<Schedule>> {
    let mut statement =
        connection.prepare("SELECT schedule_id FROM automation_schedules ORDER BY schedule_id")?;
    statement
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|id| read_schedule(connection, &id?)?.context("Schedule missing"))
        .collect()
}
fn read_trigger(connection: &Connection, id: &str) -> Result<Option<Trigger>> {
    let raw:Option<(String,u16,String,String,String)>=connection.query_row("SELECT schedule_id,ordinal,request_id,reservation_id,record_json FROM automation_triggers WHERE trigger_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((schedule_id, ordinal, request_id, reservation_id, raw)) = raw else {
        return Ok(None);
    };
    let t: Trigger = serde_json::from_str(&raw).context("Damaged/future trigger retained")?;
    let s = read_schedule(connection, &t.schedule_id)?.context("Trigger schedule missing")?;
    let p = policy(connection, &s)?;
    let identities = [&t.id, &t.schedule_id, &t.reservation_id, &t.execution_id];
    for id in identities {
        uuid(id)?;
    }
    uuid(t.request_context.request_id())?;
    let unique = identities
        .into_iter()
        .map(|id| id.as_str())
        .chain([t.request_context.request_id(), p.id.as_str()])
        .collect::<std::collections::BTreeSet<_>>();
    if t.schema_version != 1
        || t.id != id
        || t.schedule_id != schedule_id
        || t.ordinal != ordinal
        || t.reservation_id != reservation_id
        || t.request_context.request_id() != request_id
        || t.ordinal == 0
        || t.ordinal > s.claimed_runs
        || t.claimed_at_ms < t.due_at_ms
        || t.due_at_ms < s.armed_at_ms.saturating_add(s.every_ms)
        || t.claimed_at_ms >= s.expires_at_ms
        || t.claimed_at_ms.saturating_sub(t.due_at_ms) >= s.every_ms
        || t.request_context.origin_node() != p.scope.owner_context.origin_node()
        || t.request_context.origin_area() != p.scope.owner_context.origin_area()
        || t.request_context.request_id() == p.scope.owner_context.request_id()
        || unique.len() != 6
        || t.record_sha256 != digest(&t)?
        || serde_json::to_vec(&t)?.len() > 4096
    {
        bail!("Damaged trigger identity/time/context; no replacement");
    }
    Ok(Some(t))
}
fn triggers(connection: &Connection) -> Result<Vec<Trigger>> {
    let mut statement = connection
        .prepare("SELECT trigger_id FROM automation_triggers ORDER BY schedule_id,ordinal")?;
    statement
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|id| read_trigger(connection, &id?)?.context("Trigger missing"))
        .collect()
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    let claims = triggers(connection)?;
    let mut active_definitions = std::collections::BTreeSet::new();
    let mut identities = std::collections::BTreeSet::new();
    for t in &claims {
        for id in [&t.id, &t.reservation_id, &t.execution_id] {
            if !identities.insert(id.as_str()) {
                bail!("Trigger identity is reused across retained claims");
            }
        }
        if !identities.insert(t.request_context.request_id()) {
            bail!("Trigger request identity is reused");
        }
    }
    for s in schedules(connection)? {
        if matches!(s.state, State::Waiting | State::InFlight)
            && !active_definitions.insert(s.definition_id.clone())
        {
            bail!("Multiple active schedules own the same definition; no trigger overlap");
        }
        let own = claims
            .iter()
            .filter(|t| t.schedule_id == s.id)
            .collect::<Vec<_>>();
        if own.len() != s.claimed_runs as usize
            || own
                .iter()
                .enumerate()
                .any(|(i, t)| t.ordinal as usize != i + 1)
            || s.active_trigger
                .as_ref()
                .is_some_and(|id| own.last().is_none_or(|t| t.id != *id))
        {
            bail!("Trigger ordinal/count/ownership history differs; no budget reset");
        }
        if s.state == State::InFlight
            && own.last().is_none_or(|t| {
                s.observed_at_ms != s.launch_started_at_ms.unwrap_or(t.claimed_at_ms)
                    || s.launch_started_at_ms
                        .is_some_and(|at| at < t.claimed_at_ms || at >= s.expires_at_ms)
            })
        {
            bail!("In-flight schedule clock differs from its claim");
        }
        if let Some(refusal) = &s.refusal {
            let t = own.last().context("Refusal lacks actual durable claim")?;
            if refusal.trigger_id != t.id || refusal.trigger_sha256 != t.record_sha256 {
                bail!("Backend refusal differs from immutable claim");
            }
            let reserved: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM automation_occurrences WHERE reservation_id=?1)",
                [&t.reservation_id],
                |r| r.get(0),
            )?;
            let source:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM durable_jobs WHERE json_extract(plan_json,'$.goal.request_context.request_id')=?1)",[t.request_context.request_id()],|r|r.get(0))?;
            if reserved || source {
                bail!("Refused-before-launch evidence conflicts with retained work; no resolution");
            }
        }
    }
    Ok(())
}
fn current(
    connection: &Connection,
    s: &Schedule,
    runtime: &str,
    issuer: &str,
    now: u64,
) -> Result<bool> {
    let p = policy(connection, s)?;
    let d = automations::read_definition(connection, &s.definition_id)?;
    let latest=automation_authorizations::records(connection)?.iter().filter(|p|matches!(&p.creation,PolicyCommand::Approve{definition,..} if definition==&s.definition_id)).map(|p|p.generation).max().unwrap_or(0);
    Ok(s.runtime_id == runtime
        && s.issuer_id == issuer
        && p.current_at(runtime, issuer, latest, &d, now))
}
fn save_schedule(
    store: &JobStore,
    connection: &Connection,
    s: &mut Schedule,
    new: bool,
) -> Result<()> {
    s.record_sha256 = digest(s)?;
    validate_schedule(connection, s)?;
    let raw = serde_json::to_string(s)?;
    quota(
        store,
        connection,
        "automation_schedules",
        "schedule_id",
        &s.id,
        SCHEDULE_COST,
        new,
    )?;
    let reserved = i64::try_from(SCHEDULE_COST - raw.len())?;
    if new {
        connection.execute("INSERT INTO automation_schedules(schedule_id,record_json,reserved_bytes) VALUES(?1,?2,?3)",params![s.id,raw,reserved])?;
    } else {
        connection.execute(
            "UPDATE automation_schedules SET record_json=?2,reserved_bytes=?3 WHERE schedule_id=?1",
            params![s.id, raw, reserved],
        )?;
    }
    Ok(())
}
fn quota(
    store: &JobStore,
    connection: &Connection,
    table: &str,
    key: &str,
    id: &str,
    cost: usize,
    new: bool,
) -> Result<()> {
    if new && automations::count(connection)? >= store.max_count {
        bail!("Trigger history count quota exhausted");
    }
    // Only internal constant identifiers call this helper, never caller strings.
    let reserve = if table == "automation_schedules" {
        "+reserved_bytes"
    } else {
        ""
    };
    let previous: Option<i64> = connection
        .query_row(
            &format!(
                "SELECT length(CAST(record_json AS BLOB)){reserve} FROM {table} WHERE {key}=?1"
            ),
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if previous.is_some_and(|old| old >= cost as i64) {
        return Ok(());
    }
    let jobs: i64 = connection.query_row(
        "SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs",
        [],
        |r| r.get(0),
    )?;
    let used = usize::try_from(jobs)?
        .checked_add(automations::definition_bytes(connection)?)
        .context("Trigger storage overflow")?
        .checked_sub(usize::try_from(previous.unwrap_or(0))?)
        .context("Trigger storage underflow")?;
    if cost > store.max_bytes.saturating_sub(used) {
        bail!("Trigger byte quota exhausted; no claim advanced");
    }
    Ok(())
}
fn advance(s: &mut Schedule, now: u64) -> Result<()> {
    s.revision = s
        .revision
        .checked_add(1)
        .context("Schedule revision exhausted")?;
    s.observed_at_ms = now;
    Ok(())
}
fn interrupted(s: &mut Schedule, reason: &str, now: u64) -> Result<()> {
    advance(s, now)?;
    s.state = State::Interrupted;
    s.reason = Some(reason.into());
    s.next_due_ms = None;
    Ok(())
}
pub(super) fn interrupt_on_open(store: &JobStore) -> Result<()> {
    let connection = store.connection.lock().expect("job journal");
    let retained = schedules(&connection)?;
    let tx = connection.unchecked_transaction()?;
    for mut s in retained {
        if matches!(s.state, State::Waiting | State::InFlight) {
            interrupted(&mut s, "restart", eefn::protocol::now_ms())?;
            save_schedule(store, &tx, &mut s, false)?;
        }
    }
    validate_all(&tx)?;
    tx.commit()?;
    Ok(())
}
fn trigger_for_request(connection: &Connection, request: &str) -> Result<Option<Trigger>> {
    let id: Option<String> = connection
        .query_row(
            "SELECT trigger_id FROM automation_triggers WHERE request_id=?1",
            [request],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| read_trigger(connection, &id)?.context("Trigger missing"))
        .transpose()
}
pub(super) fn validate_delegation(connection: &Connection, plan: &Plan) -> Result<()> {
    let Some(context) = &plan.goal.request_context else {
        return Ok(());
    };
    let claim = trigger_for_request(connection, context.request_id())?;
    let delegation = plan.approval.as_ref().and_then(|a| a.delegation.as_ref());
    match (claim, delegation.and_then(|d| d.trigger.as_ref())) {
        (None, None) => Ok(()),
        (Some(t), Some(link)) => {
            let s = read_schedule(connection, &t.schedule_id)?.context("Schedule missing")?;
            let d = delegation.unwrap();
            if link.schedule_id != s.id
                || link.trigger_id != t.id
                || d.policy_id != s.creation.policy
                || d.reservation_id != t.reservation_id
                || d.runtime_id != s.runtime_id
                || *context != t.request_context
                || plan
                    .execution
                    .as_ref()
                    .is_some_and(|e| e.command.operation_id != t.execution_id)
            {
                bail!("Delegation differs from durable trigger lineage");
            }
            Ok(())
        }
        // A fresh unapproved source is permitted, but cannot use direct approval
        // or a different reservation/execution receipt to bypass its schedule.
        (Some(_), None) if plan.approval.is_none() => Ok(()),
        _ => bail!("Trigger-linked job cannot omit or import schedule authority"),
    }
}
pub(super) fn reservation_link(
    connection: &Connection,
    plan: &Plan,
    p: &Authorization,
    reservation: &str,
    now: u64,
) -> Result<Option<TriggerLink>> {
    let Some(context) = &plan.goal.request_context else {
        return Ok(None);
    };
    let Some(t) = trigger_for_request(connection, context.request_id())? else {
        return Ok(None);
    };
    let s = read_schedule(connection, &t.schedule_id)?.context("Schedule missing")?;
    if t.request_context != *context
        || t.reservation_id != reservation
        || s.creation.policy != p.id
        || s.state != State::InFlight
        || s.active_trigger.as_deref() != Some(&t.id)
        || now < s.observed_at_ms
        || !current(connection, &s, &p.runtime_id, &p.issuer_id, now)?
    {
        bail!("Durable trigger stopped/interrupted/stale; no new reservation");
    }
    Ok(Some(TriggerLink {
        schedule_id: s.id,
        trigger_id: t.id,
    }))
}
pub(super) fn fence(connection: &Connection, plan: &Plan, runtime: &str, now: u64) -> Result<()> {
    validate_delegation(connection, plan)?;
    let Some(link) = plan
        .approval
        .as_ref()
        .and_then(|a| a.delegation.as_ref())
        .and_then(|d| d.trigger.as_ref())
    else {
        return Ok(());
    };
    let s = read_schedule(connection, &link.schedule_id)?.context("Schedule missing")?;
    if s.state != State::InFlight
        || s.active_trigger.as_deref() != Some(&link.trigger_id)
        || s.runtime_id != runtime
        || now < s.observed_at_ms
        || now >= s.expires_at_ms
    {
        bail!("Durable trigger stopped/interrupted/expired; no new issuance");
    }
    Ok(())
}
pub(super) fn refuse_removal(connection: &Connection, plan: &Plan) -> Result<()> {
    if let Some(context) = &plan.goal.request_context {
        if trigger_for_request(connection, context.request_id())?.is_some() {
            bail!(
                "Trigger lineage must be retained; use exact occurrence archiving after resolution"
            );
        }
    }
    Ok(())
}
/// A review/compile checkpoint is not actuation, but must not manufacture more
/// source work for a claim after its schedule stopped or changed authority.
pub(super) fn source_authority(
    connection: &Connection,
    context: &eefn::context::RequestContext,
) -> Result<()> {
    let Some(t) = trigger_for_request(connection, context.request_id())? else {
        return Ok(());
    };
    let s = read_schedule(connection, &t.schedule_id)?.context("Schedule missing")?;
    if *context != t.request_context
        || s.state != State::InFlight
        || s.active_trigger.as_deref() != Some(&t.id)
        || !current(
            connection,
            &s,
            &s.runtime_id,
            &s.issuer_id,
            eefn::protocol::now_ms(),
        )?
    {
        bail!("Durable trigger source admission/compilation stopped or changed authority");
    }
    Ok(())
}

// Journal entry points used by the guarded owner service and bounded timer.
#[allow(dead_code)]
impl JobStore {
    /// Called only by the bridge after its pre-launch path actually refused.
    /// Absence alone is not a caller-supplied recovery assertion.
    pub(crate) fn trigger_record_refusal(&self, id: &str, runtime: &str) -> Result<bool> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let t = read_trigger(&connection, id)?.context("Trigger missing")?;
        let mut s = read_schedule(&connection, &t.schedule_id)?.context("Schedule missing")?;
        if s.refusal.is_some() {
            return Ok(true);
        }
        if s.runtime_id != runtime
            || s.state != State::InFlight
            || s.active_trigger.as_deref() != Some(id)
            || s.launch_started_at_ms.is_some()
            || source_job(&connection, &t)?.is_some()
        {
            return Ok(false);
        }
        let now = eefn::protocol::now_ms();
        let mut refusal = Refusal {
            schema_version: 1,
            trigger_id: t.id,
            trigger_sha256: t.record_sha256,
            runtime_id: runtime.into(),
            observed_at_ms: now,
            phase: "refused_before_launch".into(),
            refusal_sha256: String::new(),
        };
        let mut raw = json!(refusal);
        raw.as_object_mut().unwrap().remove("refusal_sha256");
        refusal.refusal_sha256 = eefn::grants::definition_digest(&raw)?;
        interrupted(&mut s, "preflight_refused", now)?;
        s.refusal = Some(refusal);
        let tx = connection.unchecked_transaction()?;
        save_schedule(self, &tx, &mut s, false)?;
        validate_all(&tx)?;
        tx.commit()?;
        Ok(true)
    }
    pub(crate) fn resolve_trigger_refusal(
        &self,
        id: &str,
        revision: u64,
        trigger: &str,
        hash: &str,
        operation: &str,
    ) -> Result<(Schedule, bool)> {
        uuid(operation)?;
        uuid(trigger)?;
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let mut s = read_schedule(&connection, id)?.context("Schedule missing")?;
        if let Some(old) = &s.refusal_resolution {
            if old.source_revision != revision
                || old.trigger_id != trigger
                || old.refusal_sha256 != hash
                || old.operation_id != operation
            {
                bail!("Refusal-resolution receipt conflicts; inspect original state");
            }
            return Ok((s, true));
        }
        let refusal = s
            .refusal
            .as_ref()
            .context("No backend refusal proof; missing source is not no-work evidence")?;
        if s.revision != revision
            || !matches!(s.state, State::Interrupted | State::Stopped)
            || refusal.trigger_id != trigger
            || refusal.refusal_sha256 != hash
            || operation == s.id
            || operation == s.creation.policy
            || triggers(&connection)?.iter().any(|t| {
                [
                    t.id.as_str(),
                    t.reservation_id.as_str(),
                    t.execution_id.as_str(),
                    t.request_context.request_id(),
                ]
                .contains(&operation)
            })
        {
            bail!("Exact current refusal revision/hash/receipt required; no forced resolution");
        }
        let now = eefn::protocol::now_ms();
        advance(&mut s, now)?;
        s.state = State::Resolved;
        s.next_due_ms = None;
        s.reason = Some("refusal_resolved".into());
        s.refusal_resolution = Some(RefusalResolution {
            operation_id: operation.into(),
            source_revision: revision,
            trigger_id: trigger.into(),
            refusal_sha256: hash.into(),
            resolved_at_ms: now,
        });
        let tx = connection.unchecked_transaction()?;
        save_schedule(self, &tx, &mut s, false)?;
        validate_all(&tx)?;
        tx.commit()?;
        Ok((s, false))
    }
    pub(crate) fn schedule_list(&self) -> Result<Vec<Schedule>> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        schedules(&connection)
    }
    pub(crate) fn schedule_get(&self, id: &str) -> Result<Schedule> {
        uuid(id)?;
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        read_schedule(&connection, id)?.context("Schedule missing")
    }
    pub(crate) fn trigger_get(&self, id: &str) -> Result<Trigger> {
        uuid(id)?;
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        read_trigger(&connection, id)?.context("Trigger missing")
    }
    pub(crate) fn trigger_job(&self, id: &str) -> Result<Option<Plan>> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let t = read_trigger(&connection, id)?.context("Trigger missing")?;
        source_job(&connection, &t)
    }
    pub(crate) fn trigger_begin_launch(&self, id: &str, runtime: &str) -> Result<bool> {
        self.trigger_begin_launch_clock(id, runtime, eefn::protocol::now_ms)
    }
    fn trigger_begin_launch_clock(
        &self,
        id: &str,
        runtime: &str,
        clock: impl Fn() -> u64,
    ) -> Result<bool> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let t = read_trigger(&connection, id)?.context("Trigger missing")?;
        let mut s = read_schedule(&connection, &t.schedule_id)?.context("Schedule missing")?;
        if s.runtime_id != runtime
            || s.state != State::InFlight
            || s.active_trigger.as_deref() != Some(id)
        {
            bail!("Trigger no longer owns admission authority");
        }
        if s.launch_started_at_ms.is_some() {
            return Ok(false);
        }
        if source_job(&connection, &t)?.is_some() {
            bail!("Source already exists; no implicit continuation");
        }
        let now = clock();
        if now < s.observed_at_ms || !current(&connection, &s, runtime, &s.issuer_id, now)? {
            bail!("Trigger clock/policy changed before launch");
        }
        advance(&mut s, now)?;
        s.launch_started_at_ms = Some(now);
        let tx = connection.unchecked_transaction()?;
        save_schedule(self, &tx, &mut s, false)?;
        let commit = clock();
        if commit < now || !current(&tx, &s, runtime, &s.issuer_id, commit)? {
            bail!("Trigger clock/policy changed before launch commit");
        }
        tx.commit()?;
        Ok(true)
    }
    pub(crate) fn schedule_interrupt_now(&self, id: &str, reason: &str) -> Result<Schedule> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let mut s = read_schedule(&connection, id)?.context("Schedule missing")?;
        if matches!(s.state, State::Waiting | State::InFlight) {
            interrupted(&mut s, reason, eefn::protocol::now_ms())?;
            save_schedule(self, &connection, &mut s, false)?;
        }
        Ok(s)
    }
    pub(crate) fn trigger_interrupt_now(&self, id: &str, reason: &str) -> Result<Schedule> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let t = read_trigger(&connection, id)?.context("Trigger missing")?;
        let mut s = read_schedule(&connection, &t.schedule_id)?.context("Schedule missing")?;
        if s.state == State::InFlight && s.active_trigger.as_deref() == Some(id) {
            interrupted(&mut s, reason, eefn::protocol::now_ms())?;
            save_schedule(self, &connection, &mut s, false)?;
        }
        Ok(s)
    }
    pub(crate) fn schedule_arm_now(
        &self,
        command: Arm,
        runtime: &str,
        issuer: &str,
    ) -> Result<(Schedule, bool)> {
        self.schedule_arm(command, runtime, issuer, eefn::protocol::now_ms)
    }
    pub(crate) fn schedule_claim_due_now(
        &self,
        id: &str,
        runtime: &str,
        issuer: &str,
    ) -> Result<Option<Trigger>> {
        self.schedule_claim_due(id, runtime, issuer, eefn::protocol::now_ms)
    }
    pub(crate) fn schedule_stop_now(
        &self,
        id: &str,
        revision: u64,
        operation: &str,
    ) -> Result<(Schedule, bool)> {
        self.schedule_stop(id, revision, operation, eefn::protocol::now_ms())
    }
    pub(crate) fn schedule_settle_now(
        &self,
        id: &str,
        runtime: &str,
        issuer: &str,
    ) -> Result<Schedule> {
        self.schedule_settle(id, runtime, issuer, eefn::protocol::now_ms())
    }
    fn schedule_arm(
        &self,
        command: Arm,
        runtime: &str,
        issuer: &str,
        clock: impl Fn() -> u64,
    ) -> Result<(Schedule, bool)> {
        uuid(&command.operation_id)?;
        uuid(&command.policy)?;
        uuid(runtime)?;
        if command.operation_id == command.policy
            || command.policy_sha256.len() != 64
            || !command.policy_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("Invalid exact schedule arm");
        }
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        if let Some(s) = read_schedule(&connection, &command.operation_id)? {
            if s.creation != command {
                bail!("Schedule receipt conflicts");
            }
            return Ok((s, true));
        }
        let p = automation_authorizations::read(&connection, &command.policy)?
            .context("Policy missing")?;
        let PolicyCommand::Approve { definition, .. } = &p.creation else {
            unreachable!()
        };
        let source = automations::read_definition(&connection, definition)?;
        let eefn::automation::AutomationCommand::Define { every_ms, .. } = source.creation else {
            unreachable!()
        };
        let now = clock();
        let due = now.checked_add(every_ms).context("Trigger time overflow")?;
        let mut s = Schedule {
            schema_version: 1,
            id: command.operation_id.clone(),
            creation: command,
            definition_id: definition.clone(),
            runtime_id: runtime.into(),
            issuer_id: issuer.into(),
            generation: p.generation,
            armed_at_ms: now,
            expires_at_ms: p.expires_at_ms,
            every_ms,
            revision: 1,
            state: State::Waiting,
            observed_at_ms: now,
            next_due_ms: Some(due),
            active_trigger: None,
            launch_started_at_ms: None,
            refusal: None,
            refusal_resolution: None,
            claimed_runs: 0,
            stop: None,
            reason: None,
            record_sha256: String::new(),
        };
        if s.creation.policy_sha256 != p.authorization_sha256
            || !current(&connection, &s, runtime, issuer, now)?
            || p.scope.run_allowance(&p.creation.limits().unwrap())?.0
                > p.expires_at_ms.saturating_sub(due)
        {
            bail!("Current policy/time budget cannot arm schedule");
        }
        for old in schedules(&connection)?
            .iter()
            .filter(|old| old.definition_id == *definition)
        {
            if matches!(old.state, State::Waiting | State::InFlight)
                || triggers(&connection)?
                    .iter()
                    .filter(|t| t.schedule_id == old.id)
                    .any(|t| !resolved_claim(&connection, t).unwrap_or(false))
            {
                bail!("Previous schedule/claim is active or unresolved; no overlap");
            }
        }
        if !automation_occurrences::definition_resolved(&connection, definition)? {
            bail!("Previous occurrence is unresolved; no schedule replacement");
        }
        if !automation_occurrences::allowance_available(&connection, &p)? {
            bail!("Policy occurrence budget exhausted; no schedule armed");
        }
        let tx = connection.unchecked_transaction()?;
        save_schedule(self, &tx, &mut s, true)?;
        let commit = clock();
        if commit < now
            || !current(&tx, &s, runtime, issuer, commit)?
            || p.scope.run_allowance(&p.creation.limits().unwrap())?.0
                > p.expires_at_ms.saturating_sub(due.max(commit))
        {
            bail!("Schedule clock/policy changed before commit");
        }
        tx.commit()?;
        Ok((s, false))
    }
    fn schedule_claim_due(
        &self,
        id: &str,
        runtime: &str,
        issuer: &str,
        clock: impl Fn() -> u64,
    ) -> Result<Option<Trigger>> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let mut s = read_schedule(&connection, id)?.context("Schedule missing")?;
        if s.state != State::Waiting {
            return Ok(None);
        };
        let now = clock();
        let due = s.next_due_ms.unwrap();
        let reason = if now < s.observed_at_ms {
            Some("backward_clock")
        } else if !current(&connection, &s, runtime, issuer, now)? {
            Some("policy_changed")
        } else if now >= due.saturating_add(s.every_ms) {
            Some("missed_trigger")
        } else {
            None
        };
        if let Some(reason) = reason {
            interrupted(&mut s, reason, now)?;
            save_schedule(self, &connection, &mut s, false)?;
            return Ok(None);
        };
        if now < due {
            return Ok(None);
        };
        if !automation_occurrences::definition_resolved(&connection, &s.definition_id)? {
            bail!("Previous occurrence is unfinished/uncertain; no trigger overlap");
        }
        let p = policy(&connection, &s)?;
        let PolicyCommand::Approve { max_runs, .. } = p.creation else {
            unreachable!()
        };
        let budget_available = automation_occurrences::allowance_available(&connection, &p)?;
        if s.claimed_runs >= max_runs
            || !budget_available
            || p.scope.run_allowance(&p.creation.limits().unwrap())?.0
                > s.expires_at_ms.saturating_sub(now)
        {
            advance(&mut s, now)?;
            s.state = State::Exhausted;
            s.reason = Some(
                if s.claimed_runs >= max_runs {
                    "run_budget"
                } else if !budget_available {
                    "policy_budget"
                } else {
                    "time_budget"
                }
                .into(),
            );
            s.next_due_ms = None;
            save_schedule(self, &connection, &mut s, false)?;
            return Ok(None);
        }
        let mut t = Trigger {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            schedule_id: s.id.clone(),
            ordinal: s.claimed_runs + 1,
            due_at_ms: due,
            claimed_at_ms: now,
            request_context: eefn::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                p.scope.owner_context.origin_node().into(),
                p.scope.owner_context.origin_area().to_vec(),
            )?,
            reservation_id: uuid::Uuid::new_v4().to_string(),
            execution_id: uuid::Uuid::new_v4().to_string(),
            record_sha256: String::new(),
        };
        t.record_sha256 = digest(&t)?;
        advance(&mut s, now)?;
        s.claimed_runs = t.ordinal;
        s.state = State::InFlight;
        s.next_due_ms = None;
        s.active_trigger = Some(t.id.clone());
        s.launch_started_at_ms = None;
        let tx = connection.unchecked_transaction()?;
        save_schedule(self, &tx, &mut s, false)?;
        let raw = serde_json::to_string(&t)?;
        quota(
            self,
            &tx,
            "automation_triggers",
            "trigger_id",
            &t.id,
            raw.len(),
            true,
        )?;
        tx.execute("INSERT INTO automation_triggers(trigger_id,schedule_id,ordinal,request_id,reservation_id,record_json) VALUES(?1,?2,?3,?4,?5,?6)",params![t.id,t.schedule_id,t.ordinal,t.request_context.request_id(),t.reservation_id,raw])?;
        validate_all(&tx)?;
        let commit = clock();
        if commit < now
            || commit >= due.saturating_add(s.every_ms)
            || !current(&tx, &s, runtime, issuer, commit)?
            || p.scope.run_allowance(&p.creation.limits().unwrap())?.0
                > s.expires_at_ms.saturating_sub(commit)
        {
            bail!("Trigger clock/policy changed before commit; no claim saved");
        }
        tx.commit()?;
        Ok(Some(t))
    }
    fn schedule_stop(
        &self,
        id: &str,
        revision: u64,
        operation: &str,
        now: u64,
    ) -> Result<(Schedule, bool)> {
        uuid(operation)?;
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let mut s = read_schedule(&connection, id)?.context("Schedule missing")?;
        let stop = Stop {
            operation_id: operation.into(),
            source_revision: revision,
        };
        if let Some(old) = &s.stop {
            if old != &stop {
                bail!("Stop receipt conflicts");
            }
            return Ok((s, true));
        }
        if revision != s.revision || operation == id || operation == s.creation.policy {
            bail!("Exact current stop revision/receipt required");
        }
        if triggers(&connection)?.iter().any(|t| {
            [
                t.id.as_str(),
                t.reservation_id.as_str(),
                t.execution_id.as_str(),
                t.request_context.request_id(),
            ]
            .contains(&operation)
        }) {
            bail!("Stop receipt conflicts with retained trigger identity");
        }
        advance(&mut s, now)?;
        s.state = State::Stopped;
        s.stop = Some(stop);
        s.next_due_ms = None;
        s.reason = Some("owner_stop".into());
        save_schedule(self, &connection, &mut s, false)?;
        Ok((s, false))
    }
    fn schedule_settle(&self, id: &str, runtime: &str, issuer: &str, now: u64) -> Result<Schedule> {
        let connection = self.connection.lock().expect("job journal");
        validate_all(&connection)?;
        let mut s = read_schedule(&connection, id)?.context("Schedule missing")?;
        if s.state != State::InFlight {
            bail!("Only an in-flight claim can settle; no automatic restart");
        }
        let t = read_trigger(&connection, s.active_trigger.as_deref().unwrap())?
            .context("Trigger missing")?;
        if !resolved_claim(&connection, &t)? {
            bail!("Trigger admission/completion is unresolved; no replacement");
        }
        if now < s.observed_at_ms {
            interrupted(&mut s, "backward_clock", now)?;
        } else if !current(&connection, &s, runtime, issuer, now)? {
            interrupted(&mut s, "policy_changed", now)?;
        } else {
            let p = policy(&connection, &s)?;
            let PolicyCommand::Approve { max_runs, .. } = p.creation else {
                unreachable!()
            };
            let next = now
                .checked_add(s.every_ms)
                .context("Trigger time overflow")?;
            advance(&mut s, now)?;
            s.active_trigger = None;
            s.launch_started_at_ms = None;
            let budget_available = automation_occurrences::allowance_available(&connection, &p)?;
            if s.claimed_runs >= max_runs
                || !budget_available
                || p.scope.run_allowance(&p.creation.limits().unwrap())?.0
                    > s.expires_at_ms.saturating_sub(next)
            {
                s.state = State::Exhausted;
                s.next_due_ms = None;
                s.reason = Some(
                    if s.claimed_runs >= max_runs {
                        "run_budget"
                    } else if !budget_available {
                        "policy_budget"
                    } else {
                        "time_budget"
                    }
                    .into(),
                );
            } else {
                s.state = State::Waiting;
                s.next_due_ms = Some(next);
            }
        }
        save_schedule(self, &connection, &mut s, false)?;
        Ok(s)
    }
}
fn resolved_claim(connection: &Connection, t: &Trigger) -> Result<bool> {
    let s = read_schedule(connection, &t.schedule_id)?.context("Schedule missing")?;
    if s.refusal_resolution
        .as_ref()
        .is_some_and(|r| r.trigger_id == t.id)
    {
        // Check the complete retained refusal/claim consistency before using it.
        validate_all(connection)?;
        return Ok(true);
    }
    if let Some(record) = automation_occurrences::row(connection, &t.reservation_id)? {
        let plan = automation_occurrences::archives::retained_plan(connection, &record)?;
        validate_delegation(connection, &plan)?;
        return Ok(automation_occurrences::archives::resolved(&plan));
    }
    // Only an actual retained cancellation with no approval/execution resolves
    // partial admission. Absence of a reservation/job is never no-work proof.
    Ok(source_job(connection, t)?.is_some_and(|p| {
        p.approval.is_none()
            && p.execution.is_none()
            && automation_occurrences::archives::resolved(&p)
    }))
}
fn source_job(connection: &Connection, t: &Trigger) -> Result<Option<Plan>> {
    if let Some(record) = automation_occurrences::row(connection, &t.reservation_id)? {
        let p = automation_occurrences::archives::retained_plan(connection, &record)?;
        validate_delegation(connection, &p)?;
        return Ok(Some(p));
    }
    let mut statement=connection.prepare("SELECT job_id FROM durable_jobs WHERE json_extract(plan_json,'$.goal.request_context.request_id')=?1")?;
    let ids = statement
        .query_map([t.request_context.request_id()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut found = None;
    for id in ids {
        let plan = read(connection, &id)?;
        if plan.goal.request_context.as_ref() == Some(&t.request_context) {
            if found.is_some() {
                bail!("Partial trigger admission is ambiguous; no replacement");
            }
            found = Some(plan);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskEngine;
    use eefn::automation_occurrence::OccurrenceCommand;
    fn fixture(store: Arc<JobStore>) -> (Arc<TaskEngine>, Plan, Authorization, Schedule) {
        let (engine, source, _, p) =
            super::super::automation_authorizations::tests::fixture(store.clone());
        store
            .automation_policy_checkpoint(
                p.creation.clone(),
                Some(p.clone()),
                &p.runtime_id,
                p.approved_at_ms,
            )
            .unwrap();
        let s = store
            .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || {
                p.approved_at_ms + 1
            })
            .unwrap()
            .0;
        (engine, source, p, s)
    }
    fn arm(p: &Authorization) -> Arm {
        Arm {
            policy: p.id.clone(),
            policy_sha256: p.authorization_sha256.clone(),
            operation_id: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn opening(engine: &TaskEngine, source: &Plan, t: &Trigger) -> Result<serde_json::Value> {
        crate::request_drafts::open(
            engine,
            json!({"schema_version":1,"original_text":" Inspect status ",
            "proposal":source.request_review.as_ref().unwrap().proposals[0].proposal}),
            &t.request_context,
            &["system.info".into()],
        )
    }
    #[test]
    fn backend_refusal_resolution_is_explicit_deduplicated_and_never_replays_or_refunds() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        assert!(store.trigger_record_refusal(&t.id, &p.runtime_id).unwrap());
        let refused = store.schedule_get(&s.id).unwrap();
        assert_eq!(refused.state, State::Interrupted);
        assert_eq!(refused.reason.as_deref(), Some("preflight_refused"));
        let evidence = refused.refusal.as_ref().unwrap();
        let op = uuid::Uuid::new_v4().to_string();
        let barrier = std::sync::Barrier::new(16);
        let results = std::thread::scope(|scope| {
            (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store
                            .resolve_trigger_refusal(
                                &s.id,
                                refused.revision,
                                &t.id,
                                &evidence.refusal_sha256,
                                &op,
                            )
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|(_, reused)| !*reused).count(), 1);
        for (record, _) in &results {
            assert_eq!(record, &results[0].0);
        }
        assert_eq!(results[0].0.state, State::Resolved);
        assert_eq!(results[0].0.claimed_runs, 1);
        assert_eq!(results[0].0.refusal, refused.refusal);
        assert!(store.trigger_job(&t.id).unwrap().is_none());
        assert_eq!(store.usage().unwrap()["trigger_count"], 1);
        assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
        assert!(
            store
                .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || t.claimed_at_ms + 2)
                .unwrap()
                .is_none()
        );
        let fresh = store
            .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || t.claimed_at_ms + 2)
            .unwrap();
        assert!(!fresh.1);
        assert_eq!(fresh.0.claimed_runs, 0);
    }
    #[test]
    fn marked_missing_source_or_actual_partial_admission_cannot_be_called_refused_no_work() {
        for marked in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            if marked {
                store
                    .trigger_begin_launch_clock(&t.id, &p.runtime_id, || t.claimed_at_ms + 1)
                    .unwrap();
            } else {
                opening(&engine, &source, &t).unwrap();
            }
            assert!(!store.trigger_record_refusal(&t.id, &p.runtime_id).unwrap());
            let current = store.schedule_get(&s.id).unwrap();
            assert!(current.refusal.is_none());
            assert!(
                store
                    .resolve_trigger_refusal(
                        &s.id,
                        current.revision,
                        &t.id,
                        &"a".repeat(64),
                        &uuid::Uuid::new_v4().to_string()
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("missing source")
            );
        }
    }
    #[test]
    fn refusal_resolution_write_fault_or_stale_binding_keeps_all_claim_and_proof_state() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        store.trigger_record_refusal(&t.id, &p.runtime_id).unwrap();
        let before = store.schedule_get(&s.id).unwrap();
        let hash = before.refusal.as_ref().unwrap().refusal_sha256.clone();
        let op = uuid::Uuid::new_v4().to_string();
        let wrong_hash = "b".repeat(64);
        for field in ["revision", "hash", "trigger"] {
            assert!(
                store
                    .resolve_trigger_refusal(
                        &s.id,
                        if field == "revision" {
                            before.revision + 1
                        } else {
                            before.revision
                        },
                        if field == "trigger" { &s.id } else { &t.id },
                        if field == "hash" { &wrong_hash } else { &hash },
                        &op
                    )
                    .is_err()
            );
        }
        store.connection.lock().unwrap().execute_batch("CREATE TRIGGER refusal_fault BEFORE UPDATE ON automation_schedules BEGIN SELECT RAISE(FAIL,'isolated refusal resolution write fault'); END;").unwrap();
        assert!(
            store
                .resolve_trigger_refusal(&s.id, before.revision, &t.id, &hash, &op)
                .is_err()
        );
        assert_eq!(store.schedule_get(&s.id).unwrap(), before);
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER refusal_fault")
            .unwrap();
        store
            .resolve_trigger_refusal(&s.id, before.revision, &t.id, &hash, &op)
            .unwrap();
        assert!(
            store
                .resolve_trigger_refusal(&s.id, before.revision + 1, &t.id, &hash, &op)
                .is_err()
        );
    }
    #[test]
    fn refusal_and_resolution_survive_restart_lowered_quota_and_retained_owner_stop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 30, 1024 * 1024).unwrap();
        let (engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        store.trigger_record_refusal(&t.id, &p.runtime_id).unwrap();
        let refused = store.schedule_get(&s.id).unwrap();
        let stop = uuid::Uuid::new_v4().to_string();
        let stopped = store
            .schedule_stop(&s.id, refused.revision, &stop, eefn::protocol::now_ms())
            .unwrap()
            .0;
        let op = uuid::Uuid::new_v4().to_string();
        let hash = stopped.refusal.as_ref().unwrap().refusal_sha256.clone();
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        let resolved = store
            .resolve_trigger_refusal(&s.id, stopped.revision, &t.id, &hash, &op)
            .unwrap();
        assert!(!resolved.1);
        assert_eq!(resolved.0.stop, stopped.stop);
        assert!(
            store
                .schedule_stop(&s.id, refused.revision, &stop, p.expires_at_ms + 1)
                .unwrap()
                .1
        );
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        assert_eq!(
            store
                .resolve_trigger_refusal(&s.id, stopped.revision, &t.id, &hash, &op)
                .unwrap(),
            (resolved.0, true)
        );
    }
    #[test]
    fn corrupt_refusal_claim_binding_or_conflicting_source_fails_closed_without_repair() {
        for fault in ["claim", "source", "future"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            Connection::open(&path).unwrap();
            let store = JobStore::open(&path, 30, 1024 * 1024).unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            store.trigger_record_refusal(&t.id, &p.runtime_id).unwrap();
            {
                let connection = store.connection.lock().unwrap();
                if fault == "source" {
                    let mut plan = source.clone();
                    plan.goal.request_context = Some(t.request_context.clone());
                    connection
                        .execute(
                            "UPDATE durable_jobs SET plan_json=?1 WHERE job_id=?2",
                            params![serde_json::to_string(&plan).unwrap(), source.plan_id],
                        )
                        .unwrap();
                } else {
                    let mut row = read_schedule(&connection, &s.id).unwrap().unwrap();
                    let refusal = row.refusal.as_mut().unwrap();
                    if fault == "claim" {
                        refusal.trigger_sha256 = "a".repeat(64);
                    } else {
                        refusal.schema_version = 2;
                    }
                    let mut raw = json!(refusal);
                    raw.as_object_mut().unwrap().remove("refusal_sha256");
                    refusal.refusal_sha256 = eefn::grants::definition_digest(&raw).unwrap();
                    row.record_sha256 = digest(&row).unwrap();
                    let raw = serde_json::to_string(&row).unwrap();
                    connection.execute("UPDATE automation_schedules SET record_json=?2,reserved_bytes=?3 WHERE schedule_id=?1",params![row.id,raw,(SCHEDULE_COST-raw.len())as i64]).unwrap();
                }
            }
            drop(engine);
            drop(store);
            assert!(JobStore::open(&path, 30, 1024 * 1024).is_err(), "{fault}");
        }
    }
    #[test]
    fn launch_marker_is_one_shot_under_concurrency_and_never_creates_source_work() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        let barrier = std::sync::Barrier::new(16);
        let results = std::thread::scope(|scope| {
            (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store
                            .trigger_begin_launch_clock(&t.id, &p.runtime_id, || {
                                t.claimed_at_ms + 1
                            })
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|started| **started).count(), 1);
        let saved = store.schedule_get(&s.id).unwrap();
        assert_eq!(saved.revision, 3);
        assert_eq!(saved.launch_started_at_ms, Some(t.claimed_at_ms + 1));
        assert!(store.trigger_job(&t.id).unwrap().is_none());
        assert_eq!(store.usage().unwrap()["count"], 1);
        assert!(
            !store
                .trigger_begin_launch_clock(&t.id, &p.runtime_id, || p.expires_at_ms + 1)
                .unwrap()
        );
        assert_eq!(store.schedule_get(&s.id).unwrap(), saved);
    }
    #[test]
    fn launch_marker_write_or_clock_failure_rolls_back_without_source_or_continuation() {
        for fault in ["write", "backward", "expired"] {
            let store = JobStore::in_memory().unwrap();
            let (_engine, _source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            let before = store.schedule_get(&s.id).unwrap();
            if fault == "write" {
                store.connection.lock().unwrap().execute_batch("CREATE TRIGGER launch_fault BEFORE UPDATE ON automation_schedules BEGIN SELECT RAISE(FAIL,'isolated launch checkpoint fault'); END;").unwrap();
            }
            let calls = std::cell::Cell::new(0u8);
            assert!(
                store
                    .trigger_begin_launch_clock(&t.id, &p.runtime_id, || {
                        let n = calls.get();
                        calls.set(n + 1);
                        if n > 0 && fault == "backward" {
                            t.claimed_at_ms
                        } else if n > 0 && fault == "expired" {
                            p.expires_at_ms
                        } else {
                            t.claimed_at_ms + 1
                        }
                    })
                    .is_err()
            );
            assert_eq!(store.schedule_get(&s.id).unwrap(), before);
            assert!(store.trigger_job(&t.id).unwrap().is_none());
        }
    }
    #[test]
    fn preexisting_source_and_stopped_claim_never_receive_a_launch_marker() {
        for stopped in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            if stopped {
                store
                    .schedule_stop(
                        &s.id,
                        2,
                        &uuid::Uuid::new_v4().to_string(),
                        t.claimed_at_ms + 1,
                    )
                    .unwrap();
            } else {
                opening(&engine, &source, &t).unwrap();
            }
            assert!(
                store
                    .trigger_begin_launch_clock(&t.id, &p.runtime_id, || t.claimed_at_ms + 2)
                    .is_err()
            );
            assert!(
                store
                    .schedule_get(&s.id)
                    .unwrap()
                    .launch_started_at_ms
                    .is_none()
            );
            assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
        }
    }
    #[test]
    fn late_bridge_failure_cannot_interrupt_a_successor_claim_or_owner_stop() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let first = claim(&store, &p, &s);
        let job = fresh(&engine, &source, &first);
        engine.cancel(&job.plan_id).unwrap();
        let waiting = store
            .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, first.claimed_at_ms + 2)
            .unwrap();
        assert_eq!(
            store
                .trigger_interrupt_now(&first.id, "launch_failed")
                .unwrap(),
            waiting
        );
        let second = claim(&store, &p, &waiting);
        let inflight = store.schedule_get(&s.id).unwrap();
        assert_eq!(
            store
                .trigger_interrupt_now(&first.id, "launch_failed")
                .unwrap(),
            inflight
        );
        assert_eq!(inflight.active_trigger.as_deref(), Some(second.id.as_str()));
        let stopped = store
            .schedule_stop(
                &s.id,
                inflight.revision,
                &uuid::Uuid::new_v4().to_string(),
                second.claimed_at_ms + 1,
            )
            .unwrap()
            .0;
        assert_eq!(
            store
                .trigger_interrupt_now(&second.id, "launch_failed")
                .unwrap(),
            stopped
        );
    }
    #[test]
    fn marked_unadmitted_launch_survives_restart_as_unknown_not_ready_to_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        Connection::open(&path).unwrap();
        let store = JobStore::open(&path, 30, 1024 * 1024).unwrap();
        let (engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        store
            .trigger_begin_launch_clock(&t.id, &p.runtime_id, || t.claimed_at_ms + 1)
            .unwrap();
        drop(engine);
        drop(store);
        let store = JobStore::open(&path, 1, 1024).unwrap();
        let saved = store.schedule_get(&s.id).unwrap();
        assert_eq!(saved.state, State::Interrupted);
        assert_eq!(saved.launch_started_at_ms, Some(t.claimed_at_ms + 1));
        assert!(store.trigger_job(&t.id).unwrap().is_none());
        assert!(
            store
                .trigger_begin_launch_clock(&t.id, &p.runtime_id, || t.claimed_at_ms + 2)
                .is_err()
        );
        assert!(
            store
                .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 2)
                .is_err()
        );
    }
    #[test]
    fn stopped_claim_cannot_admit_a_late_source_but_receipt_inspection_survives() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        store
            .schedule_stop(
                &s.id,
                2,
                &uuid::Uuid::new_v4().to_string(),
                t.claimed_at_ms + 1,
            )
            .unwrap();
        assert!(
            opening(&engine, &source, &t)
                .unwrap_err()
                .to_string()
                .contains("source admission/compilation")
        );
        assert_eq!(store.usage().unwrap()["count"], 1);
        assert!(
            store
                .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || t.claimed_at_ms + 2)
                .is_err()
        );
    }
    #[test]
    fn stop_after_review_fences_compilation_without_rewriting_partial_history() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        let review = opening(&engine, &source, &t).unwrap();
        let id = review["id"].as_str().unwrap();
        let saved = store.get(id).unwrap();
        let compiled = crate::request_planning::tests::compiled(&saved);
        store
            .schedule_stop(
                &s.id,
                2,
                &uuid::Uuid::new_v4().to_string(),
                t.claimed_at_ms + 1,
            )
            .unwrap();
        assert!(
            engine
                .edit_review_plan(id, |plan| {
                    plan.goal = compiled.goal;
                    plan.tasks = compiled.tasks;
                    plan.planning = compiled.planning;
                    plan.status = PlanStatus::AwaitingApproval;
                    Ok(true)
                })
                .unwrap_err()
                .to_string()
                .contains("source admission/compilation")
        );
        assert_eq!(json!(store.get(id).unwrap()), json!(saved));
        assert_eq!(opening(&engine, &source, &t).unwrap()["reused"], true);
        assert_eq!(json!(store.get(id).unwrap()), json!(saved));
    }
    #[test]
    fn changed_policy_refuses_source_admission_and_compilation_without_new_authority() {
        for compiled in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            let saved = compiled.then(|| {
                let reply = opening(&engine, &source, &t).unwrap();
                store.get(reply["id"].as_str().unwrap()).unwrap()
            });
            store
                .automation_policy_checkpoint(
                    PolicyCommand::Revoke {
                        id: p.id.clone(),
                        revision: 1,
                        operation_id: uuid::Uuid::new_v4().to_string(),
                    },
                    None,
                    &p.runtime_id,
                    t.claimed_at_ms + 1,
                )
                .unwrap();
            if let Some(saved) = saved {
                let candidate = crate::request_planning::tests::compiled(&saved);
                assert!(
                    engine
                        .edit_review_plan(&saved.plan_id, |plan| {
                            plan.goal = candidate.goal;
                            plan.tasks = candidate.tasks;
                            plan.planning = candidate.planning;
                            plan.status = PlanStatus::AwaitingApproval;
                            Ok(true)
                        })
                        .is_err()
                );
                assert_eq!(json!(store.get(&saved.plan_id).unwrap()), json!(saved));
            } else {
                assert!(opening(&engine, &source, &t).is_err());
                assert_eq!(store.usage().unwrap()["count"], 1);
            }
        }
    }
    #[test]
    fn cancelled_retained_partial_source_resolves_without_fabricating_reservation_or_refund() {
        for compiled in [false, true] {
            let store = JobStore::in_memory().unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            let job = if compiled {
                fresh(&engine, &source, &t)
            } else {
                let reply = opening(&engine, &source, &t).unwrap();
                store.get(reply["id"].as_str().unwrap()).unwrap()
            };
            assert!(
                store
                    .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 1)
                    .is_err()
            );
            let cancelled = engine.cancel(&job.plan_id).unwrap();
            assert!(cancelled.approval.is_none());
            assert!(cancelled.execution.is_none());
            assert!(store.remove_finished(&job.plan_id).is_err());
            let settled = store
                .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 2)
                .unwrap();
            assert_eq!(settled.state, State::Waiting);
            assert_eq!(settled.claimed_runs, 1);
            assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
            assert_eq!(store.usage().unwrap()["trigger_count"], 1);
            assert_eq!(json!(store.get(&job.plan_id).unwrap()), json!(cancelled));
        }
    }
    #[test]
    fn source_origin_substitution_and_missing_partial_history_never_resolve_a_claim() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        let altered = eefn::context::RequestContext::new(
            t.request_context.request_id().into(),
            "other-node".into(),
            t.request_context.origin_area().to_vec(),
        )
        .unwrap();
        assert!(crate::request_drafts::open(&engine,json!({"schema_version":1,"original_text":" Inspect status ","proposal":source.request_review.as_ref().unwrap().proposals[0].proposal}),&altered,&["system.info".into()]).is_err());
        let reply = opening(&engine, &source, &t).unwrap();
        let id = reply["id"].as_str().unwrap();
        engine.cancel(id).unwrap();
        {
            let connection = store.connection.lock().unwrap();
            connection
                .execute("DELETE FROM durable_jobs WHERE job_id=?1", [id])
                .unwrap();
        }
        assert!(
            store
                .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 1)
                .unwrap_err()
                .to_string()
                .contains("unresolved")
        );
        assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
    }
    fn claim(store: &JobStore, p: &Authorization, s: &Schedule) -> Trigger {
        store
            .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || {
                s.next_due_ms.unwrap()
            })
            .unwrap()
            .unwrap()
    }
    fn fresh(engine: &TaskEngine, source: &Plan, t: &Trigger) -> Plan {
        let saved = crate::request_drafts::open(
            engine,
            json!({"schema_version":1,"original_text":" Inspect status ",
            "proposal":source.request_review.as_ref().unwrap().proposals[0].proposal}),
            &t.request_context,
            &["system.info".into()],
        )
        .unwrap();
        let saved = engine.store.get(saved["id"].as_str().unwrap()).unwrap();
        let compiled = crate::request_planning::tests::compiled(&saved);
        engine
            .edit_review_plan(&saved.plan_id, |p| {
                p.goal = compiled.goal;
                p.tasks = compiled.tasks;
                p.planning = compiled.planning;
                p.status = PlanStatus::AwaitingApproval;
                Ok(true)
            })
            .unwrap()
    }
    fn reserve_command(p: &Authorization, job: &Plan, t: &Trigger) -> OccurrenceCommand {
        OccurrenceCommand::Reserve {
            policy: p.id.clone(),
            policy_sha256: p.authorization_sha256.clone(),
            job: job.plan_id.clone(),
            revision: job.revision,
            plan_sha256: job.planning.as_ref().unwrap().plan_sha256.clone(),
            operation_id: t.reservation_id.clone(),
        }
    }
    fn reserve(store: &JobStore, p: &Authorization, job: &Plan, t: &Trigger) -> serde_json::Value {
        store
            .occurrence_checkpoint(
                reserve_command(p, job, t),
                &p.runtime_id,
                Some(&p.issuer_id),
                // Compilation uses the real journal clock; injected approval
                // time must not precede that source under parallel test load.
                (t.claimed_at_ms + 1).max(job.updated_at_ms),
            )
            .unwrap()
    }
    #[test]
    fn arm_references_and_final_clock_are_exact_without_refreshing_old_receipts() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, p, s) = fixture(store.clone());
        let count = store.usage().unwrap()["schedule_count"].clone();
        for field in ["hash", "runtime", "issuer", "nil", "time"] {
            let mut command = arm(&p);
            if field == "hash" {
                command.policy_sha256 = "a".repeat(64);
            }
            if field == "nil" {
                command.operation_id = uuid::Uuid::nil().to_string();
            }
            let runtime = if field == "runtime" {
                uuid::Uuid::new_v4().to_string()
            } else {
                p.runtime_id.clone()
            };
            let issuer = if field == "issuer" {
                "b".repeat(64)
            } else {
                p.issuer_id.clone()
            };
            let now = if field == "time" {
                p.expires_at_ms - 1
            } else {
                p.approved_at_ms + 1
            };
            assert!(
                store
                    .schedule_arm(command, &runtime, &issuer, || now)
                    .is_err(),
                "{field}"
            );
        }
        assert_eq!(store.usage().unwrap()["schedule_count"], count);
        store
            .schedule_stop(
                &s.id,
                1,
                &uuid::Uuid::new_v4().to_string(),
                p.approved_at_ms + 2,
            )
            .unwrap();
        let calls = std::cell::Cell::new(0u8);
        assert!(
            store
                .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || {
                    let n = calls.get();
                    calls.set(n + 1);
                    if n == 0 {
                        p.approved_at_ms + 3
                    } else {
                        p.expires_at_ms
                    }
                })
                .is_err()
        );
        assert_eq!(store.usage().unwrap()["schedule_count"], count);
    }
    #[test]
    fn settling_near_expiry_exhausts_time_instead_of_scheduling_a_shortened_run() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        let job = fresh(&engine, &source, &t);
        reserve(&store, &p, &job, &t);
        engine.cancel(&job.plan_id).unwrap();
        let allowance = p
            .scope
            .run_allowance(&p.creation.limits().unwrap())
            .unwrap()
            .0;
        let now = p.expires_at_ms - s.every_ms - allowance + 1;
        let ended = store
            .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, now)
            .unwrap();
        assert_eq!(ended.state, State::Exhausted);
        assert_eq!(ended.reason.as_deref(), Some("time_budget"));
        assert_eq!(ended.claimed_runs, 1);
        assert!(ended.next_due_ms.is_none());
        assert_eq!(store.usage().unwrap()["occurrence_count"], 1);
    }
    #[test]
    fn concurrent_due_claim_is_one_durable_identity_and_never_creates_a_job() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, source, p, s) = fixture(store.clone());
        let original = json!(store.get(&source.plan_id).unwrap());
        assert!(
            store
                .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || s.armed_at_ms)
                .unwrap()
                .is_none()
        );
        let barrier = std::sync::Barrier::new(16);
        let claims = std::thread::scope(|scope| {
            (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        store
                            .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || {
                                s.next_due_ms.unwrap()
                            })
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(claims.len(), 1);
        let t = &claims[0];
        assert_eq!(t.ordinal, 1);
        assert_ne!(
            t.request_context.request_id(),
            p.scope.owner_context.request_id()
        );
        assert_eq!(
            t.request_context.origin_area(),
            p.scope.owner_context.origin_area()
        );
        assert_eq!(store.usage().unwrap()["trigger_count"], 1);
        assert_eq!(store.usage().unwrap()["count"], 1);
        assert_eq!(store.usage().unwrap()["occurrence_count"], 0);
        assert!(
            store
                .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || p.expires_at_ms + 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(json!(store.get(&source.plan_id).unwrap()), original);
    }
    #[test]
    fn claim_insert_quota_and_final_clock_faults_roll_back_cursor_and_all_ids() {
        for fault in ["insert", "count", "bytes", "backward", "late"] {
            let mut store = JobStore::in_memory().unwrap();
            let (engine, _source, p, s) = fixture(store.clone());
            drop(engine);
            let before = store.usage().unwrap();
            if fault == "count" {
                let count = automations::count(&store.connection.lock().unwrap()).unwrap();
                Arc::get_mut(&mut store).unwrap().max_count = count;
            }
            if fault == "bytes" {
                Arc::get_mut(&mut store).unwrap().max_bytes = 1;
            }
            if fault == "insert" {
                store.connection.lock().unwrap().execute_batch("CREATE TRIGGER claim_fault BEFORE INSERT ON automation_triggers BEGIN SELECT RAISE(FAIL,'isolated trigger insert fault'); END;").unwrap();
            }
            let calls = std::cell::Cell::new(0);
            let due = s.next_due_ms.unwrap();
            let result = store.schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || {
                let n = calls.get();
                calls.set(n + 1);
                if n > 0 && fault == "backward" {
                    due - 1
                } else if n > 0 && fault == "late" {
                    due + s.every_ms
                } else {
                    due
                }
            });
            assert!(result.is_err(), "{fault}");
            let connection = store.connection.lock().unwrap();
            assert_eq!(read_schedule(&connection, &s.id).unwrap().unwrap(), s);
            assert!(triggers(&connection).unwrap().is_empty());
            drop(connection);
            assert_eq!(
                store.usage().unwrap()["trigger_count"],
                before["trigger_count"]
            );
        }
    }
    #[test]
    fn missed_backward_or_changed_authority_interrupts_without_catchup() {
        for fault in ["missed", "backward", "runtime", "issuer", "revoked"] {
            let store = JobStore::in_memory().unwrap();
            let (_engine, _source, p, s) = fixture(store.clone());
            let due = s.next_due_ms.unwrap();
            if fault == "revoked" {
                store
                    .automation_policy_checkpoint(
                        PolicyCommand::Revoke {
                            id: p.id.clone(),
                            revision: 1,
                            operation_id: uuid::Uuid::new_v4().to_string(),
                        },
                        None,
                        &p.runtime_id,
                        due - 1,
                    )
                    .unwrap();
            }
            let runtime = if fault == "runtime" {
                uuid::Uuid::new_v4().to_string()
            } else {
                p.runtime_id.clone()
            };
            let issuer = if fault == "issuer" {
                "b".repeat(64)
            } else {
                p.issuer_id.clone()
            };
            let now = if fault == "missed" {
                due + s.every_ms
            } else if fault == "backward" {
                s.armed_at_ms - 1
            } else {
                due
            };
            assert!(
                store
                    .schedule_claim_due(&s.id, &runtime, &issuer, || now)
                    .unwrap()
                    .is_none()
            );
            let interrupted = read_schedule(&store.connection.lock().unwrap(), &s.id)
                .unwrap()
                .unwrap();
            assert_eq!(interrupted.state, State::Interrupted);
            assert!(interrupted.next_due_ms.is_none());
            assert_eq!(store.usage().unwrap()["trigger_count"], 0);
            assert!(
                store
                    .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || due + 1)
                    .unwrap()
                    .is_none()
            );
        }
    }
    #[test]
    fn unadmitted_claim_stays_uncertain_and_stop_reuse_never_rearms_it() {
        let store = JobStore::in_memory().unwrap();
        let (_engine, _source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        assert!(
            store
                .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 1)
                .unwrap_err()
                .to_string()
                .contains("unresolved")
        );
        let op = uuid::Uuid::new_v4().to_string();
        let (stopped, reused) = store
            .schedule_stop(&s.id, 2, &op, t.claimed_at_ms + 1)
            .unwrap();
        assert!(!reused);
        assert_eq!(stopped.active_trigger.as_deref(), Some(t.id.as_str()));
        assert!(
            store
                .schedule_stop(&s.id, 2, &op, p.expires_at_ms + 1)
                .unwrap()
                .1
        );
        assert_eq!(
            store
                .schedule_arm(
                    s.creation.clone(),
                    &uuid::Uuid::new_v4().to_string(),
                    "b",
                    || p.expires_at_ms + 1
                )
                .unwrap(),
            (stopped, true)
        );
        assert!(
            store
                .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || t.claimed_at_ms + 1)
                .unwrap_err()
                .to_string()
                .contains("overlap")
        );
        assert!(
            store
                .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || t.claimed_at_ms + 1)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .schedule_stop(
                    &s.id,
                    2,
                    &uuid::Uuid::new_v4().to_string(),
                    t.claimed_at_ms + 1
                )
                .is_err()
        );
    }
    #[test]
    fn linked_reservation_rejects_direct_approval_receipt_substitution_and_stopped_issuance() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let t = claim(&store, &p, &s);
        let job = fresh(&engine, &source, &t);
        let direct = crate::request_approval::tests::command(&job);
        assert!(
            crate::request_approval::tests::save(
                &engine,
                &job,
                &direct,
                crate::request_approval::tests::record(&direct)
            )
            .is_err()
        );
        let mut wrong = reserve_command(&p, &job, &t);
        if let OccurrenceCommand::Reserve { operation_id, .. } = &mut wrong {
            *operation_id = uuid::Uuid::new_v4().to_string();
        }
        assert!(
            store
                .occurrence_checkpoint(
                    wrong,
                    &p.runtime_id,
                    Some(&p.issuer_id),
                    t.claimed_at_ms + 1
                )
                .is_err()
        );
        let result = reserve(&store, &p, &job, &t);
        let approved = store.get(&job.plan_id).unwrap();
        assert_eq!(
            result["reservation"]["delegation"]["trigger"],
            json!(TriggerLink {
                schedule_id: s.id.clone(),
                trigger_id: t.id.clone()
            })
        );
        {
            let connection = store.connection.lock().unwrap();
            automation_occurrences::fence(
                &connection,
                &approved,
                &p.runtime_id,
                &p.issuer_id,
                || t.claimed_at_ms + 2,
            )
            .unwrap();
        }
        let wrong_exec = uuid::Uuid::new_v4().to_string();
        assert!(
            store
                .change(&job.plan_id, "isolated wrong execution receipt", |plan| {
                    plan.status = PlanStatus::Running;
                    plan.execution = Some(crate::request_execution::ExecutionRecord {
                        schema_version: 1,
                        command: eefn::request_execution::ExecuteCommand {
                            id: job.plan_id.clone(),
                            expected_revision: approved.revision,
                            operation_id: wrong_exec,
                            approval_sha256: approved
                                .approval
                                .as_ref()
                                .unwrap()
                                .approval_sha256
                                .clone(),
                        },
                        started_at_ms: t.claimed_at_ms + 1,
                        attempts: vec![],
                        abandonment: None,
                    });
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(store.get(&job.plan_id).unwrap()), json!(approved));
        store
            .schedule_stop(
                &s.id,
                2,
                &uuid::Uuid::new_v4().to_string(),
                t.claimed_at_ms + 2,
            )
            .unwrap();
        let connection = store.connection.lock().unwrap();
        assert!(
            automation_occurrences::fence(
                &connection,
                &approved,
                &p.runtime_id,
                &p.issuer_id,
                || t.claimed_at_ms + 3
            )
            .unwrap_err()
            .to_string()
            .contains("Durable trigger stopped")
        );
        drop(connection);
        assert!(
            store
                .occurrence_checkpoint(
                    reserve_command(&p, &job, &t),
                    &p.runtime_id,
                    None,
                    t.claimed_at_ms + 3
                )
                .unwrap()["reused"]
                .as_bool()
                .unwrap()
        );
    }
    #[test]
    fn next_fixed_delay_needs_actual_terminal_proof_and_never_refunds_claims_or_reservations() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, mut s) = fixture(store.clone());
        for ordinal in 1..=3 {
            let t = claim(&store, &p, &s);
            let job = fresh(&engine, &source, &t);
            let result = reserve(&store, &p, &job, &t);
            assert_eq!(result["reservation"]["ordinal"], ordinal);
            assert!(
                store
                    .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, t.claimed_at_ms + 2)
                    .is_err()
            );
            let cancelled = engine.cancel(&job.plan_id).unwrap();
            // Source/cancellation checkpoints use the actual wall clock. The
            // synthetic due clock must not date their archive before them when
            // other parallel tests make this fixture take more than one interval.
            let settled_at = (t.claimed_at_ms + 2).max(cancelled.updated_at_ms);
            store
                .occurrence_checkpoint(
                    OccurrenceCommand::Archive {
                        reservation: t.reservation_id.clone(),
                        reservation_sha256: result["reservation"]["reservation_sha256"]
                            .as_str()
                            .unwrap()
                            .into(),
                        job: job.plan_id.clone(),
                        revision: cancelled.revision,
                        operation_id: uuid::Uuid::new_v4().to_string(),
                    },
                    &p.runtime_id,
                    None,
                    settled_at,
                )
                .unwrap();
            s = store
                .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, settled_at)
                .unwrap();
            assert_eq!(s.claimed_runs, ordinal);
            if ordinal < 3 {
                assert_eq!(s.next_due_ms, Some(settled_at + s.every_ms));
            }
        }
        assert_eq!(s.state, State::Exhausted);
        assert_eq!(s.reason.as_deref(), Some("run_budget"));
        assert_eq!(store.usage().unwrap()["trigger_count"], 3);
        assert_eq!(store.usage().unwrap()["occurrence_count"], 3);
        assert_eq!(store.usage().unwrap()["occurrence_archive_count"], 3);
        assert!(
            store
                .schedule_arm(arm(&p), &p.runtime_id, &p.issuer_id, || s.observed_at_ms
                    + 1)
                .unwrap_err()
                .to_string()
                .contains("budget exhausted")
        );
        assert_eq!(store.usage().unwrap()["schedule_count"], 1);
    }
    #[test]
    fn restart_interrupts_waiting_and_inflight_even_under_lowered_storage_limits() {
        for inflight in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            Connection::open(&path).unwrap();
            let store = JobStore::open(&path, 30, 1024 * 1024).unwrap();
            let (engine, source, p, s) = fixture(store.clone());
            let t = inflight.then(|| claim(&store, &p, &s));
            let original = json!(store.get(&source.plan_id).unwrap());
            drop(engine);
            drop(store);
            let store = JobStore::open(&path, 1, 1024).unwrap();
            let saved = read_schedule(&store.connection.lock().unwrap(), &s.id)
                .unwrap()
                .unwrap();
            assert_eq!(saved.state, State::Interrupted);
            assert_eq!(saved.reason.as_deref(), Some("restart"));
            assert_eq!(saved.active_trigger, t.as_ref().map(|t| t.id.clone()));
            if let Some(t) = t {
                assert_eq!(
                    read_trigger(&store.connection.lock().unwrap(), &t.id)
                        .unwrap()
                        .unwrap(),
                    t
                );
            }
            assert_eq!(json!(store.get(&source.plan_id).unwrap()), original);
            let op = uuid::Uuid::new_v4().to_string();
            store
                .schedule_stop(&s.id, saved.revision, &op, eefn::protocol::now_ms())
                .unwrap();
            assert!(
                store
                    .schedule_claim_due(&s.id, &p.runtime_id, &p.issuer_id, || p.approved_at_ms
                        + 2000)
                    .unwrap()
                    .is_none()
            );
            assert!(store.usage().unwrap()["over_limit"].as_bool().unwrap());
        }
    }
    #[test]
    fn damaged_missing_or_future_trigger_history_is_retained_without_restart_repair() {
        for fault in ["future", "missing", "scope"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("jobs.db");
            Connection::open(&path).unwrap();
            let store = JobStore::open(&path, 30, 1024 * 1024).unwrap();
            let (engine, _source, p, s) = fixture(store.clone());
            let t = claim(&store, &p, &s);
            {
                let connection = store.connection.lock().unwrap();
                if fault == "missing" {
                    connection
                        .execute("DELETE FROM automation_triggers", [])
                        .unwrap();
                } else {
                    let mut raw = json!(t);
                    if fault == "future" {
                        raw["schema_version"] = json!(2);
                    } else {
                        raw["request_context"]["origin_node"] = json!("other");
                    }
                    connection
                        .execute(
                            "UPDATE automation_triggers SET record_json=?1",
                            [serde_json::to_string(&raw).unwrap()],
                        )
                        .unwrap();
                }
            }
            drop(engine);
            drop(store);
            assert!(JobStore::open(&path, 30, 1024 * 1024).is_err(), "{fault}");
            let connection = Connection::open(&path).unwrap();
            let raw: String = connection
                .query_row("SELECT record_json FROM automation_schedules", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Schedule>(&raw).unwrap().state,
                State::InFlight
            );
        }
    }
    #[test]
    fn duplicate_active_ownership_and_cross_claim_identity_aliases_fail_closed() {
        let store = JobStore::in_memory().unwrap();
        let (engine, source, p, s) = fixture(store.clone());
        let mut duplicate = s.clone();
        duplicate.id = uuid::Uuid::new_v4().to_string();
        duplicate.creation.operation_id = duplicate.id.clone();
        duplicate.record_sha256 = digest(&duplicate).unwrap();
        {
            let connection = store.connection.lock().unwrap();
            let raw = serde_json::to_string(&duplicate).unwrap();
            connection.execute("INSERT INTO automation_schedules(schedule_id,record_json,reserved_bytes) VALUES(?1,?2,?3)",params![duplicate.id,raw,(SCHEDULE_COST-raw.len()) as i64]).unwrap();
            assert!(
                validate_all(&connection)
                    .unwrap_err()
                    .to_string()
                    .contains("Multiple active")
            );
            // Isolated fault injection cleanup only; the core never repairs it.
            connection
                .execute(
                    "DELETE FROM automation_schedules WHERE schedule_id=?1",
                    [&duplicate.id],
                )
                .unwrap();
        }
        let first = claim(&store, &p, &s);
        let job = fresh(&engine, &source, &first);
        reserve(&store, &p, &job, &first);
        engine.cancel(&job.plan_id).unwrap();
        let waiting = store
            .schedule_settle(&s.id, &p.runtime_id, &p.issuer_id, first.claimed_at_ms + 2)
            .unwrap();
        let mut second = claim(&store, &p, &waiting);
        assert!(
            store
                .schedule_stop(
                    &s.id,
                    waiting.revision + 1,
                    &second.execution_id,
                    second.claimed_at_ms + 1
                )
                .is_err()
        );
        second.execution_id = first.reservation_id;
        second.record_sha256 = digest(&second).unwrap();
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE automation_triggers SET record_json=?2 WHERE trigger_id=?1",
                params![second.id, serde_json::to_string(&second).unwrap()],
            )
            .unwrap();
        assert!(
            validate_all(&connection)
                .unwrap_err()
                .to_string()
                .contains("identity is reused")
        );
    }
}
