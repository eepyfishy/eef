//! Atomic interpreter accounts and attempts in the existing coordinator journal.
use super::*;
use crate::interpreter_allowance::{
    Account, Binding, Dispatch, MAX_RECORD_BYTES, Permit, Phase, PolicyPin, Record, Reservation,
    Reserved, stage,
};
use eefn::{
    grants::{ExecutionGrant, GrantLimits, GrantReport, ReceiptState},
    interpreter_task::{Source, Task},
};
use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

pub(super) fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS interpreter_allowance_accounts (
        policy_id TEXT PRIMARY KEY, record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB))<=4096));
        CREATE TABLE IF NOT EXISTS interpreter_allowance_attempts (
        grant_id TEXT PRIMARY KEY,policy_id TEXT NOT NULL,sequence INTEGER NOT NULL CHECK(sequence>0),
        source_job_id TEXT NOT NULL,source_revision INTEGER NOT NULL CHECK(source_revision>0),
        stage TEXT NOT NULL CHECK(stage IN ('initial','clarification','method')),
        operation_id TEXT NOT NULL,origin_node TEXT NOT NULL,provider_node TEXT NOT NULL,
        record_json TEXT NOT NULL CHECK(length(CAST(record_json AS BLOB))<=2097152),
        reserved_bytes INTEGER NOT NULL CHECK(reserved_bytes>=0 AND reserved_bytes<=2097152),
        UNIQUE(policy_id,sequence),UNIQUE(source_job_id,source_revision,stage),UNIQUE(origin_node,operation_id,stage));")?;
    Ok(())
}
pub(super) fn bytes(connection: &Connection) -> Result<usize> {
    let total:i64=connection.query_row("SELECT (SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))+reserved_bytes),0) FROM interpreter_allowance_attempts)+(SELECT COALESCE(SUM(length(CAST(record_json AS BLOB))),0) FROM interpreter_allowance_accounts)",[],|r|r.get(0))?;
    Ok(usize::try_from(total)?)
}
fn snapshot(connection: &Connection) -> Result<(Vec<Record>, BTreeMap<String, Account>)> {
    let policies = interpreter_policies::records(connection)?;
    let mut accounts = BTreeMap::new();
    let mut statement=connection.prepare("SELECT policy_id,length(CAST(record_json AS BLOB)),record_json FROM interpreter_allowance_accounts")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let length: i64 = row.get(1)?;
        if !(1..=4096).contains(&length) {
            bail!("Invalid interpreter account size; retained")
        }
        let id: String = row.get(0)?;
        let raw: String = row.get(2)?;
        let account: Account = serde_json::from_str(&raw)?;
        if account.schema_version != 1
            || account.policy_id != id
            || account.charged.calls == 0
            || accounts.insert(id, account).is_some()
        {
            bail!("Invalid interpreter account identity; retained")
        }
    }
    let mut computed = BTreeMap::new();
    let mut records = Vec::new();
    let mut jobs = HashSet::new();
    let mut statement=connection.prepare("SELECT grant_id,policy_id,sequence,source_job_id,source_revision,stage,operation_id,origin_node,provider_node,length(CAST(record_json AS BLOB)),record_json,reserved_bytes FROM interpreter_allowance_attempts ORDER BY policy_id,sequence")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let length: i64 = row.get(9)?;
        if !(1..=MAX_RECORD_BYTES as i64).contains(&length) {
            bail!("Oversized interpreter attempt; retained")
        }
        let raw: String = row.get(10)?;
        let record: Record = serde_json::from_str(&raw)?;
        record.validate()?;
        let binding = &record.binding;
        let g = &binding.grant;
        let task = record.task()?;
        let policy = policies
            .iter()
            .find(|p| p.id == binding.policy_id)
            .context("Interpreter account policy is missing; retained")?;
        record.matches_policy(policy)?;
        if row.get::<_, String>(0)? != record.id()
            || row.get::<_, String>(1)? != binding.policy_id
            || u64::try_from(row.get::<_, i64>(2)?)? != binding.sequence
            || row.get::<_, String>(3)? != g.job_id
            || u64::try_from(row.get::<_, i64>(4)?)? != task.snapshot.revision
            || row.get::<_, String>(5)? != record.stage()?
            || row.get::<_, String>(6)? != task.snapshot.operation_id
            || row.get::<_, String>(7)? != g.request_context.origin_node()
            || row.get::<_, String>(8)? != g.executor_node
            || usize::try_from(row.get::<_, i64>(11)?)? != record.checkpoint_reserve()?
        {
            bail!("Interpreter attempt columns/reservation differ; retained")
        }
        if !computed.contains_key(&policy.id) {
            computed.insert(policy.id.clone(), Account::empty(policy)?);
        }
        let account = computed.get_mut(&policy.id).expect("inserted account");
        if binding.sequence != account.charged.calls + 1
            || binding.previous_sha256 != account.last_sha256
        {
            bail!("Interpreter allowance chain is incomplete; retained")
        }
        account.charged.add(&record)?;
        account.last_sha256 = record.binding_sha256.clone();
        let eefn::interpreter_policy::Command::Approve { limits, .. } = &policy.creation else {
            unreachable!()
        };
        if account.charged.calls > u64::from(limits.max_calls)
            || account.charged.output_tokens > limits.max_reserved_output_tokens
            || account.charged.execution_ms > limits.max_reserved_execution_ms
            || account.charged.result_bytes > limits.max_reserved_result_bytes
        {
            bail!("Retained interpreter charges exceed their policy; retained")
        }
        if record.holds_slot() && !jobs.insert(g.job_id.clone()) {
            bail!("Interpreter job has multiple active owners; retained")
        }
        records.push(record);
    }
    if computed != accounts {
        bail!("Interpreter account totals/history differ; retained without resetting allowances")
    }
    Ok((records, accounts))
}
pub(super) fn validate_all(connection: &Connection) -> Result<()> {
    snapshot(connection).map(|_| ())
}

