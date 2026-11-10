//! One authenticated session owns admission, the source and renewal together.
//! Reconnect never reconstructs any of them from historical receipt bytes.
use super::*;
use crate::workload_leases::{Contract, Requests, definition::Definition};

type Writer = Arc<Mutex<tokio::io::WriteHalf<crate::tls::NodeStream>>>;
#[derive(Clone)]
struct Output {
    crypto: NodeCrypto,
    writer: Writer,
    failed: tokio::sync::watch::Sender<bool>,
}

#[cfg(test)]
#[path = "client_workload_tests.rs"]
mod tests;
impl Output {
    async fn send(&self, value: &Value) -> Result<()> {
        write_session_frame(&self.crypto, &self.writer, &self.failed, value).await
    }
    async fn probe(&self, id: &str, query: Value) -> Result<()> {
        self.send(&json!({"type":"workload_probe","schema_version":2,"id":id,"query":query}))
            .await
    }
}
fn rejected(id: &str, code: &str) -> Value {
    json!({"type":"workload_rejected","schema_version":2,"id":id,"code":code})
}

fn unconfirmed_status(id: &str, code: &str, error: &anyhow::Error) -> Value {
    // Local diagnostics retain only closed bridge/owner-limit categories. Contexts can
    // contain private source bindings, interpreter output or filesystem paths.
    let reason = error
        .downcast_ref::<crate::python::BridgeFailure>()
        .map(|failure| failure.code())
        .or_else(|| {
            error
                .downcast_ref::<crate::grants::GrantDenied>()
                .and_then(|failure| match failure.0 {
                    "grant_owner_limits_exceeded" => Some("grant_owner_limits_exceeded"),
                    "grant_receipt_quota_full" => Some("grant_receipt_quota_full"),
                    _ => None,
                })
        })
        .unwrap_or("workload_unconfirmed");
    json!({"grant_id":id,"state":"unconfirmed","code":code,
        "reason_code":reason,"outcome_confirmed":false})
}

type Prepared = (
    crate::workload_leases::Policy,
    crate::workload_policy::Permit,
    crate::workload_runtime::PreparedMicrophone,
);
async fn prepare(
    engine: Arc<NodeEngine>,
    runtime: String,
    definition: Definition,
    execution: tokio::sync::OwnedSemaphorePermit,
    #[cfg(test)] started: Arc<std::sync::atomic::AtomicUsize>,
) -> Result<(tokio::sync::OwnedSemaphorePermit, Result<Prepared>)> {
    let budget = Duration::from_millis(definition.limits.max_lease_ms);
    let (reply, receiver) = tokio::sync::oneshot::channel();
    // No runtime blocking pool or unbounded queue. Existing work admission
    // bounds actual native preparation, including after its waiter is lost.
    // This worker prepares passive bytes/reservations only, never a lease or
    // running source; runtime shutdown need not join a stalled config read.
    std::thread::Builder::new()
        .name("eefn-source-prepare".into())
        .spawn(move || {
            #[cfg(test)]
            started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.prepare_workload(&runtime, definition)
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Node source preparation failed")));
            drop(engine);
            let _ = reply.send((execution, result));
        })
        .context("Node source preparation worker unavailable")?;
    timeout(budget, receiver)
        .await
        .context("Source preparation observation expired; no source started")?
        .context("Node source preparation worker unavailable")
}

/// A local channel lifetime observation, never a lease or completion receipt.
#[derive(Debug)]
struct SourceRetired;

async fn await_source<F, R, T>(source: F, renewal: R) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
    R: std::future::Future<Output = Result<SourceRetired>>,
{
    tokio::pin!(source);
    tokio::pin!(renewal);
    tokio::select! {
        biased;
        result = &mut source => result,
        retired = &mut renewal => {
            retired?;
            // The exact live receiver is gone. The same source future may
            // still await its bounded historical save; no source is restarted
            // and no renewal continues while that receipt is pending.
            source.await
        }
    }
}

async fn renew(
    definition: &Definition,
    contract: &Contract,
    requests: Arc<Requests>,
    output: &Output,
    sender: tokio::sync::mpsc::Sender<crate::workload_leases::LeaseProof>,
    mut deadline: Instant,
    session_end: Instant,
) -> Result<SourceRetired> {
    let mut sequence = 2_u64;
    loop {
        tokio::select! {
            biased;
            _ = sender.closed() => return Ok(SourceRetired),
            result = async {
                let now = Instant::now();
                if now >= deadline.min(session_end) {
                    bail!("Original workload renewal window expired")
                }
                // Schedule from the actual last proof's remaining monotonic
                // budget, never an advertised duration or a new session end.
                let due = now + deadline.min(session_end).duration_since(now) / 3;
                tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
                let proof = requests.request(contract, sequence, |query| {
                    output.probe(&definition.grant_id, query)
                }).await?;
                deadline = proof.deadline().min(session_end);
                // Closure after proof receipt has the same historical meaning.
                if sender.send(proof).await.is_err() { return Ok(false); }
                sequence = sequence.checked_add(1).context("Workload renewal sequence exhausted")?;
                Ok::<_, anyhow::Error>(true)
            } => if !result? { return Ok(SourceRetired); },
        }
    }
}

