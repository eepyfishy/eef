//! The original issuer awaits durable consumption through its existing journal.
//! Native persistence receives exact metadata, never the issuer or its window.
use super::*;

impl Issued {
    /// Accept only the exact report from the original pinned authenticated node
    /// session. Durable ACK and post-write liveness precede renewal authority.
    pub async fn acknowledge(
        &mut self,
        ack: eefn::grants::workload::Receipt,
        deadline: tokio::time::Instant,
    ) -> Result<()> {
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
        let valid = (|| -> Result<()> {
            ack.validate()?;
            ensure!(
                ack.definition == pending.issued.definition && ack.receipt.state == "accepted",
                "Wrong workload ACK scope/state"
            );
            if pending.issued.acknowledged {
                ensure!(
                    saved
                        .execution
                        .as_ref()
                        .unwrap()
                        .workload
                        .as_ref()
                        .unwrap()
                        .acknowledgement
                        .as_ref()
                        .is_some_and(|prior| json!(prior) == json!(ack)),
                    "Conflicting retained workload ACK"
                );
            }
            Ok(())
        })();
        pending.issued.local_live()?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload ACK deadline elapsed during validation"
        );
        if let Err(error) = valid {
            // Wrong receipts do not consume a sequence or retire a healthy issuer.
            pending.retire = false;
            return Err(error);
        }
        if !pending.issued.acknowledged {
            pending
                .issued
                .window
                .check_consumed(&pending.issued.definition.scope()?)?;
            let saved = pending
                .issued
                .store
                .checkpoint_workload_ack_before(pending.issued.definition.clone(), ack, deadline)
                .await?;
            pending.issued.validate_live(&saved)?;
            ensure!(
                tokio::time::Instant::now() < deadline,
                "Workload ACK deadline elapsed after persistence"
            );
            pending
                .issued
                .window
                .acknowledge_consumed(&pending.issued.definition.scope()?)?;
            pending.issued.acknowledged = true;
        }
        pending.issued.local_live()?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload ACK deadline elapsed before confirmation"
        );
        pending.retire = false;
        Ok(())
    }
}