fn source_matches(plan: &Plan, record: &Record, delivering: bool) -> Result<()> {
    let task = record.task()?;
    if plan.plan_id != task.snapshot.job_id
        || plan.goal.request_context.as_ref() != Some(&task.snapshot.request_context)
        || plan.status != PlanStatus::Review
        || plan.planning.is_some()
        || plan.approval.is_some()
        || plan.execution.is_some()
    {
        bail!("Interpreter source is changed, stopped or already executable")
    }
    match &task.snapshot.source {
        Source::Initial { input } => {
            if crate::request_intent::validate(plan)? != *input {
                bail!("Interpreter raw source changed")
            }
            if delivering {
                let expected = Reserved {
                    record: record.clone(),
                    deadline: Instant::now(),
                }
                .initial_claim()?
                .context("Missing initial claim")?;
                let claim = plan
                    .input_interpretation
                    .as_ref()
                    .context("Interpreter source ownership missing")?;
                if claim.claim != expected
                    || claim.state != eefn::request_interpretation::State::Claimed
                    || plan.revision
                        != task
                            .snapshot
                            .revision
                            .checked_add(1)
                            .context("Source revision overflow")?
                {
                    bail!("Interpreter source ownership changed")
                }
            } else if plan.revision != task.snapshot.revision || plan.input_interpretation.is_some()
            {
                bail!("Interpreter source already claimed or stale")
            }
        }
        Source::Clarification { draft } | Source::Method { draft } => {
            if plan.revision != task.snapshot.revision
                || plan
                    .request_review
                    .as_ref()
                    .is_none_or(|saved| json!(saved) != json!(draft))
            {
                bail!("Interpreter review source changed")
            }
        }
    }
    Ok(())
}
fn update_record(connection: &Connection, old: &Record, new: &Record) -> Result<()> {
    new.validate()?;
    if old.binding_sha256 != new.binding_sha256 || json!(old.binding) != json!(new.binding) {
        bail!("Interpreter binding is immutable")
    }
    let encoded = serde_json::to_string(new)?;
    let reserve = new.checkpoint_reserve()?;
    let previous = serde_json::to_vec(old)?.len() + old.checkpoint_reserve()?;
    if encoded
        .len()
        .checked_add(reserve)
        .context("Interpreter checkpoint size overflow")?
        > previous
    {
        bail!("Interpreter checkpoint exceeds its original allocation")
    }
    if connection.execute("UPDATE interpreter_allowance_attempts SET record_json=?2,reserved_bytes=?3 WHERE grant_id=?1",params![new.id(),encoded,i64::try_from(reserve)?])?!=1 {bail!("Interpreter attempt disappeared before checkpoint")}
    Ok(())
}
fn retire_initial_claim(connection: &Connection, record: &Record) -> Result<()> {
    let continuation = Reserved {
        record: record.clone(),
        deadline: Instant::now(),
    };
    let Some(expected) = continuation.initial_claim()? else {
        return Ok(());
    };
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM durable_jobs WHERE job_id=?1)",
        [&expected.id],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut plan = read(connection, &expected.id)?;
    let Some(claim) = &plan.input_interpretation else {
        bail!("Interpreter source lost its ownership checkpoint")
    };
    if claim.claim != expected {
        bail!("Interpreter source ownership differs; retained")
    }
    if claim.state != eefn::request_interpretation::State::Claimed {
        return Ok(());
    }
    let before = serde_json::to_vec(&plan)?
        .len()
        .checked_add(reserve(&plan))
        .context("Interpreter source size overflow")?;
    crate::request_interpretation::apply(
        &mut plan,
        crate::request_interpretation::Transition::Interrupt,
        None,
        &[],
    )?;
    stamp(
        &mut plan,
        "Interpreter continuation retired; no automatic retry authorized",
    )?;
    validate_retained_review(&plan)?;
    let encoded = serde_json::to_string(&plan)?;
    if encoded
        .len()
        .checked_add(reserve(&plan))
        .context("Interpreter source size overflow")?
        > before
    {
        bail!("Interpreter retirement exceeds its retained checkpoint space")
    }
    connection.execute(
        "UPDATE durable_jobs SET plan_json=?2,updated_ms=?3,reserved_bytes=?4 WHERE job_id=?1",
        params![
            plan.plan_id,
            encoded,
            i64::try_from(plan.updated_at_ms)?,
            i64::try_from(reserve(&plan))?
        ],
    )?;
    Ok(())
}
pub(super) fn interrupt_on_open(store: &JobStore) -> Result<()> {
    let mut connection = store.connection.lock().expect("job journal");
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for old in snapshot(&transaction)?.0 {
        let mut record = old.clone();
        match old.phase {
            Phase::Reserved => record.phase = Phase::NotSent,
            Phase::DeliveryIntent | Phase::Accepted => record.phase = Phase::Interrupted,
            _ => continue,
        }
        record.revision = record
            .revision
            .checked_add(1)
            .context("Interpreter revision overflow")?;
        update_record(&transaction, &old, &record)?;
        retire_initial_claim(&transaction, &record)?;
    }
    transaction.commit()?;
    Ok(())
}

