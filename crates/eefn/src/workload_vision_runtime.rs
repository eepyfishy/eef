//! Admitted vision source: one camera, durable intent before each local model
//! call, original lease/current owner controls and observed source completion.
use super::*;
use crate::{NodeEngine, grants::GrantLimits, workload_leases::definition::Source};
use anyhow::{Context, bail};
use tokio::time::Instant;

pub(super) struct PreparedVision {
    pub(super) engine: Arc<NodeEngine>,
    pub(super) bounds: crate::python::vision::VisionBounds,
    pub(super) policy_epoch: u64,
    pub(super) source_version: u64,
}
impl NodeService {
    fn check_vision_current(
        &self,
        runtime: &str,
        d: &Definition,
        prepared: &PreparedVision,
        listening_epoch: u64,
    ) -> Result<()> {
        ensure!(
            self.source_readiness.version() == prepared.source_version
                && self.workload_gate.snapshot() == (prepared.policy_epoch, false)
                && self.camera_listening_gate.snapshot() == (listening_epoch, false),
            "Prepared vision source configuration or permission changed"
        );
        self.with_permission_runtime(runtime, |engine| {
            ensure!(
                std::ptr::eq(engine, prepared.engine.as_ref()),
                "Vision engine changed"
            );
            self.inspect_grant_configuration(|config| {
                ensure!(
                    config["node_id"] == self.node_id
                        && self.live.lock().unwrap()["runtime_id"] == runtime,
                    "Vision runtime changed"
                );
                crate::workload_policy::from_config(config)?.permits_definition(d)?;
                ensure!(
                    crate::listening::Consent::from_config(config)?.enabled
                        && config.pointer("/permissions/media/camera") == Some(&json!(true))
                        && engine.applied_permissions().media.camera,
                    "Vision camera consent or permission was revoked"
                );
                let saved: crate::context::NodeMetadata =
                    serde_json::from_value(config.get("metadata").cloned().unwrap_or(json!({})))?;
                saved.validate()?;
                ensure!(
                    json!(saved.resources) == json!(engine.applied_resources()),
                    "Vision resource binding changed"
                );
                Ok(())
            })
        })?;
        ensure!(
            self.source_readiness.version() == prepared.source_version
                && self.workload_gate.snapshot() == (prepared.policy_epoch, false),
            "Vision configuration changed during admission check"
        );
        Ok(())
    }
}
impl PreparedSource {
    pub(super) async fn run_vision<F, C>(
        self,
        mut consumed: crate::grants::workload::Consumption,
        renewals: Renewals,
        observe: F,
    ) -> Result<crate::grants::workload::Receipt>
    where
        F: FnMut(crate::workload_events::Event) -> C + Send,
        C: Future<Output = Result<()>> + Send,
    {
        let prepared = self
            .vision
            .as_ref()
            .context("Prepared vision scope unavailable")?;
        let Source::CameraVision { parameters, .. } = &self.definition.source else {
            bail!("Vision runtime requires its exact source")
        };
        let parameters = Arc::new(parameters.clone());
        let deadline = Instant::from_std(consumed.session_deadline()?);
        self.service.check_vision_current(
            &self.runtime_id,
            &self.definition,
            prepared,
            self.listening_epoch,
        )?;
        let (_, policy_permit) = self
            .service
            .workload_policy_permit(&self.runtime_id, &self.definition)?;
        let receipt = Box::pin(consumed.acknowledgement_before(deadline)).await?;
        ensure!(
            receipt.definition == self.definition,
            "Vision differs from consumed ACK"
        );
        self.service.check_vision_current(
            &self.runtime_id,
            &self.definition,
            prepared,
            self.listening_epoch,
        )?;
        let mut changes = self.service.source_readiness.subscribe();
        ensure!(
            *changes.borrow() == prepared.source_version,
            "Prepared vision configuration changed before capture"
        );
        let filter = Arc::new(std::sync::Mutex::new(crate::workload_events::Filter::new(
            &self.definition,
        )?));
        let final_filter = filter.clone();
        let writer = Arc::new(consumed.event_writer_from_ack(receipt)?);
        let reporting = consumed.reporting_authority();
        let contract = self.definition.lease_contract()?;
        let definition = Arc::new(self.definition.clone());
        let service = self.service.clone();
        let runtime = self.runtime_id.clone();
        let listening_epoch = self.listening_epoch;
        let observer = Arc::new(tokio::sync::Mutex::new(observe));
        let (reservation, cancelled_exit) = consumed.observe_capture_stop(self.reservation);
        let operation = Box::pin(self.python.observe_reserved_vision(
            &self.source,
            self.params,
            &self.definition.grant_id,
            prepared.bounds.clone(),
            deadline,
            reservation,
            move |observation| {
                let parameters = parameters.clone();
                let writer = writer.clone();
                let reporting = reporting.clone();
                let contract = contract.clone();
                let service = service.clone();
                let runtime = runtime.clone();
                let definition = definition.clone();
                let filter = filter.clone();
                let observer = observer.clone();
                async move {
                    service.check_vision_current(
                        &runtime,
                        &definition,
                        prepared,
                        listening_epoch,
                    )?;
                    let event = filter
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Vision filter unavailable"))?
                        .observe(observation.motion.clone(), std::time::Instant::now())?;
                    if let Some(event) = event {
                        let checkpoint = Arc::new(reporting.checkpoint(&contract)?);
                        writer.append(event.clone(), checkpoint.clone()).await?;
                        checkpoint
                            .supervise(&contract, async {
                                let delivery = {
                                    let mut observer = observer.lock().await;
                                    (&mut *observer)(event)
                                };
                                delivery.await
                            })
                            .await?;
                    }
                    if let Some(image) = observation.image {
                        let checkpoint = Arc::new(reporting.checkpoint(&contract)?);
                        let permit = writer
                            .begin_analysis(observation.motion.sequence, &image, checkpoint.clone())
                            .await?;
                        // Recheck after actual persistence; native I/O cannot
                        // extend the original model authorization window.
                        service.check_vision_current(
                            &runtime,
                            &definition,
                            prepared,
                            listening_epoch,
                        )?;
                        checkpoint.check(&contract)?;
                        let binding = prepared.engine.bind_local_vision(&parameters.model)?;
                        let limits = GrantLimits {
                            max_execution_ms: parameters.inference.timeout_ms,
                            max_result_bytes: parameters.inference.max_result_bytes,
                        };
                        let call = Box::pin(binding.analyze(
                            &image,
                            &parameters.prompt,
                            &parameters.inference,
                            &limits,
                            deadline.min(Instant::from_std(checkpoint.deadline())),
                            || {
                                checkpoint.check(&contract)?;
                                service.check_vision_current(
                                    &runtime,
                                    &definition,
                                    prepared,
                                    listening_epoch,
                                )
                            },
                        ));
                        let text = checkpoint.supervise(&contract, call).await?;
                        service.check_vision_current(
                            &runtime,
                            &definition,
                            prepared,
                            listening_epoch,
                        )?;
                        writer
                            .finish_analysis(
                                permit,
                                text,
                                Arc::new(reporting.checkpoint(&contract)?),
                            )
                            .await?;
                    }
                    Ok(())
                }
            },
        ));
        let operation = Box::pin(async move {
            let source = operation.await?;
            let events = final_filter
                .lock()
                .map_err(|_| anyhow::anyhow!("Vision filter unavailable"))?
                .seal(&source.summary().stream)?;
            Ok(ObservedWorkload {
                source: ObservedSource::Vision(source),
                events,
            })
        });
        let version = prepared.source_version;
        let service = self.service.clone();
        let versioned = async {
            tokio::select! {
                biased;
                changed=changes.changed()=>{changed.context("Vision configuration control retired")?;bail!("Vision source configuration changed")},
                result=operation=>{ensure!(service.source_readiness.version()==version,"Vision changed before source completion");result}
            }
        };
        let controlled = Box::pin(self.service.supervise_camera_listening_epoch(
            &self.runtime_id,
            deadline,
            Some(self.listening_epoch),
            Box::pin(versioned),
        ));
        Box::pin(
            policy_permit.supervise(Box::pin(consumed.supervise_observed(
                renewals,
                controlled,
                cancelled_exit,
            ))),
        )
        .await
    }
}

#[cfg(all(test, windows, feature = "native-test-fixtures"))]
#[path = "workload_vision_runtime_native_tests.rs"]
mod tests;
