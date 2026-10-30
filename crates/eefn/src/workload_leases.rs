//! Version-two session lease mechanics; public admission remains incomplete.
//! Internal prepared sources use this without a startup, registration or public
//! RPC launch. Existing v1 limits remain unchanged. Renewals retain one future.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

pub const MAX_SESSION_MS: u64 = 86_400_000;
pub const MAX_LEASE_MS: u64 = 30_000;
const MAX_PENDING: usize = 64;
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[path = "workload_definition.rs"]
pub mod definition;
#[path = "workload_reporting.rs"]
mod reporting;
pub(crate) use reporting::{Checkpoint, ReportingAuthority};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub max_session_ms: u64,
    pub max_lease_ms: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_session_ms: 3_600_000,
            max_lease_ms: MAX_LEASE_MS,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=MAX_SESSION_MS).contains(&self.max_session_ms)
                && (1..=MAX_LEASE_MS).contains(&self.max_lease_ms)
                && self.max_lease_ms <= self.max_session_ms,
            "Invalid workload lease policy"
        );
        Ok(())
    }
}

/// Scope comes from the exact separately reviewed/consumed workload definition.
/// Its digest must bind source, resource, origin, models, budgets and reporting.
/// This metadata validates a lease contract; it never grants execution by itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema_version: u32,
    pub scope: crate::grants::stop::Scope,
    pub max_session_ms: u64,
    pub max_lease_ms: u64,
}
impl Contract {
    pub fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        ensure!(
            self.schema_version == 2
                && (1..=MAX_SESSION_MS).contains(&self.max_session_ms)
                && (1..=MAX_LEASE_MS).contains(&self.max_lease_ms)
                && self.max_lease_ms <= self.max_session_ms,
            "Invalid workload lease contract"
        );
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        crate::grants::definition_digest(&json!(self))
    }
    fn permitted(&self, policy: &Policy) -> Result<()> {
        self.validate()?;
        policy.validate()?;
        ensure!(
            policy.enabled
                && self.max_session_ms <= policy.max_session_ms
                && self.max_lease_ms <= policy.max_lease_ms,
            "Workload leases disabled or outside local bounds"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub contract: Contract,
    pub sequence: u64,
}
impl Query {
    fn validate(&self) -> Result<()> {
        self.contract.validate()?;
        crate::automation_occurrence::uuid(&self.id)?;
        ensure!(
            self.kind == "workload_lease_probe" && self.schema_version == 2 && self.sequence > 0,
            "Invalid workload lease probe"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub contract_sha256: String,
    pub sequence: u64,
    pub lease_remaining_ms: u64,
    pub session_remaining_ms: u64,
}

/// Monotonic issuer-side helper for one already-recorded grant offer. The
/// coordinator must supply its original monotonic issuance deadline, authenticate each peer,
/// and call acknowledge_consumed only after validating the node's durable ACK.
/// Construction itself records no grant and supplies no admission authority.
pub struct IssuerWindow {
    contract: Contract,
    deadline: Instant,
    next_sequence: u64,
    consumed: bool,
    stopped: bool,
}
impl IssuerWindow {
    pub fn new(contract: Contract, policy: &Policy, issued_deadline: Instant) -> Result<Self> {
        contract.permitted(policy)?;
        let now = Instant::now();
        ensure!(
            issued_deadline > now
                && issued_deadline.duration_since(now)
                    <= Duration::from_millis(contract.max_session_ms),
            "Issuer session exceeds recorded bounds"
        );
        Ok(Self {
            contract,
            deadline: issued_deadline,
            next_sequence: 1,
            consumed: false,
            stopped: false,
        })
    }
    pub fn acknowledge_consumed(&mut self, scope: &crate::grants::stop::Scope) -> Result<()> {
        self.check_consumed(scope)?;
        self.consumed = true;
        Ok(())
    }
    pub fn check_consumed(&self, scope: &crate::grants::stop::Scope) -> Result<()> {
        ensure!(
            !self.stopped
                && !self.consumed
                && self.next_sequence == 2
                && Instant::now() < self.deadline
                && *scope == self.contract.scope,
            "Consumed acknowledgement scope or lifecycle changed"
        );
        Ok(())
    }
    pub fn stop(&mut self) {
        self.stopped = true;
    }
    pub fn reply(
        &mut self,
        query: &Query,
        issuer: &str,
        session: &str,
        node: &str,
    ) -> Result<Reply> {
        query.validate()?;
        ensure!(
            !self.stopped
                && Instant::now() < self.deadline
                && self.contract.scope.matches_session(issuer, session, node)
                && query.contract.digest()? == self.contract.digest()?
                && query.sequence == self.next_sequence
                && (query.sequence == 1 || self.consumed),
            "Issuer lease scope, sequence or admission changed"
        );
        let remaining_ms = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let window_ms = remaining_ms.min(self.contract.max_lease_ms);
        ensure!(
            window_ms > 5 + window_ms / 100,
            "Issuer lease has no conservative budget"
        );
        let next = self
            .next_sequence
            .checked_add(1)
            .context("Issuer lease sequence exhausted")?;
        let reply = Reply {
            kind: "workload_lease".into(),
            schema_version: 2,
            id: query.id.clone(),
            contract_sha256: self.contract.digest()?,
            sequence: query.sequence,
            lease_remaining_ms: window_ms,
            session_remaining_ms: remaining_ms,
        };
        self.next_sequence = next;
        Ok(reply)
    }
}
impl Reply {
    fn matches(&self, query: &Query) -> bool {
        self.kind == "workload_lease"
            && self.schema_version == 2
            && self.id == query.id
            && self.sequence == query.sequence
            && query
                .contract
                .digest()
                .is_ok_and(|digest| digest == self.contract_sha256)
    }
}

/// Non-serializable and non-cloneable. Created only by the nonce correlator;
/// eventual product callers must feed it authenticated session frames only.
/// It proves neither consumption, admission nor resource ownership.
pub struct LeaseProof {
    contract_sha256: String,
    sequence: u64,
    deadline: Instant,
    session_deadline: Instant,
}
impl LeaseProof {
    fn confirm(query: &Query, reply: Reply, sent: Instant) -> Result<Self> {
        query.validate()?;
        ensure!(
            reply.matches(query) && sent <= Instant::now() && sent.elapsed() < PROBE_TIMEOUT,
            "Workload lease scope changed or challenge expired"
        );
        ensure!(
            reply.lease_remaining_ms > 0
                && reply.lease_remaining_ms <= query.contract.max_lease_ms
                && reply.lease_remaining_ms <= reply.session_remaining_ms
                && reply.session_remaining_ms <= query.contract.max_session_ms,
            "Workload lease widened its contract"
        );
        let deadline = |ms: u64| -> Result<Instant> {
            let budget = ms
                .checked_sub(5 + ms / 100)
                .filter(|v| *v > 0)
                .context("No conservative workload lease budget")?;
            sent.checked_add(Duration::from_millis(budget))
                .context("Workload lease time overflow")
        };
        let proof = Self {
            contract_sha256: query.contract.digest()?,
            sequence: query.sequence,
            deadline: deadline(reply.lease_remaining_ms)?,
            session_deadline: deadline(reply.session_remaining_ms)?,
        };
        ensure!(
            Instant::now() < proof.deadline.min(proof.session_deadline),
            "Workload lease round trip consumed its budget"
        );
        Ok(proof)
    }
    pub fn session_deadline(&self) -> Instant {
        self.session_deadline
    }
    pub(crate) fn initial_deadline(&self, contract: &Contract, policy: &Policy) -> Result<Instant> {
        contract.permitted(policy)?;
        ensure!(
            self.sequence == 1 && self.contract_sha256 == contract.digest()?,
            "Initial workload lease scope changed"
        );
        let deadline = self.deadline.min(self.session_deadline);
        ensure!(Instant::now() < deadline, "Initial workload lease expired");
        Ok(deadline)
    }
}

struct Pending {
    query: Query,
    sent: Instant,
    sent_wall: u64,
    sender: oneshot::Sender<Result<LeaseProof>>,
}
#[derive(Default)]
pub struct Requests {
    pending: Mutex<HashMap<String, Pending>>,
}
struct Cleanup {
    requests: Arc<Requests>,
    id: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.requests.pending.lock().unwrap().remove(&self.id);
    }
}
impl Requests {
    /// Reserved for the authenticated response demultiplexer. No current route
    /// calls it; a parsed reply alone must never become workload admission.
    #[allow(dead_code)]
    pub(crate) fn complete(&self, value: Value) {
        let Ok(reply) = serde_json::from_value::<Reply>(value) else {
            return;
        };
        let mut pending = self.pending.lock().unwrap();
        if !pending
            .get(&reply.id)
            .is_some_and(|p| reply.matches(&p.query))
        {
            return;
        }
        let entry = pending.remove(&reply.id).unwrap();
        let expected = entry
            .sent_wall
            .saturating_add(entry.sent.elapsed().as_millis().min(u64::MAX as u128) as u64);
        let result = if crate::protocol::now_ms().abs_diff(expected) > 1000 {
            Err(anyhow::anyhow!(
                "Local clock changed during workload lease challenge"
            ))
        } else {
            LeaseProof::confirm(&entry.query, reply, entry.sent)
        };
        let _ = entry.sender.send(result);
    }
    pub async fn request<F, C>(
        self: &Arc<Self>,
        contract: &Contract,
        sequence: u64,
        write: F,
    ) -> Result<LeaseProof>
    where
        F: FnOnce(Value) -> C,
        C: Future<Output = Result<()>>,
    {
        let query = Query {
            kind: "workload_lease_probe".into(),
            schema_version: 2,
            id: uuid::Uuid::new_v4().to_string(),
            contract: contract.clone(),
            sequence,
        };
        query.validate()?;
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, p| !p.sender.is_closed());
            ensure!(
                pending.len() < MAX_PENDING,
                "Workload lease challenge capacity full"
            );
            pending.insert(
                query.id.clone(),
                Pending {
                    query: query.clone(),
                    sent: Instant::now(),
                    sent_wall: crate::protocol::now_ms(),
                    sender,
                },
            );
        }
        let _cleanup = Cleanup {
            requests: self.clone(),
            id: query.id.clone(),
        };
        tokio::time::timeout(PROBE_TIMEOUT, async {
            write(json!(query)).await?;
            receiver.await.context("Workload lease session ended")?
        })
        .await
        .context("Workload lease challenge timed out")?
    }
}

