//! Node-owned source preparation. No startup, public command or wire launch yet.
use crate::{
    NodeService,
    python::{
        PythonRuntime,
        stream::{ObservedStream, PinnedStreamSource, StreamBounds},
    },
    workload_leases::{Renewals, definition::Definition},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{future::Future, sync::Arc};

/// Neither serializable nor cloneable. Owns a reservation before consumption;
/// preparing it imports no device and opens no source.
pub(crate) struct PreparedMicrophone {
    service: Arc<NodeService>,
    runtime_id: String,
    listening_epoch: u64,
    definition: Definition,
    python: PythonRuntime,
    source: PinnedStreamSource,
    params: Value,
    bounds: StreamBounds,
    reservation: crate::capture_ownership::Reservation,
}
impl NodeService {
    #[allow(dead_code)]
    pub(crate) fn prepare_microphone(
        self: &Arc<Self>,
        runtime_id: &str,
        definition: Definition,
    ) -> Result<PreparedMicrophone> {
        crate::automation_occurrence::uuid(runtime_id)?;
        definition.validate()?;
        ensure!(
            definition.executor_node == self.node_id,
            "Source belongs to another node"
        );
        self.with_permission_runtime(runtime_id, |engine| {
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == runtime_id,
                    "Source preparation runtime changed"
                );
                let saved: crate::context::NodeMetadata =
                    serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
                saved.validate()?;
                ensure!(
                    json!(saved.resources) == json!(engine.applied_resources()),
                    "Source bindings changed; restart and review required"
                );
                ensure!(
                    crate::listening::Consent::from_config(config)?.enabled
                        && config.pointer("/permissions/media/microphone") == Some(&json!(true))
                        && engine.applied_permissions().media.microphone,
                    "Source requires local listening consent and microphone permission"
                );
                let (listening_epoch, blocked) = self.listening_gate.snapshot();
                ensure!(!blocked, "Listening consent revoked in this runtime");
                let runtime = engine.microphone_runtime(&self.node_id, &self.microphone_pool)?;
                let mut limits = runtime.limits().clone();
                limits.max_job_memory_mib = Some(
                    limits
                        .max_job_memory_mib
                        .map_or(definition.limits.max_job_memory_mib, |local| {
                            local.min(definition.limits.max_job_memory_mib)
                        }),
                );
                crate::model_memory::supported(limits.max_job_memory_mib)?;
                let python = runtime.as_ref().clone().with_limits(limits)?;
                let resource_id = match &definition.source {
                    crate::workload_leases::definition::Source::MicrophoneEnergy {
                        resource_id,
                        ..
                    } => resource_id,
                };
                let resource = engine
                    .applied_resources()
                    .iter()
                    .find(|r| crate::context::resource_id(&self.node_id, &r.id) == *resource_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Reviewed microphone resource is unavailable")
                    })?;
                let bytes = crate::workload_sources::MICROPHONE_SOURCE;
                let params = definition.bound_python_parameters(resource, bytes)?;
                let adapter = match &definition.source {
                    crate::workload_leases::definition::Source::MicrophoneEnergy {
                        adapter_sha256,
                        ..
                    } => adapter_sha256,
                };
                let source = PinnedStreamSource::new(bytes, adapter)?;
                let blocks = match &definition.source {
                    crate::workload_leases::definition::Source::MicrophoneEnergy {
                        parameters,
                        ..
                    } => parameters.max_blocks,
                };
                let bounds = StreamBounds {
                    max_frame_bytes: 1024,
                    max_stdout_bytes: python.limits().max_stdout_bytes.min(4 * 1024 * 1024),
                    max_observations: u64::from(blocks),
                    max_source_frames: definition.limits.max_source_frames,
                };
                bounds.validate(python.limits())?;
                let reservation = self.microphone_pool.reserve()?;
                Ok(PreparedMicrophone {
                    service: self.clone(),
                    runtime_id: runtime_id.into(),
                    listening_epoch,
                    definition,
                    python,
                    source,
                    params,
                    bounds,
                    reservation,
                })
            })
        })
    }
}
impl PreparedMicrophone {
    #[allow(dead_code)]
    pub(crate) async fn run<F, C>(
        self,
        consumed: crate::grants::workload::Consumption,
        renewals: Renewals,
        observe: F,
    ) -> Result<ObservedStream>
    where
        F: FnMut(crate::workload_events::Event) -> C + Send,
        C: Future<Output = Result<()>> + Send,
    {
        let receipt = consumed.acknowledgement()?;
        ensure!(
            receipt.definition == self.definition,
            "Prepared source differs from consumed definition"
        );
        let current: crate::workload_sources::Report = serde_json::from_value(
            self.service
                .workload_sources(crate::workload_sources::Request {
                    schema_version: 2,
                    expected_node_id: self.definition.executor_node.clone(),
                    expected_runtime_id: self.runtime_id.clone(),
                })?,
        )?;
        current.confirms_template(&crate::workload_leases::definition::Template {
            schema_version: 2,
            executor_node: self.definition.executor_node.clone(),
            priority: self.definition.priority,
            source: self.definition.source.clone(),
            limits: self.definition.limits.clone(),
            events: self.definition.events.clone(),
        })?;
        let deadline = tokio::time::Instant::from_std(consumed.session_deadline()?);
        let mut events = crate::workload_events::Filter::new(&self.definition)?;
        let reporting = consumed.reporting_authority();
        let reporting_contract = self.definition.lease_contract()?;
        let writer = Arc::new(consumed.event_writer()?);
        let observer = Arc::new(tokio::sync::Mutex::new(observe));
        let operation = self.python.observe_reserved_microphone(
            &self.source,
            self.params,
            &self.definition.grant_id,
            self.bounds,
            deadline,
            self.reservation,
            move |observation| {
                let pending = events
                    .observe(observation, std::time::Instant::now())
                    .and_then(|event| {
                        event
                            .map(|event| {
                                let checkpoint =
                                    Arc::new(reporting.checkpoint(&reporting_contract)?);
                                Ok((checkpoint, event))
                            })
                            .transpose()
                    });
                let contract = reporting_contract.clone();
                let writer = writer.clone();
                let observer = observer.clone();
                async move {
                    // read_frames awaits this future before reading another
                    // observation. At most one event is pending, tighter than
                    // every valid reviewed max_pending; no detached worker or
                    // offline queue can outlive the lease/listening supervisor.
                    if let Some((checkpoint, event)) = pending? {
                        let authority = checkpoint.clone();
                        checkpoint
                            .supervise(&contract, async move {
                                writer.append(event.clone(), authority).await?;
                                // Construct the callback only AFTER durable event
                                // persistence; FnMut itself may have sync effects.
                                let delivery = {
                                    let mut observer = observer.lock().await;
                                    (&mut *observer)(event)
                                };
                                delivery.await
                            })
                            .await?;
                    }
                    Ok(())
                }
            },
        );
        consumed
            .supervise(
                renewals,
                self.service.supervise_listening_epoch(
                    &self.runtime_id,
                    deadline,
                    Some(self.listening_epoch),
                    operation,
                ),
            )
            .await
    }
}

#[cfg(all(test, windows))]
#[path = "workload_runtime_tests.rs"]
mod tests;
