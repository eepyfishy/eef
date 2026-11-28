//! Bounded pause/removal on the original journal, with no execution dispatch.
use crate::task::TaskEngine;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};

enum Control {
    Pause,
    Remove,
}

impl TaskEngine {
    pub(crate) async fn control_history_job_request(
        &self,
        kind: &str,
        id: &str,
        origin: Option<&str>,
    ) -> Result<Value> {
        self.history_control_before(
            kind,
            id,
            origin,
            Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
    }

    async fn history_control_before(
        &self,
        kind: &str,
        id: &str,
        origin: Option<&str>,
        deadline: Instant,
    ) -> Result<Value> {
        let control = match kind {
            "jobs.pause" => Control::Pause,
            "jobs.remove" => Control::Remove,
            _ => bail!("History control cannot start, resume or stop execution"),
        };
        eefn::request_drafts::validate_id(id)?;
        if let Some(origin) = origin {
            eefn::network::validate_remote_node_id(origin)?;
        }
        ensure!(
            Instant::now() < deadline,
            "History control deadline elapsed before admission"
        );
        let slot = self
            .store
            .history_controls
            .slots
            .clone()
            .try_acquire_owned()
            .context("History control capacity unavailable; no queued retry")?;
        let (reply, receiver) = oneshot::channel();
        let engine = self.clone();
        let id = id.to_owned();
        let origin = origin.map(str::to_owned);
        let end = deadline.into_std();
        self.store.history_controls.submit(Box::new(move || {
            let current = || !reply.is_closed() && std::time::Instant::now() < end;
            let outcome = if current() {
                #[cfg(test)]
                engine
                    .store
                    .history_controls
                    .started
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match control {
                    Control::Pause => engine
                        .pause_current(&id, origin.as_deref(), current)
                        .map(|plan| engine.job_view(&plan, true)),
                    Control::Remove => engine
                        .store
                        .remove_finished_current(&id, origin.as_deref(), current)
                        .map(|()| json!({"removed":true})),
                }))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("History control worker failed")))
            } else {
                Err(anyhow::anyhow!("History control abandoned before storage"))
            };
            // Cancellation ends observation, not a native operation already
            // entered. Keep its real slot and original journal until it ends.
            drop(engine);
            drop(slot);
            let _ = reply.send(outcome);
        }))?;
        let result = tokio::time::timeout_at(deadline, receiver)
            .await
            .context("History control deadline elapsed. Inspect the job before another command")?
            .context(
                "History control worker unavailable. Inspect the job before another command",
            )?;
        ensure!(
            Instant::now() < deadline,
            "History control observed after deadline. Inspect the job before another command"
        );
        result
    }
}

#[cfg(test)]
impl super::JobStore {
    pub(crate) fn history_control_started(&self) -> usize {
        self.history_controls
            .started
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn history_control_available(&self) -> usize {
        self.history_controls.slots.available_permits()
    }
}

#[cfg(test)]
#[path = "job_history_controls_tests.rs"]
mod tests;
