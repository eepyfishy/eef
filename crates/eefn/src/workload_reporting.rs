//! Read-only reporting lifetime of one consumed runner. This is not source,
//! renewal, restart, completion or network delivery authority.
use super::{Contract, Window};
use anyhow::{Context, Result, ensure};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

struct State {
    contract_sha256: String,
    started: bool,
    retired: bool,
    sequence: u64,
    deadline: Option<Instant>,
}
#[derive(Clone)]
pub(crate) struct ReportingAuthority(Arc<Mutex<State>>);

/// A single checkpoint's captured window never moves later on renewal. A
/// blocking worker must check before/after writing and its async waiter must
/// use this deadline; a late save cannot acknowledge or revive source work.
pub(crate) struct Checkpoint {
    authority: ReportingAuthority,
    contract_sha256: String,
    sequence: u64,
    deadline: Instant,
}
impl ReportingAuthority {
    pub(crate) fn new(contract: &Contract) -> Result<Self> {
        Ok(Self(Arc::new(Mutex::new(State {
            contract_sha256: contract.digest()?,
            started: false,
            retired: false,
            sequence: 0,
            deadline: None,
        }))))
    }
    pub(crate) fn checkpoint(&self, contract: &Contract) -> Result<Checkpoint> {
        let digest = contract.digest()?;
        let state = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Reporting control unavailable"))?;
        ensure!(
            state.contract_sha256 == digest && state.started && !state.retired,
            "Reporting runner is not active for this contract"
        );
        let deadline = state
            .deadline
            .ok_or_else(|| anyhow::anyhow!("Reporting window unavailable"))?;
        ensure!(Instant::now() < deadline, "Reporting lease expired");
        Ok(Checkpoint {
            authority: self.clone(),
            contract_sha256: digest,
            sequence: state.sequence,
            deadline,
        })
    }
    pub(crate) fn retire(&self) {
        // A poisoned control is already unusable to every reader.
        if let Ok(mut state) = self.0.lock() {
            state.retired = true;
            state.deadline = None;
        }
    }
    pub(super) fn guard(self) -> Guard {
        Guard(self)
    }
}
impl Checkpoint {
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }
    pub(crate) fn check(&self, contract: &Contract) -> Result<()> {
        ensure!(
            contract.digest()? == self.contract_sha256,
            "Reporting checkpoint scope changed"
        );
        let state = self
            .authority
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Reporting control unavailable"))?;
        ensure!(
            state.contract_sha256 == self.contract_sha256
                && state.started
                && !state.retired
                && state.sequence >= self.sequence()
                && state
                    .deadline
                    .is_some_and(|current| Instant::now() < current.min(self.deadline)),
            "Reporting checkpoint expired or runner retired"
        );
        Ok(())
    }
    pub(crate) async fn supervise<F, T>(&self, contract: &Contract, operation: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        self.check(contract)?;
        let result =
            tokio::time::timeout_at(tokio::time::Instant::from_std(self.deadline()), operation)
                .await
                .context("Workload event reporting window expired")??;
        self.check(contract)?;
        Ok(result)
    }
}
pub(super) struct Guard(ReportingAuthority);
impl Guard {
    pub(super) fn bind(&self, window: &Window) -> Result<()> {
        {
            let mut state = self
                .0
                .0
                .lock()
                .map_err(|_| anyhow::anyhow!("Reporting control unavailable"))?;
            ensure!(
                state.contract_sha256 == window.digest && !state.started && !state.retired,
                "Reporting lifetime cannot bind another runner"
            );
            window.live()?;
            state.started = true;
            state.sequence = window.sequence;
            state.deadline = Some(window.deadline.min(window.session_deadline));
        }
        Ok(())
    }
    pub(super) fn advance(&self, window: &Window) -> Result<()> {
        let mut state = self
            .0
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Reporting control unavailable"))?;
        ensure!(
            state.contract_sha256 == window.digest
                && state.started
                && !state.retired
                && state.sequence.checked_add(1) == Some(window.sequence),
            "Reporting renewal scope or lifecycle changed"
        );
        window.live()?;
        state.sequence = window.sequence;
        state.deadline = Some(window.deadline.min(window.session_deadline));
        Ok(())
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.retire();
    }
}
