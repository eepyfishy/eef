//! Bounded local-owner observation. No raw inputs or executable grants are returned.
use crate::{
    Runtime,
    interpreter_allowance::{Charge, Phase},
};
use anyhow::{Context, Result, ensure};
use eefn::{
    grants::ReceiptState,
    interpreter_policy::{Authorization, Command, Limits},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub id: Option<String>,
    #[arg(long, default_value_t = 25)]
    pub limit: u16,
    #[arg(long, default_value_t = 0)]
    pub after_sequence: u64,
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        if let Some(id) = &self.id {
            eefn::interpreter_policy::uuid(id)?;
        }
        ensure!(
            (1..=100).contains(&self.limit) && self.after_sequence <= 1_000_000,
            "Invalid interpreter usage page"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub expected_runtime_id: String,
    pub command: Options,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    id: String,
    generation: u64,
    sha256: String,
    revoked: bool,
    limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    sequence: u64,
    job_id: String,
    stage: String,
    origin_node: String,
    provider_node: String,
    backend: String,
    model_id: String,
    phase: Phase,
    result_state: Option<ReceiptState>,
    holds_slot: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    success: bool,
    runtime_id: String,
    command: Options,
    current_generation: u64,
    policy: Option<Policy>,
    charged: Charge,
    occupied_slots: u64,
    items: Vec<Item>,
    next_after: Option<u64>,
    grants_issued: bool,
    model_inference_requested: bool,
    allowance_reserved: bool,
    automatically_retried: bool,
    automatic_delegation_available: bool,
}
impl Runtime {
    pub fn interpreter_usage(&self, request: Request) -> Result<Value> {
        ensure!(
            request.schema_version == 1 && request.expected_runtime_id == self.runtime_id(),
            "Coordinator runtime changed or usage schema is unsupported"
        );
        eefn::interpreter_policy::uuid(&request.expected_runtime_id)?;
        request.command.validate()?;
        // One journal gate observes policy, chain and account totals together.
        self.engine
            .store
            .interpreter_usage(&request.command, self.runtime_id())
    }
}
pub(crate) fn report(
    options: &Options,
    runtime: &str,
    policies: &[Authorization],
    records: Vec<crate::interpreter_allowance::Record>,
    accounts: std::collections::BTreeMap<String, crate::interpreter_allowance::Account>,
) -> Result<Value> {
    options.validate()?;
    eefn::interpreter_policy::uuid(runtime)?;
    let current_generation = policies.last().map_or(0, |p| p.generation);
    let selected = match &options.id {
        Some(id) => Some(
            policies
                .iter()
                .find(|p| p.id == *id)
                .context("Interpreter usage policy unavailable")?,
        ),
        None => policies.last(),
    };
    let mut result = Report {
        schema_version: 1,
        report_type: "interpreter_allowance_usage".into(),
        success: true,
        runtime_id: runtime.into(),
        command: options.clone(),
        current_generation,
        policy: None,
        charged: Default::default(),
        occupied_slots: 0,
        items: vec![],
        next_after: None,
        grants_issued: false,
        model_inference_requested: false,
        allowance_reserved: false,
        automatically_retried: false,
        automatic_delegation_available: false,
    };
    if let Some(p) = selected {
        let Command::Approve { limits, .. } = &p.creation else {
            unreachable!()
        };
        result.policy = Some(Policy {
            id: p.id.clone(),
            generation: p.generation,
            sha256: p.authorization_sha256.clone(),
            revoked: p.revocation.is_some(),
            limits: limits.clone(),
        });
        if let Some(account) = accounts.get(&p.id) {
            result.charged = account.charged.clone()
        }
        result.occupied_slots = records
            .iter()
            .filter(|r| r.binding.policy_id == p.id && r.holds_slot())
            .count() as u64;
        let mut page = records
            .iter()
            .filter(|r| r.binding.policy_id == p.id && r.binding.sequence > options.after_sequence);
        for r in page.by_ref().take(usize::from(options.limit)) {
            let g = &r.binding.grant;
            result.items.push(Item {
                id: r.id().into(),
                sequence: r.binding.sequence,
                job_id: g.job_id.clone(),
                stage: r.stage()?.into(),
                origin_node: g.request_context.origin_node().into(),
                provider_node: g.executor_node.clone(),
                backend: g.backend.clone().context("Missing interpreter backend")?,
                model_id: g.model_id.clone().context("Missing interpreter model")?,
                phase: r.phase,
                result_state: r.report.as_ref().map(|r| r.state),
                holds_slot: r.holds_slot(),
            });
        }
        if page.next().is_some() {
            result.next_after = result.items.last().map(|i| i.sequence)
        }
    }
    let value = json!(result);
    ensure!(
        confirmed(&value, options, runtime),
        "Interpreter usage reply unconfirmed"
    );
    Ok(value)
}
pub fn confirmed(value: &Value, options: &Options, runtime: &str) -> bool {
    let Ok(r) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    if options.validate().is_err()
        || eefn::interpreter_policy::uuid(runtime).is_err()
        || r.command != *options
        || r.runtime_id != runtime
        || r.schema_version != 1
        || r.report_type != "interpreter_allowance_usage"
        || !r.success
        || r.grants_issued
        || r.model_inference_requested
        || r.allowance_reserved
        || r.automatically_retried
        || r.automatic_delegation_available
        || r.current_generation > i64::MAX as u64
        || r.items.len() > usize::from(options.limit)
    {
        return false;
    }
    match &r.policy {
        None => {
            return options.id.is_none()
                && r.current_generation == 0
                && r.charged == Charge::default()
                && r.occupied_slots == 0
                && r.items.is_empty()
                && r.next_after.is_none();
        }
        Some(p) => {
            if eefn::interpreter_policy::uuid(&p.id).is_err()
                || eefn::request_interpretation::digest(&p.sha256).is_err()
                || p.generation == 0
                || p.generation > r.current_generation
                || p.limits.validate().is_err()
                || options.id.as_ref().is_some_and(|id| id != &p.id)
                || (options.id.is_none() && p.generation != r.current_generation)
                || r.charged.calls > u64::from(p.limits.max_calls)
                || r.charged.output_tokens > p.limits.max_reserved_output_tokens
                || r.charged.execution_ms > p.limits.max_reserved_execution_ms
                || r.charged.result_bytes > p.limits.max_reserved_result_bytes
                || r.occupied_slots > r.charged.calls
            {
                return false;
            }
        }
    }
    let expected_len = (r.charged.calls.saturating_sub(options.after_sequence))
        .min(u64::from(options.limit)) as usize;
    if r.items.len() != expected_len
        || r.next_after.is_some()
            != (options.after_sequence.saturating_add(expected_len as u64) < r.charged.calls)
    {
        return false;
    }
    let mut previous = options.after_sequence;
    for i in &r.items {
        let valid_state = match i.phase {
            Phase::Reserved | Phase::DeliveryIntent | Phase::NotSent => i.result_state.is_none(),
            Phase::Accepted => i.result_state == Some(ReceiptState::Accepted),
            Phase::Interrupted => {
                i.result_state.is_none() || i.result_state == Some(ReceiptState::Accepted)
            }
            Phase::Final => i.result_state.is_some_and(|s| s != ReceiptState::Accepted),
        };
        if !valid_state
            || eefn::interpreter_policy::uuid(&i.id).is_err()
            || eefn::request_drafts::validate_id(&i.job_id).is_err()
            || i.sequence != previous + 1
            || i.sequence > r.charged.calls
            || !matches!(i.stage.as_str(), "initial" | "clarification" | "method")
            || eefn::network::validate_remote_node_id(&i.origin_node).is_err()
            || eefn::network::validate_remote_node_id(&i.provider_node).is_err()
            || !matches!(i.backend.as_str(), "ollama" | "llamacpp")
            || eefn::model_selection::validate_id(&i.model_id).is_err()
            || i.holds_slot
                != (i.phase.holds_slot() || i.result_state == Some(ReceiptState::Unknown))
        {
            return false;
        }
        previous = i.sequence;
    }
    r.next_after.is_none_or(|n| {
        Some(n) == r.items.last().map(|i| i.sequence)
            && r.items.len() == usize::from(options.limit)
            && n < r.charged.calls
    })
}
pub fn render(value: &Value) -> Result<String> {
    let r: Report = serde_json::from_value(value.clone())?;
    ensure!(
        confirmed(value, &r.command, &r.runtime_id),
        "Unconfirmed interpreter usage report"
    );
    let mut text = if let Some(p) = &r.policy {
        format!(
            "Interpreter policy {}: generation {}{}.\nReserved calls: {}/{}; output tokens: {}; execution budget: {} ms; result budget: {} bytes.\nOccupied slots: {}.\n",
            p.id,
            p.generation,
            if p.revoked { " (revoked)" } else { "" },
            r.charged.calls,
            p.limits.max_calls,
            r.charged.output_tokens,
            r.charged.execution_ms,
            r.charged.result_bytes,
            r.occupied_slots
        )
    } else {
        "Interpreter sharing has no owner policy.\n".into()
    };
    for item in r.items {
        text.push_str(&format!(
            "{}  {}  {} -> {}  {:?}\n",
            item.sequence, item.id, item.origin_node, item.provider_node, item.phase
        ));
    }
    if let Some(next) = r.next_after {
        text.push_str(&format!("More history: --after-sequence {next}\n"));
    }
    text.push_str("Read-only retained usage; no allowance, grant or model call started. Reserved budgets are conservative, not measured compute usage.\n");
    Ok(text)
}
