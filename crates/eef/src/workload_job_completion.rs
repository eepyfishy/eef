//! Validated node completion in the existing coordinator job journal. No
//! execution lease is reconstructed from this historical record.
use super::*;
type NodeReceipt = eefn::grants::workload::Receipt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub schema_version: u32,
    pub receipt: eefn::request_receipt::ConsumedReceipt,
    pub report: eefn::grants::workload::CompletionReport,
    pub last_issued_lease_sequence: u64,
    pub recorded_at_ms: u64,
}
impl Completion {
    pub(crate) fn validate_events(
        &self,
        definition: &Definition,
        events: &[eefn::workload_network::Event],
    ) -> Result<()> {
        if !events.is_empty() {
            let summary = eefn::workload_network::summarize_events(definition, events)?;
            ensure!(
                self.report.event_count == summary.count
                    && self.report.event_bytes == summary.bytes
                    && self.report.event_prefix_sha256 == summary.prefix_sha256,
                "Workload completion differs from the retained event prefix"
            );
        }
        Ok(())
    }
    fn node_receipt(&self, definition: &Definition) -> NodeReceipt {
        NodeReceipt {
            schema_version: 3,
            receipt: self.receipt.clone(),
            definition: definition.clone(),
            completion: Some(self.report.clone()),
            execution_authorized: false,
            restart_authorized: false,
        }
    }
    pub(crate) fn validate(
        &self,
        definition: &Definition,
        ack: &NodeReceipt,
        issued_at_ms: u64,
    ) -> Result<()> {
        ack.validate()?;
        self.node_receipt(definition).validate()?;
        ensure!(
            self.schema_version == 1
                && ack.definition == *definition
                && ack.receipt.state == "accepted"
                && self.receipt.state == "completed"
                && self.receipt.accepted_at_ms == ack.receipt.accepted_at_ms
                && self.receipt.updated_at_ms >= ack.receipt.updated_at_ms
                && self.report.recorded_at_ms >= ack.receipt.updated_at_ms
                && self.report.lease_sequence <= self.last_issued_lease_sequence
                && self.recorded_at_ms >= issued_at_ms
                && serde_json::to_vec(self)?.len() <= COMPLETION_RESERVE - 1024,
            "Completed workload differs from its consumed ACK or issued lease history"
        );
        Ok(())
    }
    fn confirms(&self, receipt: &NodeReceipt) -> bool {
        json!(self.node_receipt(&receipt.definition)) == json!(receipt)
    }
}
impl Issued {
    /// Inputs must come from the original pinned authenticated session, never
    /// from the receipt body. This does not authenticate a transport by itself.
    pub async fn complete(
        &mut self,
        receipt: NodeReceipt,
        issuer: &str,
        session: &str,
        node: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Plan> {
        receipt.validate()?;
        ensure!(
            self.definition
                .scope()?
                .matches_session(issuer, session, node)
                && receipt.definition == self.definition
                && receipt.receipt.state == "completed",
            "Workload completion differs from its authenticated session or consumed definition"
        );
        if self.retired {
            let saved = self
                .store
                .get_before(&self.definition.job_id, deadline)
                .await?;
            let completed = saved
                .execution
                .as_ref()
                .and_then(|e| e.workload.as_ref())
                .and_then(|w| w.completion.as_ref());
            ensure!(
                saved.status == PlanStatus::Completed
                    && completed.is_some_and(|old| old.confirms(&receipt)),
                "Retired workload cannot accept new completion or restore an issuer"
            );
            return Ok(saved);
        }
        let deadline = deadline.min(tokio::time::Instant::from_std(self.deadline));
        let mut pending = IssuerOperation {
            issued: self,
            retire: true,
        };
        pending.issued.local_live()?;
        let saved = pending
            .issued
            .store
            .get_before(&pending.issued.definition.job_id, deadline)
            .await?;
        pending.issued.validate_live(&saved)?;
        let candidate = (|| -> Result<Completion> {
            ensure!(
                pending.issued.acknowledged,
                "Completion requires the durable consumed ACK"
            );
            let ack = saved
                .execution
                .as_ref()
                .unwrap()
                .workload
                .as_ref()
                .unwrap()
                .acknowledgement
                .as_ref()
                .context("Completion lacks consumed ACK")?;
            // Validate before closing the original window; malformed/wrong-session
            // data cannot change durable state or manufacture a completion record.
            let report = receipt
                .completion
                .as_ref()
                .context("Missing typed node completion")?;
            let candidate = Completion {
                schema_version: 1,
                receipt: receipt.receipt.clone(),
                report: report.clone(),
                last_issued_lease_sequence: report.lease_sequence,
                recorded_at_ms: eefn::protocol::now_ms().max(saved.updated_at_ms),
            };
            candidate.validate(&pending.issued.definition, ack, pending.issued.issued_wall)?;
            candidate.validate_events(
                &pending.issued.definition,
                &saved
                    .execution
                    .as_ref()
                    .unwrap()
                    .workload
                    .as_ref()
                    .unwrap()
                    .observations,
            )?;
            Ok(candidate)
        })();
        pending.issued.local_live()?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload completion deadline elapsed before window close"
        );
        let mut completed = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                pending.retire = false;
                return Err(error);
            }
        };
        let last = match pending.issued.window.close_after_report(
            &pending.issued.definition.scope()?,
            completed.report.lease_sequence,
        ) {
            Ok(last) => last,
            Err(error) => {
                pending.retire = false;
                return Err(error);
            }
        };
        pending.issued.retired = true;
        completed.last_issued_lease_sequence = last;
        let Some(slot) = pending.issued.final_slot.take() else {
            pending.issued.store.deny_workload_reporting();
            bail!("Workload completion lacks its original reporting reservation")
        };
        let receiver = pending.issued.store.queue_workload_completion(
            pending.issued.definition.clone(),
            completed,
            slot,
        )?;
        // Only observation is timed. Actual historical persistence owns the
        // original reservation after cancellation and cannot issue any lease.
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Workload completion deadline elapsed; inspect retained history")?
            .context("Workload completion worker unavailable; inspect retained history")?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload completion saved after observation deadline"
        );
        result
    }
}