/// Four metadata proofs at most, never an unbounded payload/event queue.
pub struct Renewals(mpsc::Receiver<LeaseProof>);
pub fn renewal_channel() -> (mpsc::Sender<LeaseProof>, Renewals) {
    let (sender, receiver) = mpsc::channel(4);
    (sender, Renewals(receiver))
}
struct Window {
    digest: String,
    sequence: u64,
    deadline: Instant,
    session_deadline: Instant,
    reporting: Option<reporting::Guard>,
}
impl Window {
    fn new_with_reporting(
        contract: &Contract,
        policy: &Policy,
        proof: LeaseProof,
        reporting: Option<ReportingAuthority>,
    ) -> Result<Self> {
        // Even a failed first lease/binding attempt permanently retires the
        // associated reporting lifetime. Only this owned runner can publish it.
        let reporting_guard = reporting.map(ReportingAuthority::guard);
        let mut window = Self::new(contract, policy, proof)?;
        if let Some(guard) = reporting_guard {
            guard.bind(&window)?;
            window.reporting = Some(guard);
        }
        Ok(window)
    }
    fn new(contract: &Contract, policy: &Policy, proof: LeaseProof) -> Result<Self> {
        contract.permitted(policy)?;
        ensure!(
            proof.contract_sha256 == contract.digest()? && proof.sequence == 1,
            "Initial workload lease scope changed"
        );
        let result = Self {
            digest: proof.contract_sha256,
            sequence: 1,
            deadline: proof.deadline,
            session_deadline: proof.session_deadline,
            reporting: None,
        };
        result.live()?;
        Ok(result)
    }
    fn live(&self) -> Result<()> {
        ensure!(
            Instant::now() < self.deadline.min(self.session_deadline),
            "Workload lease expired"
        );
        Ok(())
    }
    fn advance(&mut self, proof: LeaseProof) -> Result<()> {
        self.live()?;
        ensure!(
            proof.contract_sha256 == self.digest
                && Some(proof.sequence) == self.sequence.checked_add(1),
            "Workload renewal scope, replay or sequence changed"
        );
        // Even a fresh reply cannot move the original conservative session end.
        let session_deadline = self.session_deadline.min(proof.session_deadline);
        ensure!(
            Instant::now() < proof.deadline.min(session_deadline),
            "Workload renewal already expired"
        );
        self.sequence = proof.sequence;
        self.deadline = proof.deadline;
        self.session_deadline = session_deadline;
        if let Some(reporting) = &self.reporting {
            reporting.advance(self)?;
        }
        Ok(())
    }
}

