//! Core service exchange and live source checkpoint, entered through the
//! authenticated interpreter broker. Retained history never recreates a completion.
#[path = "interpreter_application.rs"]
mod application;
use crate::{
    Runtime,
    interpreter_allowance::{Participants, PolicyPin, Record, Reservation, Reserved},
    jobs::JobStore,
};
use anyhow::{Context, Result, ensure};
use eefn::{
    NodeEvent,
    grants::{GrantObserver, GrantReport},
    interpreter_task::Task,
};
use serde_json::json;
use std::sync::Arc;

pub(crate) enum Exchange {
    Historical(Box<Record>),
    Failed(Box<Record>),
    Completed(Completion),
}
/// Intentionally not Clone or serializable. Dropping before source promotion
/// retires ownership, including a successful reply whose promotion was lost.
pub(crate) struct Completion {
    record: Record,
    continuation: Continuation,
}
impl Completion {
    pub fn record(&self) -> &Record {
        &self.record
    }
}
struct Continuation {
    store: Arc<JobStore>,
    reserved: Reserved,
    _capacity: crate::capability::ProviderReservation,
    active: Option<crate::task::WorkloadJob>,
    retirement: Option<crate::jobs::WorkloadFinalSlot>,
}
impl Continuation {
    fn registration(&self) -> Result<&crate::task::WorkloadJob> {
        self.active.as_ref().context("Interpreter lifetime retired")
    }
    fn check(&self) -> Result<()> {
        self.store.require_workload_reporting()?;
        self.registration()?
            .check(&self.reserved.record.binding.grant.job_id)
    }
}
impl Drop for Continuation {
    fn drop(&mut self) {
        // This guard's drop is in-memory retirement only. Generic ActiveJob
        // drop also waits on SQLite and must not be used by this path.
        drop(self.active.take());
        match self.retirement.take() {
            Some(slot) => {
                if let Err(error) = self.store.queue_interpreter_retirement(
                    self.reserved.id().into(),
                    self.reserved.record.binding_sha256.clone(),
                    slot,
                ) {
                    tracing::warn!(grant_id=%self.reserved.id(),%error,"Interpreter continuation checkpoint unavailable; no replay or stop proof");
                }
            }
            None => self.store.deny_workload_reporting(),
        }
    }
}
struct Observer {
    store: Arc<JobStore>,
    id: String,
}
impl GrantObserver for Observer {
    fn record(&self, report: &GrantReport) -> Result<()> {
        self.store.interpreter_report(&self.id, report.clone())?;
        Ok(())
    }
}