impl NodeClient {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn dispatch_workload(
        &self,
        message: Value,
        writer: &Writer,
        failed: &tokio::sync::watch::Sender<bool>,
        tasks: &mut tokio::task::JoinSet<()>,
        issuer: Option<&str>,
        session: Option<&str>,
        requests: Arc<Requests>,
    ) -> Result<()> {
        // Malformed identities cannot create a route or consume reply capacity.
        let Some(id) = message["id"]
            .as_str()
            .filter(|id| id.len() <= 64 && crate::automation_occurrence::uuid(id).is_ok())
        else {
            return Ok(());
        };
        let id = id.to_owned();
        let parsed = serde_json::from_value::<crate::workload_network::Offer>(message);
        let prepared = match (
            parsed,
            self.grant_ledger.clone(),
            issuer,
            session,
            self.registration_runtime.as_deref(),
        ) {
            (Ok(offer), Some(ledger), Some(issuer), Some(session), Some(runtime))
                if self.tls_config.is_some()
                    && offer
                        .validate(issuer, session, &self.config.node_id, runtime)
                        .is_ok() =>
            {
                Some((
                    offer,
                    ledger,
                    issuer.to_owned(),
                    session.to_owned(),
                    runtime.to_owned(),
                ))
            }
            _ => None,
        };
        let Some((offer, ledger, issuer, session, runtime)) = prepared else {
            return write_session_frame(
                &self.crypto,
                writer,
                failed,
                &rejected(&id, "policy_denied"),
            )
            .await;
        };
        let Some(admission) = self.request_budget.admit("workload.session", "execute") else {
            return write_session_frame(
                &self.crypto,
                writer,
                failed,
                &rejected(&id, "workload_busy"),
            )
            .await;
        };
        let output = Output {
            crypto: self.crypto.clone(),
            writer: writer.clone(),
            failed: failed.clone(),
        };
        let engine = self.engine.clone();
        let status = self.status.clone();
        let node = self.config.node_id.clone();
        let definition = offer.definition;
        let (execution, reply) = admission.into_parts();
        #[cfg(test)]
        let prepare_started = self.source_prepare_started.clone();
        tasks.spawn(async move {
            let mut code = "policy_denied";
            let result: Result<crate::grants::workload::Receipt> = Box::pin(async {
                let checked = definition.clone();
                // Passive source preparation/configuration inspection can wait
                // on storage. Its actual work retains execution capacity even
                // after this connection task is cancelled. It opens no source.
                let (execution, prepared) = prepare(engine, runtime, checked, execution,
                    #[cfg(test)] prepare_started,
                ).await?;
                let (policy, permit, source) = prepared?;
                let result = permit.supervise(Box::pin(async {
                    let contract = definition.lease_contract()?;
                    code = "consumption_unconfirmed";
                    let initial = requests.request(&contract, 1, |query| output.probe(&id, query)).await?;
                    let initial_end = initial.initial_deadline(&contract, &policy)?;
                    let session_end = initial.session_deadline();
                    let mut consumed = ledger.consume_workload(
                        definition.clone(), &policy, initial, &issuer, &session, &node,
                        tokio::time::Instant::from_std(initial_end),
                    ).await?;
                    let ack = consumed.acknowledgement_before(tokio::time::Instant::from_std(initial_end)).await?;
                    // Cancellation here retires a possibly partial frame and
                    // drops consumption without ever polling the prepared source.
                    tokio::time::timeout_at(tokio::time::Instant::from_std(initial_end),
                        output.send(&json!({"type":"workload_ack","schema_version":2,"id":id,"receipt":ack})))
                        .await.context("Consumed ACK delivery exceeded its original lease")??;
                    status.lock().expect("workload status")["last_workload"] = json!({
                        "grant_id":id,"state":"consumed","source_started":false,"outcome_confirmed":false});
                    code = "source_unconfirmed";
                    let (sender, renewals) = crate::workload_leases::renewal_channel();
                    let event_output = output.clone();
                    let event_id = id.clone();
                    // The live window closes before historical persistence.
                    // Stop renewal on that receiver's retirement, then retain
                    // the same bounded completion waiter until terminal output.
                    await_source(
                        Box::pin(source.run(consumed, renewals, move |event| {
                            let output = event_output.clone();
                            let id = event_id.clone();
                            async move { output.send(&json!({"type":"workload_event","schema_version":2,"id":id,"event":event})).await }
                        })),
                        Box::pin(renew(&definition, &contract, requests.clone(), &output,
                            sender, initial_end, session_end)),
                    ).await
                })).await;
                // A policy cancellation drops its source/renewal scope first.
                drop(execution);
                result
            }).await;
            let frame = match result {
                Ok(receipt) => {
                    status.lock().expect("workload status")["last_workload"] = json!({
                        "grant_id":id,"state":"completed","receipt":receipt,"outcome_confirmed":true});
                    json!({"type":"workload_complete","schema_version":2,"id":id,"receipt":receipt})
                }
                Err(error) => {
                    status.lock().expect("workload status")["last_workload"] =
                        unconfirmed_status(&id, code, &error);
                    rejected(&id, code)
                }
            };
            let _ = write_reply_frame(&output.crypto, &output.writer, &output.failed, &frame, Some(reply)).await;
        });
        Ok(())
    }
}
