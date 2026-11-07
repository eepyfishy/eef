//! Versioned passive source readiness. Native inspection receives no source,
//! grant, lease or Tokio runtime handle; control stays responsive while waiting.
use crate::{
    NodeService,
    workload_leases::Policy,
    workload_sources::{Report, Request},
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;
use tokio::sync::{Semaphore, watch};
#[cfg(test)]
#[path = "workload_readiness_tests.rs"]
mod tests;

pub(crate) struct State {
    changes: watch::Sender<u64>,
    capacity: Arc<Semaphore>,
    #[cfg(test)]
    pub(crate) inspecting: std::sync::atomic::AtomicUsize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            changes: watch::channel(0).0,
            capacity: Arc::new(Semaphore::new(1)),
            #[cfg(test)]
            inspecting: Default::default(),
        }
    }
}
impl State {
    pub(crate) fn invalidate(&self) {
        self.changes.send_modify(|v| *v = v.saturating_add(1));
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
    pub(crate) fn version(&self) -> u64 {
        *self.changes.borrow()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Version {
    config: u64,
    listening: (u64, bool),
    camera_listening: (u64, bool),
    workload: (u64, bool),
}
pub(crate) struct Snapshot {
    version: Version,
    pub(crate) report: Report,
    pub(crate) camera_report: crate::camera_sources::Report,
    pub(crate) policy: Policy,
}
impl Snapshot {
    pub(crate) fn supported_sources(
        &self,
        service: &NodeService,
        runtime: &str,
    ) -> Vec<crate::workload_leases::definition::SourceKind> {
        use crate::workload_leases::definition::SourceKind;
        if !self.current(service, runtime) || !self.policy.enabled {
            return Vec::new();
        }
        let mut kinds = Vec::new();
        if self.report.execution_available {
            kinds.push(SourceKind::MicrophoneEnergy);
        }
        if self.camera_report.execution_available {
            kinds.push(SourceKind::CameraMotion);
        }
        kinds
    }
    pub(crate) fn current(&self, service: &NodeService, runtime: &str) -> bool {
        self.version.config < u64::MAX
            && self.version == service.source_version()
            && self.report.runtime_id == runtime
            && self.camera_report.runtime_id == runtime
    }
}

#[cfg(test)]
#[path = "workload_camera_readiness_tests.rs"]
mod camera_tests;
impl NodeService {
    fn source_version(&self) -> Version {
        Version {
            config: *self.source_readiness.changes.borrow(),
            listening: self.listening_gate.snapshot(),
            camera_listening: self.camera_listening_gate.snapshot(),
            workload: self.workload_gate.snapshot(),
        }
    }
    pub(crate) async fn inspect_source_readiness(
        self: Arc<Self>,
        runtime: String,
    ) -> Result<Snapshot> {
        crate::automation_occurrence::uuid(&runtime)?;
        // One actual inspection per service, including across reconnects and
        // after cancellation. Waiting is a connection-owned future, not a queue
        // of native jobs or a blocking Tokio worker.
        let permit = self
            .source_readiness
            .capacity
            .clone()
            .acquire_owned()
            .await
            .context("Source readiness inspection retired")?;
        let version = self.source_version();
        ensure!(
            version.config < u64::MAX,
            "Source readiness version exhausted"
        );
        let (reply, receiver) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("eefn-source-readiness".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<Snapshot> {
                        ensure!(!reply.is_closed(), "Source readiness observer retired");
                        let report: Report =
                            serde_json::from_value(self.workload_sources(Request {
                                schema_version: 2,
                                expected_node_id: self.node_id.clone(),
                                expected_runtime_id: runtime.clone(),
                            })?)?;
                        ensure!(!reply.is_closed(), "Source readiness observer retired");
                        let camera_report =
                            serde_json::from_value(self.camera_workload_sources(Request {
                                schema_version: 2,
                                expected_node_id: self.node_id.clone(),
                                expected_runtime_id: runtime.clone(),
                            })?)?;
                        let policy =
                            self.inspect_grant_configuration(crate::workload_policy::from_config)?;
                        let snapshot = Snapshot {
                            version,
                            report,
                            camera_report,
                            policy,
                        };
                        ensure!(
                            snapshot.current(&self, &runtime) && !reply.is_closed(),
                            "Source readiness changed during inspection"
                        );
                        Ok(snapshot)
                    },
                ))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Source readiness inspection failed")));
                drop(permit);
                drop(self);
                let _ = reply.send(result);
            })
            .context("Source readiness worker unavailable")?;
        receiver.await.context("Source readiness worker retired")?
    }
}

pub(crate) fn source_config_changed(old: &serde_json::Value, next: &serde_json::Value) -> bool {
    old.pointer("/permissions/media/microphone") != next.pointer("/permissions/media/microphone")
        || old.pointer("/permissions/media/camera") != next.pointer("/permissions/media/camera")
        || [
            "metadata",
            "listening",
            "workload_policy",
            "python",
            "python_plugins",
            "python_bridge",
        ]
        .iter()
        .any(|key| old.get(*key) != next.get(*key))
}