impl Runtime {
    /// Internal exchange: the caller must establish its authenticated origin
    /// and build the exact task from the retained source.
    pub(crate) async fn exchange_interpreter(
        &self,
        task: Task,
        pin: PolicyPin,
        participants: &Participants,
    ) -> Result<Exchange> {
        self.exchange_interpreter_with_ceiling(task, pin, participants, None)
            .await
    }
    pub(crate) async fn exchange_interpreter_with_ceiling(
        &self,
        task: Task,
        pin: PolicyPin,
        participants: &Participants,
        maximum_execution_ms: Option<u64>,
    ) -> Result<Exchange> {
        ensure!(
            maximum_execution_ms.is_none_or(|v| (1..=eefn::grants::MAX_DURATION_MS).contains(&v)),
            "Invalid caller interpreter execution ceiling"
        );
        task.validate()?;
        ensure!(
            task.snapshot.request_context.origin_node() == participants.origin.node_id(),
            "Interpreter exchange origin differs"
        );
        // History is inspectable before any fresh peer/model/capacity lookup.
        for record in self.engine.store.interpreter_allowance_records()?.0 {
            let old = record.task()?;
            if old.snapshot.job_id == task.snapshot.job_id
                && old.snapshot.revision == task.snapshot.revision
                && crate::interpreter_allowance::stage(&old.snapshot.source)
                    == crate::interpreter_allowance::stage(&task.snapshot.source)
            {
                ensure!(
                    json!(old) == json!(task),
                    "Interpreter source receipt conflicts; no replacement"
                );
                ensure!(
                    maximum_execution_ms.is_none_or(|v| record
                        .binding
                        .grant
                        .limits
                        .max_execution_ms
                        <= v),
                    "Retained interpreter attempt exceeds the caller ceiling; no replacement"
                );
                return Ok(Exchange::Historical(Box::new(record)));
            }
        }
        let retirement = self.engine.store.reserve_workload_finalization()?;
        let active = self.engine.claim_workload(&task.snapshot.job_id)?;
        let capacity = self
            .registry
            .reserve_best_with_context(
                "llm.infer",
                "run",
                &json!({"node_id":participants.provider.node_id()}),
                Some(&task.snapshot.request_context),
            )
            .context("Selected interpreter has no scheduling capacity")?;
        ensure!(
            capacity.provider().node_id == participants.provider.node_id(),
            "Interpreter capacity cannot substitute another provider"
        );
        let reserved = match self
            .reserve_interpreter_allowance_with_ceiling(
                task,
                pin.clone(),
                participants,
                maximum_execution_ms,
            )
            .await?
        {
            Reservation::Existing(record) => return Ok(Exchange::Historical(record)),
            Reservation::New(reserved) => reserved,
        };
        let continuation = Continuation {
            store: self.engine.store.clone(),
            reserved,
            _capacity: capacity,
            active: Some(active),
            retirement: Some(retirement),
        };
        continuation.check()?;
        let mut closing = self.engine.shutdown_receiver();
        let mut stops = self.engine.stop_changes.subscribe();
        let mut sources = self.engine.store.source_changes();
        let mut settings = self.config.changes();
        let mut peers = self.node_server.subscribe();
        ensure!(!*closing.borrow(), "Coordinator is shutting down");
        let dispatch = self
            .interpreter_delivery_intent(&continuation.reserved, &pin, participants)
            .await?;
        continuation.check()?;
        let observer = Arc::new(Observer {
            store: self.engine.store.clone(),
            id: continuation.reserved.id().into(),
        });
        let remaining = dispatch
            .deadline()
            .saturating_duration_since(std::time::Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Original interpreter deadline expired before dispatch"
        );
        let rpc = self.node_server.invoke_grant_observed(
            dispatch.grant().clone(),
            remaining,
            observer,
            dispatch.deadline(),
        );
        tokio::pin!(rpc);
        let deadline =
            tokio::time::sleep_until(tokio::time::Instant::from_std(dispatch.deadline()));
        tokio::pin!(deadline);
        let reply = loop {
            if let Err(error) = continuation.check() {
                break Err(error);
            }
            tokio::select! {biased;
                _=closing.changed()=>break Err(anyhow::anyhow!("Coordinator lifecycle changed during interpretation")),
                _=&mut deadline=>break Err(anyhow::anyhow!("Original interpreter deadline reached; outcome unconfirmed")),
                changed=stops.changed()=>{
                    if changed.is_err(){break Err(anyhow::anyhow!("Interpreter stop observer unavailable"))}
                    if let Err(error)=self.interpreter_continuation_current(&continuation.reserved,&pin,participants).await {break Err(error)}
                },
                changed=sources.changed()=>{
                    if changed.is_err(){break Err(anyhow::anyhow!("Interpreter source observer unavailable"))}
                    if let Err(error)=self.interpreter_continuation_current(&continuation.reserved,&pin,participants).await {break Err(error)}
                },
                changed=settings.changed()=>{
                    if changed.is_err(){break Err(anyhow::anyhow!("Interpreter settings observer unavailable"))}
                    if let Err(error)=self.interpreter_continuation_current(&continuation.reserved,&pin,participants).await {break Err(error)}
                },
                result=&mut rpc=>break result,
                event=peers.recv()=>{
                    let relevant=match event {
                        Ok(NodeEvent::Registered(_)|NodeEvent::Down(_))=>true,
                        Ok(NodeEvent::Heartbeat(value))=>value["node_id"]==participants.origin.node_id()||value["node_id"]==participants.provider.node_id(),
                        Ok(_)=>false,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed)=>break Err(anyhow::anyhow!("Interpreter gateway retired")),
                    };
                    if relevant {if let Err(error)=self.interpreter_continuation_current(&continuation.reserved,&pin,participants).await {break Err(error)}}
                },
            }
        };
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.stop_interpreter_exchange(&continuation.reserved).await;
                return Err(error);
            }
        };
        let checked = async {
            let report: GrantReport = serde_json::from_value(reply["grant"].clone())
                .context("Interpreter completion reply unconfirmed")?;
            ensure!(
                report.confirms(dispatch.grant()),
                "Interpreter reply differs from issued grant"
            );
            let record = self.exchanged_record(continuation.reserved.id())?;
            ensure!(
                record
                    .report
                    .as_ref()
                    .is_some_and(|saved| json!(saved) == json!(report)),
                "Interpreter completion differs from durable observation"
            );
            if report.success {
                continuation.check()?;
                self.interpreter_continuation_current(&continuation.reserved, &pin, participants)
                    .await?;
            }
            anyhow::Ok((report, record))
        }
        .await;
        let (report, mut record) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                self.stop_interpreter_exchange(&continuation.reserved).await;
                return Err(error);
            }
        };
        if !report.success {
            if report.state == eefn::grants::ReceiptState::Unknown {
                self.stop_interpreter_exchange(&continuation.reserved).await;
                record = self.exchanged_record(continuation.reserved.id())?;
            }
            return Ok(Exchange::Failed(Box::new(record)));
        }
        Ok(Exchange::Completed(Completion {
            record,
            continuation,
        }))
    }

    fn exchanged_record(&self, id: &str) -> Result<Record> {
        self.engine
            .store
            .interpreter_allowance_records()?
            .0
            .into_iter()
            .find(|r| r.id() == id)
            .context("Interpreter completion checkpoint unavailable")
    }

    async fn stop_interpreter_exchange(&self, reserved: &Reserved) {
        // Control-plane cancellation has its own existing bounded RPC. It never
        // renews the inference lease or provides another execution continuation.
        let result = async {
            if let Some(grant) = self
                .engine
                .store
                .interpreter_stop_intent(reserved.id(), &reserved.record.binding_sha256)?
            {
                let report = self.node_server.stop_grant(&grant).await?;
                self.engine.store.interpreter_stop_observed(
                    reserved.id(),
                    &reserved.record.binding_sha256,
                    report,
                )?;
            }
            anyhow::Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(grant_id=%reserved.id(),%error,"Interpreter stop observation unconfirmed; no retry or effect-stop proof");
        }
    }
}

#[cfg(test)]
#[path = "interpreter_exchange_tests.rs"]
mod tests;
