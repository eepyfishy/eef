//! One original engine-owned issuer drives a reviewed session. Bounded event
//! history is independent of lease control; no dashboard or detached renewal loop.
use crate::{Runtime, request_execution::Driver, task::Plan, workload_jobs::Issued};
use anyhow::{Context, Result, bail, ensure};
use eefn::{
    workload_leases::{Query, definition::Template},
    workload_network::{Event, Session},
};
use futures_util::{StreamExt, future::BoxFuture, stream::FuturesOrdered};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::Instant;

impl Runtime {
    pub(crate) async fn execute_workload(
        &self,
        saved: &Plan,
        command: &eefn::request_execution::ExecuteCommand,
        origin: Option<&str>,
    ) -> Result<Value> {
        let template = Template::from_params(&saved.goal.params)?;
        if let Some(origin) = origin {
            ensure!(
                self.disclosed_planning_nodes(origin)
                    .await?
                    .contains(&template.executor_node),
                "Source node is no longer disclosed to this origin"
            );
        }
        let session = self
            .node_server
            .workload_session(
                &template,
                self.registry.scheduling_policy().freshness_seconds,
            )
            .await?;
        let source = self.node_server.workload_source(&template, 5).await?;
        source.confirms_execution(&template, &session.runtime)?;
        ensure!(
            source.runtime_id() == session.runtime
                && saved
                    .approval
                    .as_ref()
                    .is_some_and(|a| a.issuer_id == session.issuer)
                && self
                    .node_server
                    .workload_session(
                        &template,
                        self.registry.scheduling_policy().freshness_seconds
                    )
                    .await?
                    == session,
            "Workload source, approval or authenticated session changed during inspection"
        );
        let observation_end =
            Instant::now() + Duration::from_millis(template.limits.max_lease_ms.min(5000));
        let issued = crate::workload_jobs::issue_owned(
            &self.engine,
            command,
            &session.session,
            &session.policy,
            observation_end,
        )
        .await?;
        let retained = self
            .engine
            .store
            .get_before(&command.id, observation_end)
            .await?;
        let response = crate::request_execution::report(self, &retained, false)?;
        let driver = Driver::new(self);
        let bus = self.bus.clone();
        // The original issuer moves once into this engine-tracked driver. Drop
        // on task cancellation retains Unknown; rows cannot reconstruct it.
        tokio::spawn(async move {
            let id = issued.definition().job_id.clone();
            if let Err(error) = driver.drive_workload(issued, session, bus).await {
                tracing::warn!(job_id=%id,%error,"Workload requires inspection; no automatic retry");
            }
        });
        Ok(response)
    }
}
impl Driver {
    pub(crate) async fn drive_workload(
        &self,
        mut issued: Issued,
        session: Session,
        bus: crate::event::EventBus,
    ) -> Result<()> {
        let definition = issued.definition().clone();
        let mut stop = issued.stop_receiver()?;
        let mut shutdown = self.engine.shutdown_receiver();
        // Check before opening the route, not just before processing its first
        // probe. Stop/reconnect can never create a replacement source attempt.
        ensure!(
            !*stop.borrow() && !*shutdown.borrow(),
            "Workload stopped before offer"
        );
        let outcome = {
            let frames = self.workload_frames(&mut issued, &session, bus);
            tokio::pin!(frames);
            tokio::select! { biased;
                _=stop.changed()=>Err(anyhow::anyhow!("Workload owner stop or engine shutdown")),
                _=shutdown.changed()=>Err(anyhow::anyhow!("Coordinator shutdown")),
                result=&mut frames=>result,
            }
        };
        issued.retire();
        if *stop.borrow() && !*shutdown.borrow() {
            // Revoked control precedes the exact metadata/one-send stop path.
            // A failed stop-intent save never authorizes this network request.
            if self
                .stop_workload(&definition, Instant::now() + Duration::from_secs(5))
                .await
                .is_err()
            {
                tracing::warn!(job_id=%definition.job_id,"Workload stop delivery unconfirmed; no retry");
            }
        }
        outcome
    }
    async fn workload_frames(
        &self,
        issued: &mut Issued,
        session: &Session,
        bus: crate::event::EventBus,
    ) -> Result<()> {
        let d = issued.definition().clone();
        let deadline = issued.deadline();
        ensure!(
            self.server
                .workload_session(
                    &d.template(),
                    self.registry.scheduling_policy().freshness_seconds
                )
                .await?
                == *session,
            "Workload authenticated registration changed before offer"
        );
        let mut link = self.server.prepare_workload(d.clone(), deadline).await?;
        ensure!(
            link.peer().runtime() == session.runtime
                && link.peer().session() == session.session
                && link.peer().issuer() == session.issuer
                && link.peer().node() == session.node,
            "Prepared workload route changed its authenticated peer"
        );
        issued.deliver(|| link.dispatch_offer())?;
        let mut saves: FuturesOrdered<BoxFuture<'static, Result<()>>> = FuturesOrdered::new();
        let mut received = 0u64;
        loop {
            let frame = tokio::select! { biased;
                frame=link.next()=>frame?,
                saved=saves.next(),if !saves.is_empty()=>{saved.context("Workload event observer retired")??;continue;},
            };
            let peer = frame.peer();
            ensure!(
                peer.issuer() == session.issuer
                    && peer.session() == session.session
                    && peer.node() == session.node
                    && peer.runtime() == session.runtime,
                "Workload frame is not from its original authenticated peer"
            );
            match frame.message()["type"].as_str() {
                Some("workload_probe") => {
                    let query: Query = serde_json::from_value(frame.message()["query"].clone())?;
                    let reply = issued
                        .lease(
                            &query,
                            peer.issuer(),
                            peer.session(),
                            peer.node(),
                            deadline.min(Instant::now() + Duration::from_secs(2)),
                        )
                        .await?;
                    issued.deliver(|| link.reply_lease(&query, reply))?;
                }
                Some("workload_ack") => {
                    let ack = serde_json::from_value(frame.message()["receipt"].clone())?;
                    issued.acknowledge(ack, deadline).await?;
                }
                Some("workload_event") => {
                    ensure!(
                        saves.len() < d.events.max_pending as usize,
                        "Workload event observation capacity exhausted"
                    );
                    let event: Event = serde_json::from_value(frame.message()["event"].clone())?;
                    received = received
                        .checked_add(1)
                        .context("Workload event count overflow")?;
                    ensure!(
                        event.sequence == received,
                        "Workload delivered event sequence changed"
                    );
                    let receiver = self.engine.store.queue_workload_event(
                        d.clone(),
                        event.clone(),
                        deadline,
                    )?;
                    let event_bus = bus.clone();
                    let context = d.request_context.clone();
                    let job_id = d.job_id.clone();
                    let grant_id = d.grant_id.clone();
                    saves.push_back(Box::pin(async move {
                        tokio::time::timeout_at(deadline, receiver).await.context("Workload event observation deadline elapsed")?
                            .context("Workload event worker unavailable")??;
                        ensure!(Instant::now() < deadline, "Workload event observed after its original deadline");
                        event_bus.publish("workload.event", json!({"job_id":job_id,"grant_id":grant_id,"request_context":context,"event":event})).await;
                        // Keep the original network byte/count charge through
                        // actual save observation and bounded notification.
                        drop(frame);
                        Ok(())
                    }));
                }
                Some("workload_complete") => {
                    while let Some(saved) = saves.next().await {
                        saved?;
                    }
                    ensure!(
                        received > 0,
                        "Completed source lacks its required durable event prefix"
                    );
                    let receipt = serde_json::from_value(frame.message()["receipt"].clone())?;
                    issued
                        .complete(
                            receipt,
                            peer.issuer(),
                            peer.session(),
                            peer.node(),
                            deadline,
                        )
                        .await?;
                    return Ok(());
                }
                Some("workload_rejected") => bail!(
                    "Node stopped or rejected the original workload; inspect retained evidence"
                ),
                _ => bail!("Unsupported workload control frame"),
            }
        }
    }
}