/// Drive exactly one already-admitted operation. Renewals never construct a new
/// source, repeat a recording or extend the initial session's absolute deadline.
/// This lease primitive does not consume a durable job/grant, check permissions,
/// acquire resources, create renewals or start work from a network/UI handler.
pub fn supervise<F, T>(
    contract: &Contract,
    policy: &Policy,
    initial: LeaseProof,
    renewals: Renewals,
    operation: F,
) -> impl Future<Output = Result<T>>
where
    F: Future<Output = Result<T>>,
{
    supervise_inner(contract, policy, initial, renewals, operation, None)
}

pub(crate) fn supervise_reporting<F, T>(
    contract: &Contract,
    policy: &Policy,
    initial: LeaseProof,
    renewals: Renewals,
    operation: F,
    reporting: ReportingAuthority,
) -> impl Future<Output = Result<T>>
where
    F: Future<Output = Result<T>>,
{
    supervise_inner(
        contract,
        policy,
        initial,
        renewals,
        operation,
        Some(reporting),
    )
}

async fn supervise_inner<F, T>(
    contract: &Contract,
    policy: &Policy,
    initial: LeaseProof,
    mut renewals: Renewals,
    operation: F,
    reporting: Option<ReportingAuthority>,
) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    let mut window = Window::new_with_reporting(contract, policy, initial, reporting)?;
    let mut closed = false;
    tokio::pin!(operation);
    loop {
        window.live()?;
        tokio::select! {
            biased;
            _=tokio::time::sleep_until(tokio::time::Instant::from_std(window.deadline.min(window.session_deadline)))=>return Err(anyhow::anyhow!("Workload lease expired")),
            proof=renewals.0.recv(),if !closed=>match proof {Some(proof)=>window.advance(proof)?,None=>closed=true},
            value=&mut operation=>{window.live()?;return value},
        }
    }
}

#[cfg(test)]
#[path = "workload_leases_tests.rs"]
mod tests;
