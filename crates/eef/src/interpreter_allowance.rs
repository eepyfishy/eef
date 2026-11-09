//! Persistent service reservations; no scheduler or model execution lives here.
use anyhow::{Context, Result, ensure};
use eefn::{
    grants::{ExecutionGrant, GrantReport, ReceiptState},
    interpreter_task::{Source, Task},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

pub(crate) const MAX_RECORD_BYTES: usize = 2 * 1024 * 1024;
// Existing result bytes plus bounded full-report/phase/recovery metadata.
pub(crate) const CHECKPOINT_HEADROOM: usize = 16 * 1024;

/// Not serializable: live gateway observations cannot be supplied over an API.
pub(crate) struct Participants {
    pub origin: eefn::interpreter_service::Session,
    pub provider: eefn::interpreter_service::Session,
    pub backend: String,
    pub model_id: String,
}
impl Participants {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.origin.is_fresh() && self.provider.is_fresh(),
            "Interpreter participants are stale"
        );
        ensure!(
            self.origin.issuer_id() == self.provider.issuer_id(),
            "Interpreter issuer changed between participants"
        );
        ensure!(
            self.provider.serves(&self.backend, &self.model_id),
            "Interpreter selection changed or is unavailable"
        );
        Ok(())
    }
}

/// Core-only proof assembled from current opaque gateway observations. It is
/// never serialized into a request or reconstructed from retained history.
pub(crate) struct Permit {
    pub(super) origin_node: String,
    pub(super) origin_runtime: String,
    pub(super) origin_session: String,
    pub(super) provider_node: String,
    pub(super) provider_runtime: String,
    pub(super) provider_session: String,
    pub(super) issuer: String,
    pub(super) backend: String,
    pub(super) model_id: String,
    pub(super) permission_revision: u64,
    pub(super) limits: eefn::grants::GrantPolicy,
    pub(super) capabilities: Vec<String>,
    pub(super) fresh_until: Instant,
}
impl Permit {
    pub fn from_participants(
        participants: &Participants,
        capabilities: Vec<String>,
    ) -> Result<Self> {
        participants.validate()?;
        Ok(Self {
            origin_node: participants.origin.node_id().into(),
            origin_runtime: participants.origin.runtime_id().into(),
            origin_session: participants.origin.session_id().into(),
            provider_node: participants.provider.node_id().into(),
            provider_runtime: participants.provider.runtime_id().into(),
            provider_session: participants.provider.session_id().into(),
            issuer: participants.provider.issuer_id().into(),
            backend: participants.backend.clone(),
            model_id: participants.model_id.clone(),
            permission_revision: participants
                .provider
                .permission_revision()
                .context("Interpreter sharing unavailable")?,
            limits: participants.provider.limits().clone(),
            capabilities,
            fresh_until: participants
                .origin
                .freshness_deadline()
                .min(participants.provider.freshness_deadline()),
        })
    }
    pub fn validate(&self, task: &Task) -> Result<()> {
        ensure!(
            Instant::now() < self.fresh_until,
            "Interpreter permit observation expired"
        );
        ensure!(
            task.snapshot.request_context.origin_node() == self.origin_node
                && task.snapshot.allowed_capabilities == self.capabilities,
            "Interpreter source differs from authenticated origin/disclosure"
        );
        self.limits.validate()?;
        for id in [
            &self.origin_runtime,
            &self.origin_session,
            &self.provider_runtime,
            &self.provider_session,
        ] {
            eefn::interpreter_policy::uuid(id)?;
        }
        eefn::request_interpretation::digest(&self.issuer)?;
        eefn::network::validate_remote_node_id(&self.origin_node)?;
        eefn::network::validate_remote_node_id(&self.provider_node)?;
        ensure!(
            matches!(self.backend.as_str(), "ollama" | "llamacpp"),
            "Unsupported interpreter backend"
        );
        eefn::model_selection::validate_id(&self.model_id)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PolicyPin {
    pub id: String,
    pub generation: u64,
    pub sha256: String,
}
impl From<&eefn::interpreter_policy::Authorization> for PolicyPin {
    fn from(policy: &eefn::interpreter_policy::Authorization) -> Self {
        Self {
            id: policy.id.clone(),
            generation: policy.generation,
            sha256: policy.authorization_sha256.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Reserved,
    DeliveryIntent,
    Accepted,
    Final,
    Interrupted,
    NotSent,
}
impl Phase {
    pub fn holds_slot(self) -> bool {
        !matches!(self, Self::Final | Self::NotSent)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub sequence: u64,
    pub previous_sha256: String,
    pub policy_id: String,
    pub policy_generation: u64,
    pub policy_sha256: String,
    pub runtime_id: String,
    pub origin_runtime_id: String,
    pub origin_session_id: String,
    pub provider_runtime_id: String,
    pub reserved_at_ms: u64,
    pub grant: ExecutionGrant,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema_version: u32,
    pub binding: Binding,
    pub binding_sha256: String,
    pub revision: u64,
    pub phase: Phase,
    pub delivery_at_ms: Option<u64>,
    pub report: Option<GrantReport>,
}
impl Record {
    pub fn holds_slot(&self) -> bool {
        self.phase.holds_slot()
            || self
                .report
                .as_ref()
                .is_some_and(|r| r.state == ReceiptState::Unknown)
    }
    pub fn id(&self) -> &str {
        &self.binding.grant.grant_id
    }
    pub fn task(&self) -> Result<Task> {
        let task: Task =
            serde_json::from_value(self.binding.grant.params["interpreter_task"].clone())?;
        task.validate()?;
        Ok(task)
    }
    pub fn stage(&self) -> Result<&'static str> {
        Ok(stage(&self.task()?.snapshot.source))
    }
    pub fn digest(&self) -> Result<String> {
        eefn::grants::definition_digest(
            &json!({"domain":"eef.interpreter.allowance.v1","binding":self.binding}),
        )
    }
    pub fn checkpoint_reserve(&self) -> Result<usize> {
        if matches!(self.phase, Phase::Final | Phase::NotSent) {
            return Ok(0);
        }
        self.allocation()?
            .checked_sub(serde_json::to_vec(self)?.len())
            .context("Interpreter checkpoint exceeded its original allocation")
    }
    pub fn allocation(&self) -> Result<usize> {
        let initial = Self {
            schema_version: 1,
            binding: self.binding.clone(),
            binding_sha256: self.binding_sha256.clone(),
            revision: 1,
            phase: Phase::Reserved,
            delivery_at_ms: None,
            report: None,
        };
        serde_json::to_vec(&initial)?
            .len()
            .checked_add(self.binding.grant.limits.max_result_bytes)
            .and_then(|n| n.checked_add(CHECKPOINT_HEADROOM))
            .context("Interpreter allocation overflow")
    }
    pub fn validate(&self) -> Result<()> {
        let binding = &self.binding;
        let g = &binding.grant;
        g.validate()?;
        for id in [
            &binding.policy_id,
            &binding.runtime_id,
            &binding.origin_runtime_id,
            &binding.origin_session_id,
            &binding.provider_runtime_id,
        ] {
            eefn::interpreter_policy::uuid(id)?;
        }
        eefn::request_interpretation::digest(&binding.policy_sha256)?;
        eefn::request_interpretation::digest(&binding.previous_sha256)?;
        ensure!(
            self.schema_version == 1
                && binding.policy_generation > 0
                && binding.policy_generation <= i64::MAX as u64
                && binding.sequence > 0
                && binding.sequence <= 1_000_000
                && binding.reserved_at_ms > 0
                && binding.reserved_at_ms >= g.issued_at_ms
                && binding.reserved_at_ms < g.expires_at_ms
                && self.binding_sha256 == self.digest()?
                && self.revision > 0,
            "Invalid interpreter allowance binding"
        );
        let report_ok = self.report.as_ref().is_none_or(|r| r.confirms(g));
        ensure!(report_ok, "Interpreter report differs from reserved grant");
        ensure!(
            self.delivery_at_ms
                .is_none_or(|at| at >= binding.reserved_at_ms && at < g.expires_at_ms),
            "Invalid interpreter delivery time"
        );
        let delivered = self.delivery_at_ms.is_some();
        let valid_phase = match self.phase {
            Phase::Reserved => self.revision == 1 && !delivered && self.report.is_none(),
            Phase::DeliveryIntent => self.revision == 2 && delivered && self.report.is_none(),
            Phase::Accepted => {
                self.revision == 3
                    && delivered
                    && self
                        .report
                        .as_ref()
                        .is_some_and(|r| r.state == ReceiptState::Accepted)
            }
            Phase::Final => {
                (3..=5).contains(&self.revision)
                    && delivered
                    && self
                        .report
                        .as_ref()
                        .is_some_and(|r| r.state != ReceiptState::Accepted)
            }
            Phase::Interrupted => {
                delivered
                    && match &self.report {
                        None => self.revision == 3,
                        Some(r) => self.revision == 4 && r.state == ReceiptState::Accepted,
                    }
            }
            Phase::NotSent => self.revision == 2 && !delivered && self.report.is_none(),
        };
        ensure!(valid_phase, "Invalid interpreter allowance lifecycle");
        self.task()?;
        ensure!(
            serde_json::to_vec(self)?.len() <= MAX_RECORD_BYTES,
            "Interpreter allowance record exceeds its bound"
        );
        ensure!(
            serde_json::to_vec(self)?.len() <= self.allocation()?,
            "Interpreter record exceeds its reserved checkpoint budget"
        );
        Ok(())
    }
    pub fn matches_policy(&self, policy: &eefn::interpreter_policy::Authorization) -> Result<()> {
        policy.validate()?;
        self.validate()?;
        let binding = &self.binding;
        let g = &binding.grant;
        let eefn::interpreter_policy::Command::Approve { scope, limits, .. } = &policy.creation
        else {
            unreachable!()
        };
        let task = self.task()?;
        ensure!(
            binding.policy_id == policy.id
                && binding.policy_generation == policy.generation
                && binding.policy_sha256 == policy.authorization_sha256
                && g.issuer_id == policy.issuer_id
                && binding.reserved_at_ms >= policy.approved_at_ms
                && policy
                    .expires_at_ms
                    .is_none_or(|end| binding.reserved_at_ms < end)
                && scope
                    .origin_nodes
                    .iter()
                    .any(|n| n == g.request_context.origin_node())
                && scope.provider_nodes.contains(&g.executor_node)
                && serde_json::to_vec(&task)?.len() <= limits.max_task_bytes
                && task.max_output_tokens <= limits.max_output_tokens
                && g.limits.max_execution_ms <= limits.max_execution_ms
                && g.limits.max_result_bytes <= limits.max_result_bytes,
            "Interpreter allowance differs from its retained policy"
        );
        Ok(())
    }
}
pub(crate) fn stage(source: &Source) -> &'static str {
    match source {
        Source::Initial { .. } => "initial",
        Source::Clarification { .. } => "clarification",
        Source::Method { .. } => "method",
    }
}

/// A new reservation's one in-process continuation, never reconstructed on read.
pub(crate) struct Reserved {
    pub(super) record: Record,
    pub(super) deadline: Instant,
}
impl Reserved {
    pub fn id(&self) -> &str {
        self.record.id()
    }
    pub fn initial_claim(&self) -> Result<Option<eefn::request_interpretation::Claim>> {
        let task = self.record.task()?;
        let Source::Initial { input } = &task.snapshot.source else {
            return Ok(None);
        };
        let g = &self.record.binding.grant;
        Ok(Some(eefn::request_interpretation::Claim {
            id: g.job_id.clone(),
            expected_revision: task.snapshot.revision,
            source_sha256: input.digest(&task.snapshot.request_context)?,
            operation_id: task.snapshot.operation_id,
            origin_runtime_id: self.record.binding.origin_runtime_id.clone(),
            interpreter: Some(eefn::request_interpretation::Interpreter {
                node_id: g.executor_node.clone(),
                backend: g.backend.clone().context("Missing interpreter backend")?,
                model_id: g.model_id.clone().context("Missing interpreter model")?,
                runtime_id: self.record.binding.provider_runtime_id.clone(),
            }),
        }))
    }
}
pub(crate) struct Dispatch {
    pub(super) grant: ExecutionGrant,
    pub(super) deadline: Instant,
}
impl Dispatch {
    pub fn grant(&self) -> &ExecutionGrant {
        &self.grant
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}
pub(crate) enum Reservation {
    New(Reserved),
    Existing(Box<Record>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Charge {
    pub calls: u64,
    pub output_tokens: u64,
    pub execution_ms: u64,
    pub result_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Account {
    pub schema_version: u32,
    pub policy_id: String,
    pub policy_generation: u64,
    pub policy_sha256: String,
    pub last_sha256: String,
    pub charged: Charge,
}
impl Account {
    pub fn empty(policy: &eefn::interpreter_policy::Authorization) -> Result<Self> {
        Ok(Self {
            schema_version: 1,
            policy_id: policy.id.clone(),
            policy_generation: policy.generation,
            policy_sha256: policy.authorization_sha256.clone(),
            last_sha256: eefn::grants::definition_digest(
                &json!({"domain":"eef.interpreter.allowance.account.v1","policy_sha256":policy.authorization_sha256}),
            )?,
            charged: Default::default(),
        })
    }
}
impl Charge {
    pub fn add(&mut self, record: &Record) -> Result<()> {
        self.calls = self
            .calls
            .checked_add(1)
            .context("Interpreter call count overflow")?;
        self.output_tokens = self
            .output_tokens
            .checked_add(record.task()?.max_output_tokens)
            .context("Interpreter token charge overflow")?;
        self.execution_ms = self
            .execution_ms
            .checked_add(record.binding.grant.limits.max_execution_ms)
            .context("Interpreter time charge overflow")?;
        self.result_bytes = self
            .result_bytes
            .checked_add(u64::try_from(record.binding.grant.limits.max_result_bytes)?)
            .context("Interpreter result charge overflow")?;
        Ok(())
    }
}

impl crate::Runtime {
    async fn current_interpreter_permit(
        &self,
        task: &Task,
        participants: &Participants,
    ) -> Result<Permit> {
        let (capabilities, freshness) = self
            .interpreter_participant_context(
                participants.origin.node_id(),
                participants.provider.node_id(),
            )
            .await?;
        let origin = self
            .node_server
            .interpreter_session(participants.origin.node_id(), freshness)
            .await?;
        let provider = self
            .node_server
            .interpreter_session(participants.provider.node_id(), freshness)
            .await?;
        ensure!(
            origin.same_connection(&participants.origin)
                && provider.same_connection(&participants.provider)
                && provider.permission_revision() == participants.provider.permission_revision()
                && json!(provider.limits()) == json!(participants.provider.limits()),
            "Interpreter participants changed; no substitution"
        );
        let current = Participants {
            origin,
            provider,
            backend: participants.backend.clone(),
            model_id: participants.model_id.clone(),
        };
        let permit = Permit::from_participants(&current, capabilities)?;
        permit.validate(task)?;
        Ok(permit)
    }
    pub(crate) async fn reserve_interpreter_allowance(
        &self,
        task: Task,
        pin: PolicyPin,
        participants: &Participants,
    ) -> Result<Reservation> {
        let permit = self.current_interpreter_permit(&task, participants).await?;
        self.config.edit_for_restart(|saved| {
            let settings = eefn::authentication::AuthenticationPolicy::parse(
                saved.pointer("/node/authentication"),
            )?;
            let issuer = crate::interpreter_policy::current_issuer(
                self.node_server.authentication_policy(),
                &settings,
            )?;
            ensure!(
                issuer.as_deref() == Some(permit.issuer.as_str()),
                "Interpreter authentication changed before reservation"
            );
            Ok((
                self.engine.store.reserve_interpreter_allowance(
                    task,
                    &pin,
                    &permit,
                    self.runtime_id(),
                )?,
                false,
            ))
        })
    }
    pub(crate) async fn interpreter_delivery_intent(
        &self,
        reserved: &Reserved,
        pin: &PolicyPin,
        participants: &Participants,
    ) -> Result<Dispatch> {
        let task = reserved.record.task()?;
        let permit = self.current_interpreter_permit(&task, participants).await?;
        self.config.edit_for_restart(|saved| {
            let settings = eefn::authentication::AuthenticationPolicy::parse(
                saved.pointer("/node/authentication"),
            )?;
            let issuer = crate::interpreter_policy::current_issuer(
                self.node_server.authentication_policy(),
                &settings,
            )?;
            ensure!(
                issuer.as_deref() == Some(permit.issuer.as_str()),
                "Interpreter authentication changed before delivery intent"
            );
            Ok((
                self.engine.store.interpreter_delivery_intent(
                    reserved,
                    &permit,
                    self.runtime_id(),
                    pin,
                )?,
                false,
            ))
        })
    }
}
