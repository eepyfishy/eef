//! The coordinator's public stop command keeps journal waits off async workers.
#[cfg(test)]
use super::JobStore;
use crate::task::TaskEngine;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use tokio::{sync::oneshot, time::Instant};

impl TaskEngine {
    pub(crate) async fn stop_job_request(&self, id: &str, origin: Option<&str>) -> Result<Value> {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let plan = self.cancel_before(id, origin, deadline).await?;
        Ok(self.job_view(&plan, true))
    }

    pub(crate) async fn cancel_before(
        &self,
        id: &str,
        origin: Option<&str>,
        deadline: Instant,
    ) -> Result<crate::task::Plan> {
        eefn::job_commands::validate_job_id(id)?;
        if let Some(origin) = origin {
            eefn::network::validate_remote_node_id(origin)?;
        }
        ensure!(
            Instant::now() < deadline,
            "Stop intent deadline elapsed before admission"
        );
        let slot = self
            .store
            .stop_intents
            .slots
            .clone()
            .try_acquire_owned()
            .context("Stop intent capacity unavailable")?;
        let (reply, receiver) = oneshot::channel();
        let engine = self.clone();
        let id = id.to_owned();
        let origin = origin.map(str::to_owned);
        let end = deadline.into_std();
        self.store.stop_intents.submit(Box::new(move || {
            let current = || !reply.is_closed() && std::time::Instant::now() < end;
            let outcome = if current() {
                #[cfg(test)]
                engine
                    .store
                    .stop_intents
                    .started
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    engine.cancel_in_journal(&id, origin.as_deref(), current)
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Stop intent worker failed")))
            } else {
                Err(anyhow::anyhow!("Stop intent abandoned before storage"))
            };
            // Only the native operation can release the actual capacity.
            drop(engine);
            drop(slot);
            let _ = reply.send(outcome);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("Stop intent deadline elapsed. Inspect the job for its saved state")?
            .context("Stop intent worker unavailable. Inspect the job for its saved state")?;
        ensure!(
            Instant::now() < deadline,
            "Stop intent observed after deadline. Inspect the job for its saved state"
        );
        result
    }
}

#[cfg(test)]
impl JobStore {
    pub(crate) fn stop_intent_started(&self) -> usize {
        self.stop_intents
            .started
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn stop_intent_available(&self) -> usize {
        self.stop_intents.slots.available_permits()
    }
}
