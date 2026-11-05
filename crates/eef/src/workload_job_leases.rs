//! Lease inspection uses the existing bounded journal reader. Only the original
//! caller owns renewal authority; a native read receives no issuer or window.
use super::*;

struct Probe<'a> {
    issued: &'a mut Issued,
    retire: bool,
}
impl Drop for Probe<'_> {
    fn drop(&mut self) {
        if self.retire {
            self.issued.retire();
        }
    }
}

impl Issued {
    /// Identities must come from the pinned authenticated peer. The caller's
    /// original probe deadline also bounds journal admission and observation.
    /// Cancelling this future after polling permanently closes this issuer.
    pub async fn lease(
        &mut self,
        query: &Query,
        issuer: &str,
        session: &str,
        node: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Reply> {
        let deadline = deadline.min(tokio::time::Instant::from_std(self.deadline));
        let mut probe = Probe {
            issued: self,
            retire: true,
        };
        probe.issued.local_live()?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload probe deadline elapsed before journal admission"
        );
        let saved = probe
            .issued
            .store
            .get_before(&probe.issued.definition.job_id, deadline)
            .await?;
        probe.issued.validate_live(&saved)?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload probe deadline elapsed before lease reply"
        );
        // No await follows this snapshot. Compute the remaining lease AFTER
        // the journal wait, never retain a pre-wait reply or refresh its budget.
        let reply = probe.issued.window.reply(query, issuer, session, node);
        probe.issued.local_live()?;
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Workload probe deadline elapsed after lease validation"
        );
        // A malformed/out-of-sequence probe does not retire a healthy issuer
        // or consume a sequence. Storage/deadline failure and cancellation do.
        probe.retire = false;
        reply
    }
}
