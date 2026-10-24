//! Bounds execution and separately owned replies, not external effects or CPU/RAM.
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
    work_replies: Arc<Semaphore>,
    control_replies: Arc<Semaphore>,
}

#[must_use]
pub(crate) struct Admission {
    execution: OwnedSemaphorePermit,
    reply: ReplyPermit,
}
#[must_use]
pub(crate) struct ReplyPermit {
    _permit: OwnedSemaphorePermit,
}
impl Admission {
    /// Execution has ended; retain reply ownership through the terminal write.
    pub fn into_reply(self) -> ReplyPermit {
        let (execution, reply) = self.into_parts();
        drop(execution);
        reply
    }
    /// Blocking callbacks keep execution even when their waiter is cancelled.
    /// The waiter retains reply ownership, including when a callback panics.
    pub fn into_parts(self) -> (OwnedSemaphorePermit, ReplyPermit) {
        (self.execution, self.reply)
    }
}

impl RequestBudget {
    pub fn new(limits: &RequestLimits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            work: Arc::new(Semaphore::new(limits.work)),
            control: Arc::new(Semaphore::new(limits.control)),
            // One completed reply per class may still be flushing when its peer
            // submits the next command. Do not borrow reserved control capacity
            // for work replies or queue unadmitted requests.
            work_replies: Arc::new(Semaphore::new(limits.work + 1)),
            control_replies: Arc::new(Semaphore::new(limits.control + 1)),
        })
    }

    pub fn admit(&self, capability: &str, action: &str) -> Option<Admission> {
        // Scheduling allowance only; NodeEngine still performs all policy checks.
        let control = capability == "system.ping"
            || (capability == "node.configure"
                && matches!(
                    action,
                    "status"
                        | "setup_check"
                        | "restart"
                        | "model_lifecycle_status"
                        | "model_lifecycle_cancel"
                        | "model_lifecycle_policy_status"
                        | "permission_status"
                        | "python_policy_status"
                ));
        self.admit_class(control)
    }
    /// Distinct authenticated read-only wire query, not an executable capability.
    pub fn admit_receipt(&self) -> Option<Admission> {
        self.admit_class(true)
    }
    fn admit_class(&self, control: bool) -> Option<Admission> {
        let (execution, replies) = if control {
            (&self.control, &self.control_replies)
        } else {
            (&self.work, &self.work_replies)
        };
        let execution = execution.clone().try_acquire_owned().ok()?;
        let reply = ReplyPermit {
            _permit: replies.clone().try_acquire_owned().ok()?,
        };
        Some(Admission { execution, reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn replies_are_bounded_per_class_and_cannot_borrow_reserved_control_capacity() {
        for work in [1, 3] {
            let budget = RequestBudget::new(&RequestLimits { work, control: 1 }).unwrap();
            let mut replies = Vec::new();
            for _ in 0..work + 1 {
                replies.push(
                    budget
                        .admit("node.configure", "model_command")
                        .unwrap()
                        .into_reply(),
                );
            }
            for _ in 0..8 {
                assert!(
                    budget.admit("node.configure", "model_command").is_none(),
                    "bounded pending replies; failed admissions must not leak slots"
                );
            }
            let first = budget.admit_receipt().unwrap().into_reply();
            let second = budget.admit_receipt().unwrap().into_reply();
            assert!(budget.admit_receipt().is_none());
            drop(first);
            assert!(
                budget.admit_receipt().is_some(),
                "control capacity recovers independently"
            );
            drop(second);
            replies.pop();
            let executing = (0..work)
                .map(|_| budget.admit("llm.infer", "run"))
                .collect::<Vec<_>>();
            assert!(executing[0].is_some());
            assert!(
                executing.iter().skip(1).all(Option::is_none),
                "remaining reply capacity is still bounded"
            );
            drop(executing);
            drop(replies);
            let running = (0..work)
                .map(|_| budget.admit("llm.infer", "run").unwrap())
                .collect::<Vec<_>>();
            assert!(
                budget.admit("llm.infer", "run").is_none(),
                "execution limit is unchanged"
            );
            assert!(budget.admit_receipt().is_some());
            drop(running);
        }
    }

    #[tokio::test]
    async fn blocking_callback_keeps_execution_after_waiter_cancellation() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let (execution, reply) = budget.admit_receipt().unwrap().into_parts();
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let (finished, observed) = tokio::sync::oneshot::channel();
        let waiter = tokio::spawn(async move {
            let _reply = reply;
            let _ = tokio::task::spawn_blocking(move || {
                let _ = entered.send(());
                let _ = gate.recv();
                drop(execution);
                let _ = finished.send(());
            })
            .await;
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), started)
            .await
            .unwrap()
            .unwrap();
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        assert!(
            budget.admit_receipt().is_none(),
            "a detached blocking callback retains execution capacity"
        );
        assert!(budget.admit("llm.infer", "run").is_some());
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), observed)
            .await
            .unwrap()
            .unwrap();
        assert!(budget.admit_receipt().is_some());
    }

    #[tokio::test]
    async fn failed_blocking_callback_returns_execution_but_keeps_its_error_reply_bounded() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let (execution, reply) = budget.admit_receipt().unwrap().into_parts();
        let failed = tokio::task::spawn_blocking(move || {
            let _execution = execution;
            panic!("fixture callback failure; no external effects")
        })
        .await;
        assert!(failed.unwrap_err().is_panic());
        let second_reply = budget.admit_receipt().unwrap().into_reply();
        assert!(
            budget.admit_receipt().is_none(),
            "panic replies still consume bounded reply capacity"
        );
        assert!(budget.admit("llm.infer", "run").is_some());
        drop(second_reply);
        assert!(budget.admit_receipt().is_some());
        drop(reply);
    }
    #[test]
    fn setup_inspection_uses_bounded_control_capacity_not_work_or_setup_mutation_capacity() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let work = budget.admit("llm.infer", "run").unwrap();
        let read = budget.admit("node.configure", "setup_check").unwrap();
        assert!(budget.admit("node.configure", "setup_check").is_none());
        assert!(budget.admit("node.configure", "bootstrap_policy").is_none());
        drop(read);
        assert!(budget.admit("node.configure", "bootstrap_policy").is_none());
        assert!(budget.admit("node.configure", "setup_check").is_some());
        drop(work);
    }
    #[test]
    fn model_lifecycle_inspection_and_cancel_keep_bounded_control_capacity_not_load_unload() {
        let budget = RequestBudget::new(&RequestLimits {
            work: 1,
            control: 1,
        })
        .unwrap();
        let work = budget.admit("llm.infer", "run").unwrap();
        assert!(
            budget
                .admit("node.configure", "model_lifecycle_policy")
                .is_none()
        );
        let policy_status = budget
            .admit("node.configure", "model_lifecycle_policy_status")
            .unwrap();
        assert!(
            budget
                .admit("node.configure", "model_lifecycle_status")
                .is_none()
        );
        drop(policy_status);
        assert!(budget.admit("node.configure", "model_lifecycle").is_none());
        let status = budget
            .admit("node.configure", "model_lifecycle_status")
            .unwrap();
        assert!(
            budget
                .admit("node.configure", "model_lifecycle_cancel")
                .is_none()
        );
        drop(status);
        let cancel = budget
            .admit("node.configure", "model_lifecycle_cancel")
            .unwrap();
        assert!(budget.admit("system.ping", "run").is_none());
        drop(cancel);
        drop(work);
    }

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
            ("node.configure", "permission_command"),
            ("node.configure", "python_policy"),
            ("node.update", "install"),
            ("system.info", "run"),
        ] {
            assert!(budget.admit(capability, action).is_none());
        }
        let status = budget.admit("node.configure", "permission_status").unwrap();
        assert!(
            budget
                .admit("node.configure", "permission_command")
                .is_none()
        );
        drop(status);
        let python_status = budget
            .admit("node.configure", "python_policy_status")
            .unwrap();
        assert!(budget.admit("node.configure", "python_policy").is_none());
        drop(python_status);
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
