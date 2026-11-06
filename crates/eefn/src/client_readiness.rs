//! One coalesced passive inspection belongs to the live connection. Updates
//! replace registration metadata in place, without restarting work or framing.
use super::*;
use crate::workload_readiness::Snapshot;
use futures_util::future::BoxFuture;

pub(super) struct Readiness {
    service: Option<Arc<crate::NodeService>>,
    runtime: String,
    changes: Option<tokio::sync::watch::Receiver<u64>>,
    pending: Option<BoxFuture<'static, Result<Snapshot>>>,
    requested_version: u64,
    snapshot: Option<Snapshot>,
}
impl Readiness {
    pub(super) fn new(client: &NodeClient) -> Self {
        let service = if client.tls_config.is_some()
            && client.grant_ledger.is_some()
            && client.registration_runtime.is_some()
        {
            client.engine.source_service()
        } else {
            None
        };
        let changes = service.as_ref().map(|s| s.source_readiness.subscribe());
        let mut state = Self {
            service,
            runtime: client.registration_runtime.clone().unwrap_or_default(),
            changes,
            pending: None,
            requested_version: 0,
            snapshot: None,
        };
        state.begin();
        state
    }
    fn begin(&mut self) {
        if let Some(service) = self.service.clone() {
            self.requested_version = service.source_readiness.version();
            self.pending = Some(Box::pin(
                service.inspect_source_readiness(self.runtime.clone()),
            ));
        }
    }
    pub(super) async fn changed(&mut self) {
        tokio::select! {
            changed=async {if let Some(changes)=&mut self.changes {changes.changed().await} else {std::future::pending().await}}=>{
                if changed.is_err() {self.changes=None;}
                self.snapshot=None;
                if self.pending.is_none() {self.begin();}
            }
            result=async {if let Some(pending)=&mut self.pending {pending.await} else {std::future::pending().await}}=>{
                self.pending=None;
                self.snapshot=result.ok().filter(|snapshot|self.service.as_ref().is_some_and(|s|snapshot.current(s,&self.runtime)));
                // Coalesce changes received while an actual worker was busy.
                // An unchanged inspection failure waits for a new owner change.
                if self.snapshot.is_none() && self.service.as_ref().is_some_and(|s|s.source_readiness.version()!=self.requested_version) {self.begin();}
            }
        }
    }
    pub(super) fn decorate(&self, mut message: Value) -> Value {
        let mut advertisement = crate::workload_network::Advertisement::unavailable();
        if let (Some(service), Some(snapshot)) = (&self.service, &self.snapshot) {
            if snapshot.current(service, &self.runtime)
                && snapshot.report.execution_available
                && snapshot.policy.enabled
            {
                advertisement.available = true;
                advertisement.completion_payload_prefix = true;
                advertisement.runtime_id = Some(self.runtime.clone());
                advertisement.policy = Some(snapshot.policy.clone());
            }
        }
        let caps = message["capabilities"]
            .as_array_mut()
            .expect("registration capabilities");
        caps.retain(|v| v != crate::workload_leases::definition::CAPABILITY);
        if advertisement.available {
            caps.push(json!(crate::workload_leases::definition::CAPABILITY));
        }
        message["workload_execution"] = json!(advertisement);
        message
    }
}
