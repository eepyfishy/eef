//! Bounded pre-source ACK inspection. Only the caller retains the original
//! consumption/proof; a native reader receives exact scope and journal metadata.
use super::*;
use std::sync::atomic::Ordering;

struct Checking<'a> {
    consumed: &'a mut Consumption,
    close: bool,
}
impl Drop for Checking<'_> {
    fn drop(&mut self) {
        if self.close {
            // Close authority immediately, without moving or dropping a source.
            // Running's Drop destroys its source BEFORE Consumption's original
            // reserved finalization. Keep that ordering even during this await.
            self.consumed.reporting.retire();
            self.consumed.initial.take();
        }
    }
}
impl Consumption {
    pub(super) fn check_initial_ack(&self) -> Result<Instant> {
        ensure!(
            !self.ledger.final_reporting_failed.load(Ordering::Acquire),
            "Workload reporting failed; ACK authority denied"
        );
        let deadline = self
            .initial
            .as_ref()
            .context("Initial workload authority already transferred or retired")?
            .initial_deadline(&self.contract, &self.policy)?;
        ensure!(
            !*self.stop.receiver.borrow(),
            "Workload stop already requested"
        );
        Ok(deadline)
    }
    pub(crate) async fn acknowledgement_before(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<Receipt> {
        let mut checking = Checking {
            consumed: self,
            close: true,
        };
        let end = deadline.min(tokio::time::Instant::from_std(
            checking.consumed.check_initial_ack()?,
        ));
        ensure!(
            tokio::time::Instant::now() < end,
            "Workload ACK observation deadline elapsed"
        );
        let ledger = checking.consumed.ledger.clone();
        // Share the existing single event-worker budget. This read happens
        // before source polling; its actual ownership survives waiter loss.
        let slot = ledger
            .event_workers
            .clone()
            .try_acquire_owned()
            .context("Workload ACK read capacity unavailable; no queued retry")?;
        let scope = checking.consumed.contract.scope.clone();
        let native = ledger.clone();
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let native_end = end.into_std();
        ledger.submit_workload_job(Box::new(move || {
            if reply.is_closed() || Instant::now() >= native_end {
                return;
            }
            #[cfg(test)]
            native.ack_reads_started.fetch_add(1, Ordering::SeqCst);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                native.workload_receipt(&scope.issuer_id, &scope.grant_id)
            }))
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Workload ACK read failed; evidence retained"
                ))
            });
            drop(slot);
            drop(native);
            let _ = reply.send(result);
        }))?;
        let receipt = tokio::time::timeout_at(end, receiver)
            .await
            .context("Workload ACK observation deadline elapsed; no source authority")?
            .context("Workload ACK reader unavailable; no fallback")??;
        checking.consumed.check_initial_ack()?;
        ensure!(
            tokio::time::Instant::now() < end
                && receipt.receipt.state == "accepted"
                && receipt.definition.scope()? == checking.consumed.contract.scope,
            "Workload ACK changed or completed outside its original deadline"
        );
        checking.close = false;
        Ok(receipt)
    }
}
