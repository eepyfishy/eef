//! Framed, pinned-session workload control. This transport never mints leases
//! or authorizes a source; an original reviewed issuer must process each frame.
use super::*;
use crate::{
    workload_leases::{Query, Reply, definition::Definition},
    workload_network::{Advertisement, Offer},
};
use anyhow::ensure;
use std::sync::Mutex as StdMutex;
use tokio::sync::watch;

const CONTROL_COUNT: usize = 4;
const CONTROL_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub struct WorkloadPeer {
    node: String,
    session: String,
    issuer: String,
    runtime: String,
}
impl WorkloadPeer {
    pub fn node(&self) -> &str {
        &self.node
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn runtime(&self) -> &str {
        &self.runtime
    }
}

/// Carries byte ownership through caller processing. Peer identity comes from
/// the authenticated connection, independently of any fields in this payload.
pub struct WorkloadFrame {
    envelope: report_queue::Envelope,
    peer: Arc<WorkloadPeer>,
}
impl WorkloadFrame {
    pub fn message(&self) -> &Value {
        &self.envelope.message
    }
    pub fn peer(&self) -> &WorkloadPeer {
        &self.peer
    }
}

struct Entry {
    node: String,
    session: Uuid,
    generation: Uuid,
    definition: Definition,
    control: report_queue::Queue,
    events: report_queue::Queue,
    event_slots: Arc<tokio::sync::Semaphore>,
    cancelled: watch::Sender<bool>,
    ack: Option<Value>,
    last_event: Option<crate::workload_events::Event>,
    event_bytes: usize,
    terminal: bool,
}
#[derive(Default)]
pub(super) struct Registry(StdMutex<HashMap<String, Entry>>);
impl Registry {
    fn remove(&self, id: &str, generation: Uuid) {
        let mut entries = self.0.lock().expect("workload routes");
        if entries.get(id).is_some_and(|e| e.generation == generation) {
            if let Some(entry) = entries.remove(id) {
                let _ = entry.cancelled.send(true);
            }
        }
    }
    pub(super) fn close_all(&self) {
        for (_, entry) in self.0.lock().expect("workload routes").drain() {
            let _ = entry.cancelled.send(true);
        }
    }
    pub(super) fn disconnect(&self, node: &str, session: Uuid) {
        let mut entries = self.0.lock().expect("workload routes");
        entries.retain(|_, entry| {
            if entry.node == node && entry.session == session {
                let _ = entry.cancelled.send(true);
                false
            } else {
                true
            }
        });
    }
    pub(super) fn route(&self, node: &str, session: Uuid, message: Value) -> Result<()> {
        let Some(id) = message["id"].as_str().filter(|id| id.len() <= 64) else {
            return Ok(());
        };
        if crate::automation_occurrence::uuid(id).is_err() {
            return Ok(());
        }
        let id = id.to_owned();
        let mut entries = self.0.lock().expect("workload routes");
        let Some(entry) = entries
            .get_mut(&id)
            .filter(|e| e.node == node && e.session == session)
        else {
            // Another node/retired generation cannot poison the actual owner.
            return Ok(());
        };
        let result = entry.accept(message);
        if result.is_err() {
            if let Some(entry) = entries.remove(&id) {
                let _ = entry.cancelled.send(true);
            }
        }
        // Overflow/invalid source metadata retires this workload, preserving
        // independent connection control and other ordinary node operations.
        Ok(())
    }
}

fn fields(value: &Value, expected: &[&str]) -> Result<()> {
    let object = value.as_object().context("Invalid workload envelope")?;
    ensure!(
        object.len() == expected.len()
            && object.keys().all(|k| expected.contains(&k.as_str()))
            && value["schema_version"] == 2,
        "Unsupported workload envelope fields/schema"
    );
    Ok(())
}
impl Entry {
    fn accept(&mut self, message: Value) -> Result<()> {
        ensure!(!self.terminal, "Workload terminal frame already received");
        report_queue::encoded_size(&message, CONTROL_BYTES)?;
        match message["type"]
            .as_str()
            .context("Missing workload frame kind")?
        {
            "workload_probe" => {
                fields(&message, &["type", "schema_version", "id", "query"])?;
                let query: Query = serde_json::from_value(message["query"].clone())?;
                crate::automation_occurrence::uuid(&query.id)?;
                ensure!(
                    query.schema_version == 2
                        && query.kind == "workload_lease_probe"
                        && query.sequence > 0
                        && query.contract.digest()?
                            == self.definition.lease_contract()?.digest()?,
                    "Workload probe scope changed"
                );
                self.control.try_send(message)?;
            }
            "workload_ack" | "workload_complete" => {
                fields(&message, &["type", "schema_version", "id", "receipt"])?;
                let receipt: crate::grants::workload::Receipt =
                    serde_json::from_value(message["receipt"].clone())?;
                receipt.validate()?;
                ensure!(
                    receipt.definition == self.definition,
                    "Workload receipt scope changed"
                );
                if message["type"] == "workload_ack" {
                    ensure!(
                        receipt.receipt.state == "accepted"
                            && self.ack.as_ref().is_none_or(|a| *a == message["receipt"]),
                        "Conflicting workload consumption ACK"
                    );
                    self.control.try_send(message.clone())?;
                    self.ack = Some(message["receipt"].clone());
                } else {
                    ensure!(
                        self.ack.is_some()
                            && receipt.receipt.state == "completed"
                            && self.ack.as_ref().is_some_and(|ack| {
                                ack["receipt"]["accepted_at_ms"] == receipt.receipt.accepted_at_ms
                                    && ack["receipt"]["updated_at_ms"]
                                        .as_u64()
                                        .is_some_and(|at| at <= receipt.receipt.updated_at_ms)
                            })
                            && receipt.completion.as_ref().is_some_and(|r| r.event_count
                                == self.last_event.as_ref().map_or(0, |e| e.sequence)),
                        "Completion lacks matching event count/consumed ACK"
                    );
                    self.control.try_send(message)?;
                    self.terminal = true;
                }
            }
            "workload_event" => {
                fields(&message, &["type", "schema_version", "id", "event"])?;
                ensure!(self.ack.is_some(), "Workload event precedes consumed ACK");
                let event: crate::workload_events::Event =
                    serde_json::from_value(message["event"].clone())?;
                event.follows(&self.definition, self.last_event.as_ref())?;
                let bytes = report_queue::encoded_size(
                    &message,
                    self.definition.events.max_event_bytes as usize,
                )?;
                let total = self
                    .event_bytes
                    .checked_add(bytes)
                    .context("Workload event bytes overflow")?;
                ensure!(
                    total <= self.definition.events.max_total_bytes as usize,
                    "Workload transport event budget exhausted"
                );
                let count = self
                    .event_slots
                    .clone()
                    .try_acquire_owned()
                    .context("Workload event pending count exhausted")?;
                self.events.try_send_with_permit(message, Some(count))?;
                self.last_event = Some(event);
                self.event_bytes = total;
            }
            "workload_rejected" => {
                fields(&message, &["type", "schema_version", "id", "code"])?;
                ensure!(
                    matches!(
                        message["code"].as_str(),
                        Some(
                            "policy_denied"
                                | "source_unavailable"
                                | "consumption_unconfirmed"
                                | "source_unconfirmed"
                                | "workload_busy"
                        )
                    ),
                    "Unsupported workload rejection code"
                );
                self.control.try_send(message)?;
                self.terminal = true;
            }
            _ => bail!("Unsupported workload frame"),
        }
        Ok(())
    }
}

pub struct WorkloadLink {
    registry: Arc<Registry>,
    id: String,
    generation: Uuid,
    peer: Arc<WorkloadPeer>,
    contract_sha256: String,
    max_lease_ms: u64,
    max_session_ms: u64,
    sender: mpsc::Sender<Value>,
    control: mpsc::Receiver<report_queue::Envelope>,
    events: mpsc::Receiver<report_queue::Envelope>,
    cancelled: watch::Receiver<bool>,
    deadline: tokio::time::Instant,
    completion: Option<report_queue::Envelope>,
    finished: bool,
    offer: Option<Offer>,
}
impl Drop for WorkloadLink {
    fn drop(&mut self) {
        self.registry.remove(&self.id, self.generation);
    }
}
impl WorkloadLink {
    /// Queue the prepared offer once. The coordinator serializes this call
    /// under its original engine control gate; preparing alone sends nothing.
    pub fn dispatch_offer(&mut self) -> Result<()> {
        ensure!(
            !self.finished
                && !*self.cancelled.borrow()
                && tokio::time::Instant::now() < self.deadline,
            "Workload offer belongs to a retired route"
        );
        let offer = self
            .offer
            .take()
            .context("Workload offer already consumed; no retry")?;
        self.sender
            .try_send(json!(offer))
            .context("Workload offer not queued; no retry")?;
        Ok(())
    }
    pub fn peer(&self) -> &WorkloadPeer {
        &self.peer
    }
    pub async fn next(&mut self) -> Result<WorkloadFrame> {
        loop {
            ensure!(
                !self.finished
                    && !*self.cancelled.borrow()
                    && tokio::time::Instant::now() < self.deadline,
                "Workload connection retired or deadline elapsed"
            );
            if self.completion.is_some() {
                if let Ok(envelope) = self.events.try_recv() {
                    return Ok(WorkloadFrame {
                        envelope,
                        peer: self.peer.clone(),
                    });
                }
                self.finished = true;
                return Ok(WorkloadFrame {
                    envelope: self.completion.take().unwrap(),
                    peer: self.peer.clone(),
                });
            }
            let envelope = tokio::select! {
                biased;
                _ = self.cancelled.changed() => bail!("Workload authenticated connection retired"),
                _ = tokio::time::sleep_until(self.deadline) => bail!("Workload connection deadline elapsed"),
                frame = self.control.recv() => frame.context("Workload control retired")?,
                frame = self.events.recv() => frame.context("Workload events retired")?,
            };
            if envelope.message["type"] == "workload_rejected" {
                // Revocation/failure must reach the original issuer before
                // queued observations. These are not a successful final prefix.
                self.finished = true;
                return Ok(WorkloadFrame {
                    envelope,
                    peer: self.peer.clone(),
                });
            }
            if envelope.message["type"] == "workload_complete" {
                // The TCP reader queued earlier events before this terminal
                // frame. Lease/ACK control bypasses slow events; terminal output
                // cannot discard their already received bounded prefix.
                self.completion = Some(envelope);
                continue;
            }
            return Ok(WorkloadFrame {
                envelope,
                peer: self.peer.clone(),
            });
        }
    }
    pub fn reply_lease(&self, query: &Query, reply: Reply) -> Result<()> {
        ensure!(
            !self.finished
                && !*self.cancelled.borrow()
                && tokio::time::Instant::now() < self.deadline,
            "Workload lease response belongs to a retired connection"
        );
        ensure!(
            query.id == reply.id
                && query.sequence == reply.sequence
                && query.contract.digest()? == self.contract_sha256
                && reply.contract_sha256 == self.contract_sha256
                && reply.kind == "workload_lease"
                && reply.schema_version == 2
                && reply.lease_remaining_ms > 0
                && reply.lease_remaining_ms <= self.max_lease_ms
                && reply.session_remaining_ms > 0
                && reply.session_remaining_ms <= self.max_session_ms
                && reply.lease_remaining_ms <= reply.session_remaining_ms,
            "Workload lease reply correlation changed"
        );
        self.sender
            .try_send(json!(reply))
            .map_err(|_| anyhow::anyhow!("Workload outbound queue unavailable; no retry"))?;
        Ok(())
    }
}

impl NodeServer {
    /// Current trusted connection metadata only; not approval or a lease.
    pub async fn workload_session(
        &self,
        template: &crate::workload_leases::definition::Template,
        freshness_seconds: u64,
    ) -> Result<crate::workload_network::Session> {
        template.validate()?;
        ensure!(
            (1..=300).contains(&freshness_seconds),
            "Invalid workload registration freshness"
        );
        let connections = self.inner.connections.read().await;
        let c = connections
            .get(&template.executor_node)
            .context("Workload node is not connected")?;
        self.check_grant_connection(c)?;
        self.check_lease_connection(c)?;
        ensure!(
            c.last_seen.elapsed() < Duration::from_secs(freshness_seconds),
            "Workload registration is stale"
        );
        let registered = &c
            .state
            .as_ref()
            .context("Workload registration unavailable")?
            .registration;
        let support: Advertisement =
            serde_json::from_value(registered["workload_execution"].clone())?;
        support.confirms_source(&template.source)?;
        let stop: crate::grants::stop::Advertisement =
            serde_json::from_value(registered["grant_stop"].clone())?;
        stop.validate()?;
        ensure!(
            support.available && stop.available && support.completion_payload_prefix,
            "Node does not advertise compatible workload completion and stop support"
        );
        let runtime = support.runtime_id.context("Workload runtime unavailable")?;
        let policy = support.policy.context("Workload policy unavailable")?;
        ensure!(
            registered["registration_receipt"]["runtime_id"] == runtime
                && registered["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps
                        .iter()
                        .any(|v| v == crate::workload_leases::definition::CAPABILITY))
                && template.limits.max_session_ms <= policy.max_session_ms
                && template.limits.max_lease_ms <= policy.max_lease_ms,
            "Workload runtime, capability or policy bounds changed"
        );
        let issuer = self
            .inner
            .authentication
            .tls
            .as_ref()
            .context("Workload issuer TLS unavailable")?
            .fingerprint()?;
        Ok(crate::workload_network::Session {
            node: c.info.node_id.clone(),
            issuer,
            session: c.session_id.to_string(),
            runtime,
            policy,
        })
    }
    /// Callers must first retain reviewed issuance with its original monotonic
    /// authority. This opens only a typed route; it grants no lease by itself.
    pub async fn open_workload(
        &self,
        definition: Definition,
        deadline: tokio::time::Instant,
    ) -> Result<WorkloadLink> {
        let mut link = self.prepare_workload(definition, deadline).await?;
        link.dispatch_offer()?;
        Ok(link)
    }
    /// Prepare a bounded session route without sending an execution offer.
    pub async fn prepare_workload(
        &self,
        definition: Definition,
        deadline: tokio::time::Instant,
    ) -> Result<WorkloadLink> {
        definition.validate()?;
        let now = tokio::time::Instant::now();
        ensure!(
            now < deadline
                && deadline.duration_since(now)
                    <= Duration::from_millis(definition.limits.max_session_ms),
            "Workload offer deadline elapsed"
        );
        let connections = self.inner.connections.read().await;
        let connection = connections
            .get(&definition.executor_node)
            .context("Workload node is not connected")?;
        ensure!(
            connection.credential_transport,
            "Workload execution requires credential transport"
        );
        let issuer = self
            .inner
            .authentication
            .tls
            .as_ref()
            .context("Workload execution requires pinned TLS")?
            .fingerprint()?;
        ensure!(
            definition.scope()?.matches_session(
                &issuer,
                &connection.session_id.to_string(),
                &connection.info.node_id
            ),
            "Workload definition differs from the authenticated connection"
        );
        let registered = &connection
            .state
            .as_ref()
            .context("Workload node has not registered")?
            .registration;
        let support: Advertisement =
            serde_json::from_value(registered["workload_execution"].clone())?;
        support.confirms_source(&definition.source)?;
        ensure!(
            support.available,
            "Node has no authenticated workload execution support; no fallback"
        );
        let runtime = support.runtime_id.as_ref().unwrap();
        ensure!(
            registered["registration_receipt"]["runtime_id"] == *runtime
                && registered["capabilities"]
                    .as_array()
                    .is_some_and(|caps| caps
                        .iter()
                        .any(|c| c == crate::workload_leases::definition::CAPABILITY)),
            "Workload registration generation or capability changed"
        );
        let policy = support.policy.as_ref().unwrap();
        ensure!(
            definition.limits.max_session_ms <= policy.max_session_ms
                && definition.limits.max_lease_ms <= policy.max_lease_ms,
            "Workload definition exceeds advertised node lease bounds"
        );
        let offer = Offer {
            kind: "execute_workload".into(),
            schema_version: 2,
            id: definition.grant_id.clone(),
            expected_runtime_id: runtime.clone(),
            definition: definition.clone(),
        };
        offer.validate(
            &issuer,
            &connection.session_id.to_string(),
            &connection.info.node_id,
            runtime,
        )?;
        let generation = Uuid::new_v4();
        let contract_sha256 = definition.lease_contract()?.digest()?;
        let max_lease_ms = definition.limits.max_lease_ms;
        let max_session_ms = definition.limits.max_session_ms;
        let (control, control_rx) =
            report_queue::Queue::new(CONTROL_COUNT, self.inner.report_bytes.clone());
        let (events, events_rx) = report_queue::Queue::new(
            definition.events.max_pending as usize,
            self.inner.report_bytes.clone(),
        );
        let event_slots = Arc::new(tokio::sync::Semaphore::new(
            definition.events.max_pending as usize,
        ));
        let (cancelled, cancelled_rx) = watch::channel(false);
        let id = definition.grant_id.clone();
        {
            let mut entries = self.inner.workloads.0.lock().expect("workload routes");
            ensure!(
                entries.len() < MAX_SESSION_REPORTS
                    && !entries.contains_key(&id)
                    && !entries.values().any(|e| e.node == connection.info.node_id),
                "Workload routing capacity unavailable; no queued retry"
            );
            entries.insert(
                id.clone(),
                Entry {
                    node: connection.info.node_id.clone(),
                    session: connection.session_id,
                    generation,
                    definition,
                    control,
                    events,
                    event_slots,
                    cancelled,
                    ack: None,
                    last_event: None,
                    event_bytes: 0,
                    terminal: false,
                },
            );
        }
        let link = WorkloadLink {
            registry: self.inner.workloads.clone(),
            id,
            generation,
            peer: Arc::new(WorkloadPeer {
                node: connection.info.node_id.clone(),
                session: connection.session_id.to_string(),
                issuer,
                runtime: runtime.clone(),
            }),
            contract_sha256,
            max_lease_ms,
            max_session_ms,
            sender: connection.sender.clone(),
            control: control_rx,
            events: events_rx,
            cancelled: cancelled_rx,
            deadline,
            completion: None,
            finished: false,
            offer: Some(offer),
        };
        // Keep routing read ownership through route insertion. A later dispatch
        // checks its session cancellation; it never looks up a replacement node.
        Ok(link)
    }
}

#[cfg(test)]
#[path = "server_workloads_tests.rs"]
mod tests;
