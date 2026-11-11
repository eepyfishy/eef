//! Historical interpreter retirement on the existing bounded finalizer. No live
//! continuation, source, model future or renewal handle enters the native queue.
#[cfg(test)]
use super::Pending;
use super::{JobStore, Slot};
use anyhow::Result;
use std::sync::Arc;

impl JobStore {
    pub(crate) fn queue_interpreter_retirement(
        self: &Arc<Self>,
        id: String,
        binding: String,
        slot: Slot,
    ) -> Result<()> {
        if !Arc::ptr_eq(&slot.owner, &self.finalizations.slots) {
            self.deny_workload_reporting();
            anyhow::bail!("Interpreter retirement slot belongs to another journal")
        }
        #[cfg(test)]
        let pending = Pending::new(self.finalizations.pending.clone());
        let store = self.clone();
        let submitted = self.finalizations.submit(Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                store.retire_interpreter_continuation(&id, &binding)
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Interpreter retirement checkpoint panicked")));
            if let Err(error) = result {
                store.deny_workload_reporting();
                tracing::warn!(grant_id=%id,%error,"Interpreter retirement unconfirmed; no retry or stop proof");
            }
            // Keep actual checkpoint capacity after the original registration
            // or async runtime has gone away. Inference charges are unchanged.
            drop(slot);
            drop(store);
            #[cfg(test)]
            drop(pending);
        }));
        if submitted.is_err() {
            self.deny_workload_reporting();
        }
        submitted
    }
}
