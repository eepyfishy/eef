//! Bounds admitted network request futures, not external side effects or CPU/RAM.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RequestLimits {
    pub work: usize,
    pub control: usize,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self {
            work: 16,
            control: 2,
        }
    }
}

impl RequestLimits {
    pub fn validate(&self) -> Result<()> {
        if !(1..=128).contains(&self.work) || !(1..=8).contains(&self.control) {
            bail!("request_limits.work must be 1-128 and request_limits.control must be 1-8")
        }
        Ok(())
    }
}

pub(crate) struct RequestBudget {
    work: Arc<Semaphore>,
    control: Arc<Semaphore>,
}

impl RequestBudget {
    pub fn new(limits: &RequestLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            work: Arc::new(Semaphore::new(limits.work)),
            control: Arc::new(Semaphore::new(limits.control)),
        })
    }

    pub fn admit(&self, capability: &str, action: &str) -> Option<OwnedSemaphorePermit> {
        // Scheduling allowance only; NodeEngine still performs all policy checks.
        let control = capability == "system.ping"
            || (capability == "node.configure" && matches!(action, "status" | "restart"));
        let pool = if control { &self.control } else { &self.work };
        pool.clone().try_acquire_owned().ok()
    }
    /// Distinct authenticated read-only wire query, not an executable capability.
    pub fn admit_receipt(&self) -> Option<OwnedSemaphorePermit> {
        self.control.clone().try_acquire_owned().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limits_are_compatible_bounded_and_strict() {
        let defaults: RequestLimits = serde_json::from_value(json!({})).unwrap();
        assert_eq!(defaults.work, 16);
        assert_eq!(defaults.control, 2);
        for value in [
            json!({"work":0}),
            json!({"work":129}),
            json!({"control":0}),
            json!({"control":9}),
        ] {
            assert!(
                serde_json::from_value::<RequestLimits>(value)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for value in [
            json!(null),
            json!({"work":-1}),
            json!({"work":1.5}),
            json!({"work":"2"}),
            json!({"unlimited":true}),
        ] {
            assert!(serde_json::from_value::<RequestLimits>(value).is_err());
        }
    }

    #[test]
    fn workload_exhaustion_keeps_bounded_controls_and_releases_on_drop() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let work = budget.admit("llm.infer", "run").unwrap();
        for (capability, action) in [
            ("http.request", "send"),
            ("node.configure", "install"),
            ("node.configure", "save"),
            ("node.update", "install"),
            ("system.info", "run"),
        ] {
            assert!(budget.admit(capability, action).is_none());
        }
        let ping = budget.admit("system.ping", "run").unwrap();
        assert!(budget.admit_receipt().is_none());
        assert!(budget.admit("node.configure", "restart").is_none());
        drop(ping);
        let receipt = budget.admit_receipt().unwrap();
        assert!(budget.admit("node.configure", "status").is_none());
        assert!(
            budget.admit("grants.receipt", "read").is_none(),
            "ordinary RPC cannot claim the distinct control path"
        );
        drop(receipt);
        let restart = budget.admit("node.configure", "restart").unwrap();
        assert!(budget.admit("node.configure", "status").is_none());
        drop(work);
        assert!(budget.admit("http.request", "send").is_some());
        drop(restart);
        assert!(budget.admit("node.configure", "status").is_some());
    }

    #[tokio::test]
    async fn cancelled_request_returns_its_shared_runtime_permit() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let permit = budget.admit("http.request", "send").unwrap();
        let (entered, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let _ = entered.send(());
            std::future::pending::<()>().await;
        });
        started.await.unwrap();
        assert!(budget.admit("llm.infer", "run").is_none());
        assert!(budget.admit("system.ping", "run").is_some());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(budget.admit("llm.infer", "run").is_some());
    }
}
