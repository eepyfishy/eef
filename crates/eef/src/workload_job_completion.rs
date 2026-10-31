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
    pub(super) fn validate(
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
    pub fn complete(
        &mut self,
        receipt: NodeReceipt,
        issuer: &str,
        session: &str,
        node: &str,
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
            let saved = self.store.get(&self.definition.job_id)?;
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
        let saved = self.checked_live()?;
        ensure!(
            self.acknowledged,
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
        candidate.validate(&self.definition, ack, self.issued_wall)?;
        let last = self
            .window
            .close_after_report(&self.definition.scope()?, report.lease_sequence)?;
        self.retired = true;
        let mut completed = candidate;
        completed.last_issued_lease_sequence = last;
        let result = self.store.change(
            &self.definition.job_id,
            "Authenticated node source completion saved; no execution or replay authority",
            |plan| {
                let execution = plan
                    .execution
                    .as_mut()
                    .context("Missing workload execution")?;
                let w = execution
                    .workload
                    .as_mut()
                    .context("Missing workload issuance")?;
                ensure!(
                    plan.status == PlanStatus::Running
                        && w.state == AttemptState::Accepted
                        && w.definition == self.definition
                        && w.completion.is_none(),
                    "Workload changed before completion checkpoint"
                );
                completed.validate(
                    &w.definition,
                    w.acknowledgement
                        .as_ref()
                        .context("Completion lacks consumed ACK")?,
                    w.issued_at_ms,
                )?;
                plan.updated_at_ms = plan.updated_at_ms.max(completed.recorded_at_ms);
                w.completion = Some(completed);
                w.state = AttemptState::Completed;
                execution.schema_version = 3;
                plan.status = PlanStatus::Completed;
                plan.tasks[0].status = TaskStatus::Completed;
                Ok(())
            },
        );
        if result.is_err() {
            // Completion storage failure cannot revive renewals or rerun work.
            // One uncertainty checkpoint follows; Drop never retries it.
            let _ = mark_unknown(&self.store, &self.definition);
        }
        result
    }
}