impl JobStore {
    pub(crate) fn interpreter_usage(
        &self,
        options: &crate::interpreter_usage::Options,
        runtime: &str,
    ) -> Result<serde_json::Value> {
        let connection = self.connection.lock().expect("job journal");
        let policies = interpreter_policies::records(&connection)?;
        let (records, accounts) = snapshot(&connection)?;
        crate::interpreter_usage::report(options, runtime, &policies, records, accounts)
    }
    pub(crate) fn interpreter_allowance_records(
        &self,
    ) -> Result<(Vec<Record>, BTreeMap<String, Account>)> {
        snapshot(&self.connection.lock().expect("job journal"))
    }
    pub(crate) fn reserve_interpreter_allowance(
        &self,
        task: Task,
        pin: &PolicyPin,
        permit: &Permit,
        runtime: &str,
    ) -> Result<Reservation> {
        task.validate()?;
        eefn::interpreter_policy::uuid(runtime)?;
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (records, accounts) = snapshot(&transaction)?;
        if let Some(old) = records.iter().find(|r| {
            r.binding.grant.job_id == task.snapshot.job_id
                && r.task().is_ok_and(|t| {
                    t.snapshot.revision == task.snapshot.revision
                        && stage(&t.snapshot.source) == stage(&task.snapshot.source)
                })
        }) {
            if json!(old.task()?) != json!(task) {
                bail!(
                    "Another interpreter intent already owns this source; inspect retained history"
                )
            }
            return Ok(Reservation::Existing(Box::new(old.clone())));
        }
        permit.validate(&task)?;
        let policies = interpreter_policies::records(&transaction)?;
        let policy = policies.last().context("No interpreter policy")?;
        let minted = Instant::now();
        let now = eefn::protocol::now_ms();
        if PolicyPin::from(policy) != *pin || !policy.active(policy.generation, &permit.issuer, now)
        {
            bail!("Interpreter policy changed or is inactive")
        }
        let eefn::interpreter_policy::Command::Approve { scope, limits, .. } = &policy.creation
        else {
            unreachable!()
        };
        if !scope.origin_nodes.contains(&permit.origin_node)
            || !scope.provider_nodes.contains(&permit.provider_node)
            || task.max_output_tokens > limits.max_output_tokens
            || serde_json::to_vec(&task)?.len() > limits.max_task_bytes
        {
            bail!("Interpreter request exceeds its policy scope or per-call limits")
        }
        let active: Vec<_> = records.iter().filter(|r| r.holds_slot()).collect();
        if active
            .iter()
            .any(|r| r.binding.grant.job_id == task.snapshot.job_id)
            || active.len() >= usize::from(limits.max_in_flight)
            || active
                .iter()
                .filter(|r| r.binding.grant.request_context.origin_node() == permit.origin_node)
                .count()
                >= usize::from(limits.max_in_flight_per_origin)
            || active
                .iter()
                .filter(|r| r.binding.grant.executor_node == permit.provider_node)
                .count()
                >= usize::from(limits.max_in_flight_per_provider)
        {
            bail!("Interpreter job or concurrency allowance is occupied; no replacement or queue")
        }
        let prior = accounts.get(&policy.id);
        let mut account = prior.cloned().unwrap_or(Account::empty(policy)?);
        let effective = GrantLimits {
            max_execution_ms: limits.max_execution_ms.min(permit.limits.max_duration_ms),
            max_result_bytes: limits.max_result_bytes.min(permit.limits.max_result_bytes),
        };
        let deadline = minted
            .checked_add(Duration::from_millis(effective.max_execution_ms))
            .context("Interpreter deadline overflow")?;
        let grant = ExecutionGrant {
            schema_version: 1,
            grant_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: permit.issuer.clone(),
            session_id: permit.provider_session.clone(),
            executor_node: permit.provider_node.clone(),
            job_id: task.snapshot.job_id.clone(),
            task_id: uuid::Uuid::new_v4().simple().to_string(),
            attempt: 1,
            plan_sha256: eefn::grants::definition_digest(
                &json!({"domain":"eef.interpreter.task.v1","task":task}),
            )?,
            request_context: task.snapshot.request_context.clone(),
            capability: "llm.infer".into(),
            action: "run".into(),
            params: json!({"model_id":permit.model_id,"backend":permit.backend,"permission_revision":permit.permission_revision,"interpreter_task":task}),
            resource_id: None,
            model_id: Some(permit.model_id.clone()),
            backend: Some(permit.backend.clone()),
            model_role: Some("request_interpreter".into()),
            priority: 0,
            limits: effective,
            reporting: "acceptance_and_completion".into(),
            issued_at_ms: now,
            expires_at_ms: now
                .checked_add(limits.max_execution_ms.min(permit.limits.max_duration_ms))
                .context("Interpreter time overflow")?,
        };
        grant.validate()?;
        let mut record = Record {
            schema_version: 1,
            binding: Binding {
                sequence: account
                    .charged
                    .calls
                    .checked_add(1)
                    .context("Interpreter sequence overflow")?,
                previous_sha256: account.last_sha256.clone(),
                policy_id: policy.id.clone(),
                policy_generation: policy.generation,
                policy_sha256: policy.authorization_sha256.clone(),
                runtime_id: runtime.into(),
                origin_runtime_id: permit.origin_runtime.clone(),
                origin_session_id: permit.origin_session.clone(),
                provider_runtime_id: permit.provider_runtime.clone(),
                reserved_at_ms: now,
                grant,
            },
            binding_sha256: String::new(),
            revision: 1,
            phase: Phase::Reserved,
            delivery_at_ms: None,
            report: None,
        };
        record.binding_sha256 = record.digest()?;
        record.matches_policy(policy)?;
        account.charged.add(&record)?;
        account.last_sha256 = record.binding_sha256.clone();
        if account.charged.calls > u64::from(limits.max_calls)
            || account.charged.output_tokens > limits.max_reserved_output_tokens
            || account.charged.execution_ms > limits.max_reserved_execution_ms
            || account.charged.result_bytes > limits.max_reserved_result_bytes
        {
            bail!("Interpreter aggregate allowance exhausted; no grant or model call")
        }
        let mut source = read(&transaction, &task.snapshot.job_id)?;
        source_matches(&source, &record, false)?;
        let reserved = Reserved {
            record: record.clone(),
            deadline,
        };
        let changed = if let Some(claim) = reserved.initial_claim()? {
            crate::request_interpretation::apply(
                &mut source,
                crate::request_interpretation::Transition::Claim(claim),
                Some(&permit.origin_node),
                &task.snapshot.allowed_capabilities,
            )?;
            stamp(
                &mut source,
                "Interpreter service reserved; no delivery authorized",
            )?;
            validate_retained_review(&source)?;
            true
        } else {
            false
        };
        let source_json = serde_json::to_string(&source)?;
        let source_cost = source_json
            .len()
            .checked_add(reserve(&source))
            .context("Interpreter source size overflow")?;
        let other:i64=transaction.query_row("SELECT COALESCE(SUM(length(CAST(plan_json AS BLOB))+reserved_bytes),0) FROM durable_jobs WHERE job_id<>?1",[&source.plan_id],|r|r.get(0))?;
        let old_account = prior
            .map(serde_json::to_vec)
            .transpose()?
            .map_or(0, |v| v.len());
        let encoded_account = serde_json::to_string(&account)?;
        if encoded_account.len() > 4096 {
            bail!("Interpreter account exceeds its bound")
        }
        let allocation = record.allocation()?;
        let used = usize::try_from(other)?
            .checked_add(automations::definition_bytes(&transaction)?)
            .and_then(|n| n.checked_sub(old_account))
            .and_then(|n| n.checked_add(source_cost))
            .and_then(|n| n.checked_add(encoded_account.len()))
            .and_then(|n| n.checked_add(allocation))
            .context("Interpreter reservation storage overflow")?;
        let added = if prior.is_some() { 1 } else { 2 };
        if automations::count(&transaction)?
            .checked_add(added)
            .context("Interpreter count overflow")?
            > self.max_count
            || used > self.max_bytes
            || source_cost > 8 * 1024 * 1024
        {
            bail!("Shared job/policy storage is full; no interpreter reservation saved")
        }
        let encoded = serde_json::to_string(&record)?;
        transaction.execute("INSERT INTO interpreter_allowance_attempts(grant_id,policy_id,sequence,source_job_id,source_revision,stage,operation_id,origin_node,provider_node,record_json,reserved_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![record.id(),policy.id,i64::try_from(record.binding.sequence)?,task.snapshot.job_id,i64::try_from(task.snapshot.revision)?,stage(&task.snapshot.source),task.snapshot.operation_id,permit.origin_node,permit.provider_node,encoded,i64::try_from(record.checkpoint_reserve()?)?])?;
        transaction.execute("INSERT INTO interpreter_allowance_accounts(policy_id,record_json) VALUES(?1,?2) ON CONFLICT(policy_id) DO UPDATE SET record_json=excluded.record_json",params![policy.id,encoded_account])?;
        if changed {
            transaction.execute("UPDATE durable_jobs SET plan_json=?2,updated_ms=?3,reserved_bytes=?4 WHERE job_id=?1",params![source.plan_id,source_json,i64::try_from(source.updated_at_ms)?,i64::try_from(reserve(&source))?])?;
        }
        transaction.commit()?;
        Ok(Reservation::New(reserved))
    }
    pub(crate) fn interpreter_delivery_intent(
        &self,
        reserved: &Reserved,
        permit: &Permit,
        runtime: &str,
        pin: &PolicyPin,
    ) -> Result<Dispatch> {
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let records = snapshot(&transaction)?.0;
        let old = records
            .iter()
            .find(|r| r.id() == reserved.id())
            .context("Interpreter reservation unavailable")?;
        if old.phase != Phase::Reserved
            || old.binding_sha256 != reserved.record.binding_sha256
            || old.binding.runtime_id != runtime
            || Instant::now() >= reserved.deadline
        {
            bail!("Interpreter reservation changed, expired or belongs to another runtime")
        }
        let task = old.task()?;
        permit.validate(&task)?;
        let binding = &old.binding;
        let grant = &binding.grant;
        if permit.origin_runtime != binding.origin_runtime_id
            || permit.origin_session != binding.origin_session_id
            || permit.provider_runtime != binding.provider_runtime_id
            || permit.provider_session != grant.session_id
            || permit.issuer != grant.issuer_id
            || permit.provider_node != grant.executor_node
            || grant.backend.as_deref() != Some(permit.backend.as_str())
            || grant.model_id.as_deref() != Some(permit.model_id.as_str())
            || grant.params["permission_revision"] != json!(permit.permission_revision)
            || grant.limits.max_execution_ms > permit.limits.max_duration_ms
            || grant.limits.max_result_bytes > permit.limits.max_result_bytes
        {
            bail!("Interpreter participants/model/limits changed before delivery")
        }
        let policies = interpreter_policies::records(&transaction)?;
        let policy = policies.last().context("Interpreter policy unavailable")?;
        let now = eefn::protocol::now_ms();
        if PolicyPin::from(policy) != *pin
            || binding.policy_id != policy.id
            || !policy.active(policy.generation, &permit.issuer, now)
            || now < binding.reserved_at_ms
            || now >= grant.expires_at_ms
        {
            bail!("Interpreter policy or original grant time changed")
        }
        source_matches(&read(&transaction, &grant.job_id)?, old, true)?;
        let mut record = old.clone();
        record.phase = Phase::DeliveryIntent;
        record.revision = 2;
        record.delivery_at_ms = Some(now);
        update_record(&transaction, old, &record)?;
        transaction.commit()?;
        Ok(Dispatch {
            grant: record.binding.grant,
            deadline: reserved.deadline,
        })
    }
    pub(crate) fn interpreter_not_sent(&self, reserved: Reserved) -> Result<Record> {
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let records = snapshot(&transaction)?.0;
        let old = records
            .iter()
            .find(|r| r.id() == reserved.id())
            .context("Interpreter reservation unavailable")?;
        if old.phase != Phase::Reserved || old.binding_sha256 != reserved.record.binding_sha256 {
            bail!("Interpreter reservation cannot be marked unsent")
        }
        let mut record = old.clone();
        record.phase = Phase::NotSent;
        record.revision = 2;
        update_record(&transaction, old, &record)?;
        retire_initial_claim(&transaction, &record)?;
        transaction.commit()?;
        Ok(record)
    }
    pub(crate) fn interpreter_report(&self, id: &str, report: GrantReport) -> Result<Record> {
        let mut connection = self.connection.lock().expect("job journal");
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let records = snapshot(&transaction)?.0;
        let old = records
            .iter()
            .find(|r| r.id() == id)
            .context("Interpreter attempt unavailable")?;
        if old
            .report
            .as_ref()
            .is_some_and(|saved| json!(saved) == json!(report))
        {
            return Ok(old.clone());
        }
        if !report.confirms(&old.binding.grant)
            || matches!(old.phase, Phase::Reserved | Phase::NotSent | Phase::Final)
            || (report.state == ReceiptState::Accepted && old.phase != Phase::DeliveryIntent)
            || (report.state == ReceiptState::Rejected
                && old
                    .report
                    .as_ref()
                    .is_some_and(|r| r.state == ReceiptState::Accepted))
        {
            bail!("Interpreter report is stale, conflicting or outside delivery")
        }
        let mut record = old.clone();
        record.phase = if report.state == ReceiptState::Accepted {
            Phase::Accepted
        } else {
            Phase::Final
        };
        record.report = Some(report);
        record.revision = record
            .revision
            .checked_add(1)
            .context("Interpreter revision overflow")?;
        update_record(&transaction, old, &record)?;
        if record.phase == Phase::Final && record.report.as_ref().is_some_and(|r| !r.success) {
            retire_initial_claim(&transaction, &record)?;
        }
        transaction.commit()?;
        Ok(record)
    }
}

#[cfg(test)]
#[path = "interpreter_allowance_store_tests.rs"]
mod tests;
