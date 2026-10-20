//! Existing owner/origin jobs-stop intent and exact issued-grant delivery.
use crate::{request_execution::Driver, task::Plan};
use anyhow::{Result, bail};
use eefn::grants::{ExecutionGrant, stop::Report};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub send_attempted_at_ms: u64,
    pub report: Option<Report>,
    pub outcome_unknown: bool,
}
pub(crate) fn validate(plan: &Plan) -> Result<()> {
    let Some(e) = &plan.execution else {
        return Ok(());
    };
    if e.stop_requested_at_ms
        .is_some_and(|t| t < e.started_at_ms || t > i64::MAX as u64)
    {
        bail!("Damaged execution stop timestamp")
    }
    for a in &e.attempts {
        if let Some(stop) = &a.stop {
            if e.stop_requested_at_ms
                .is_none_or(|t| stop.send_attempted_at_ms < t)
                || stop.send_attempted_at_ms > i64::MAX as u64
                || stop.outcome_unknown != stop.report.is_none()
                || stop.report.as_ref().is_some_and(|r| !r.confirms(&a.grant))
            {
                bail!("Stop delivery differs from retained issuance")
            }
        }
    }
    Ok(())
}
pub(crate) type Snapshot = (Option<u64>, BTreeMap<String, Delivery>);
pub(crate) fn snapshot(plan: &Plan) -> Snapshot {
    plan.execution
        .as_ref()
        .map(|e| {
            (
                e.stop_requested_at_ms,
                e.attempts
                    .iter()
                    .filter_map(|a| a.stop.clone().map(|s| (a.grant.grant_id.clone(), s)))
                    .collect(),
            )
        })
        .unwrap_or_default()
}
pub(crate) fn preserve(old: &Snapshot, plan: &Plan) -> Result<()> {
    let now = snapshot(plan);
    if old.0.is_some() && old.0 != now.0 {
        bail!("A retained stop request cannot be erased or retimed")
    }
    for (id, saved) in &old.1 {
        let Some(new) = now.1.get(id) else {
            bail!("A stop delivery cannot be erased or retargeted")
        };
        if saved.send_attempted_at_ms != new.send_attempted_at_ms
            || saved.report.is_some() && saved != new
        {
            bail!("Retained stop receipt is immutable")
        }
    }
    Ok(())
}
impl Driver {
    pub(crate) async fn stop_issued(&self, id: &str, grants: &[ExecutionGrant]) -> Result<()> {
        match grants {
            [] => Ok(()),
            [g] => self.stop_one(id, g).await,
            [a, b] => {
                let (one, two) = tokio::join!(self.stop_one(id, a), self.stop_one(id, b));
                one?;
                two?;
                Ok(())
            }
            _ => bail!("Stop targets must be one issued grant or its exact peer pair"),
        }
    }
    async fn stop_one(&self, id: &str, g: &ExecutionGrant) -> Result<()> {
        let mut send = false;
        self.engine.store.edit(
            id,
            "Exact stop delivery saved before one network attempt",
            |p| {
                let Some(e) = &mut p.execution else {
                    bail!("Missing execution issuance")
                };
                let Some(requested) = e.stop_requested_at_ms else {
                    return Ok(false);
                };
                let a = e
                    .attempts
                    .iter_mut()
                    .find(|a| a.grant.grant_id == g.grant_id)
                    .ok_or_else(|| anyhow::anyhow!("Stop grant is not issued"))?;
                if a.grant != *g {
                    bail!("Stop cannot retarget changed issuance")
                }
                if a.stop.is_some() {
                    return Ok(false);
                }
                a.stop = Some(Delivery {
                    send_attempted_at_ms: eefn::protocol::now_ms().max(requested),
                    report: None,
                    outcome_unknown: true,
                });
                send = true;
                Ok(true)
            },
        )?;
        if !send {
            return Ok(());
        }
        // A failed or dropped send remains uncertain; never try another
        // session, node, ordinary RPC, restart or replacement grant.
        if let Ok(report) = self.server.stop_grant(g).await {
            self.engine.store.edit(
                id,
                "Exact durable node stop intent acknowledged; effects remain unknown",
                |p| {
                    let a = p
                        .execution
                        .as_mut()
                        .and_then(|e| {
                            e.attempts
                                .iter_mut()
                                .find(|a| a.grant.grant_id == g.grant_id)
                        })
                        .ok_or_else(|| anyhow::anyhow!("Stop issuance vanished"))?;
                    if a.grant != *g || !report.confirms(g) {
                        bail!("Stop receipt scope changed")
                    }
                    let saved = a
                        .stop
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("Stop send checkpoint missing"))?;
                    if let Some(existing) = &saved.report {
                        if *existing != report {
                            bail!("Stop receipt already differs")
                        }
                        return Ok(false);
                    }
                    saved.report = Some(report);
                    saved.outcome_unknown = false;
                    Ok(true)
                },
            )?;
        }
        Ok(())
    }
    pub(crate) async fn wait_with_stop<F, T>(
        &self,
        id: &str,
        grants: &[ExecutionGrant],
        future: F,
    ) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        let mut changes = self.engine.stop_changes.subscribe();
        let stopping = || {
            self.engine.store.get(id).map(|p| {
                p.execution
                    .as_ref()
                    .is_some_and(|e| e.stop_requested_at_ms.is_some())
            })
        };
        if stopping()? {
            self.stop_issued(id, grants).await?;
            bail!(
                "Stop requested before further dispatch; inspect retained stop/consumption evidence"
            )
        }
        tokio::pin!(future);
        loop {
            tokio::select! {
                biased;
                changed=changes.changed()=>{
                    if changed.is_err() { bail!("Stop observer unavailable; no continuation") }
                    if stopping()? {
                        if self.stop_issued(id,grants).await.is_err() {
                            tracing::warn!(job_id=%id,"Stop delivery checkpoint unavailable; no retry or stop proof");
                        }
                    }
                }
                result=&mut future=>return result,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::EventBus,
        task::{PlanStatus, TaskEngine},
    };
    use serde_json::json;
    fn stopping(engine: &TaskEngine) -> (Plan, ExecutionGrant) {
        let (plan, g, _) = crate::request_execution::tests::issued(engine);
        let plan = engine
            .store
            .change(&plan.plan_id, "Explicit test stop intent", |p| {
                p.status = PlanStatus::Stopping;
                let e = p.execution.as_mut().unwrap();
                e.stop_requested_at_ms = Some(eefn::protocol::now_ms().max(e.started_at_ms));
                Ok(())
            })
            .unwrap();
        (plan, g)
    }
    fn report(g: &ExecutionGrant) -> Report {
        Report {
            schema_version: 1,
            report_type: "grant_stop_receipt".into(),
            grant_id: g.grant_id.clone(),
            grant_sha256: eefn::grants::definition_digest(&json!(g)).unwrap(),
            issuer_id: g.issuer_id.clone(),
            session_id: g.session_id.clone(),
            executor_node: g.executor_node.clone(),
            job_id: g.job_id.clone(),
            task_id: g.task_id.clone(),
            attempt: g.attempt,
            recorded_at_ms: eefn::protocol::now_ms().max(1),
            receipt_preexisted: true,
            reused: false,
            cancellation_signalled: true,
            persisted: true,
            stop_requested: true,
            execution_stopped: false,
            external_effects_stopped: false,
            execution_authorized: false,
            automatically_retried: false,
        }
    }
    #[tokio::test]
    async fn stop_delivery_offline_is_recorded_once_without_retry_or_new_dispatch() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g) = stopping(&engine);
        let server = eefn::NodeServer::new("private-stop-fixture-key", "127.0.0.1", 0).unwrap();
        let driver = Driver::peer_fixture(engine.clone(), server, Default::default());
        driver
            .stop_issued(&p.plan_id, std::slice::from_ref(&g))
            .await
            .unwrap();
        let saved = engine.store.get(&p.plan_id).unwrap();
        let before = json!(saved);
        let stop = saved.execution.as_ref().unwrap().attempts[0]
            .stop
            .as_ref()
            .unwrap();
        assert!(stop.outcome_unknown);
        assert!(stop.report.is_none());
        driver
            .stop_issued(&p.plan_id, std::slice::from_ref(&g))
            .await
            .unwrap();
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), before);
        let called = std::sync::atomic::AtomicBool::new(false);
        assert!(
            driver
                .wait_with_stop(&p.plan_id, std::slice::from_ref(&g), async {
                    called.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
                .await
                .is_err()
        );
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[test]
    fn stop_intent_target_and_confirmed_receipt_are_immutable_and_never_effect_stop_proof() {
        let engine = TaskEngine::new(EventBus::new(20));
        let (p, g) = stopping(&engine);
        engine
            .store
            .change(&p.plan_id, "Save one send attempt", |p| {
                let e = p.execution.as_mut().unwrap();
                e.attempts[0].stop = Some(Delivery {
                    send_attempted_at_ms: e.stop_requested_at_ms.unwrap(),
                    report: None,
                    outcome_unknown: true,
                });
                Ok(())
            })
            .unwrap();
        let before = json!(engine.store.get(&p.plan_id).unwrap());
        assert!(
            engine
                .store
                .change(&p.plan_id, "Cannot erase intent", |p| {
                    p.execution.as_mut().unwrap().stop_requested_at_ms = None;
                    Ok(())
                })
                .is_err()
        );
        assert!(
            engine
                .store
                .change(&p.plan_id, "Cannot erase delivery", |p| {
                    p.execution.as_mut().unwrap().attempts[0].stop = None;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), before);
        let valid = report(&g);
        assert!(valid.confirms(&g));
        let mut forged = valid.clone();
        forged.execution_stopped = true;
        assert!(
            engine
                .store
                .change(&p.plan_id, "Reject forged physical proof", |p| {
                    let s = p.execution.as_mut().unwrap().attempts[0]
                        .stop
                        .as_mut()
                        .unwrap();
                    s.report = Some(forged);
                    s.outcome_unknown = false;
                    Ok(())
                })
                .is_err()
        );
        engine
            .store
            .change(&p.plan_id, "Retain exact control receipt", |p| {
                let s = p.execution.as_mut().unwrap().attempts[0]
                    .stop
                    .as_mut()
                    .unwrap();
                s.report = Some(valid);
                s.outcome_unknown = false;
                Ok(())
            })
            .unwrap();
        let final_state = json!(engine.store.get(&p.plan_id).unwrap());
        assert!(
            engine
                .store
                .change(&p.plan_id, "Cannot replace saved proof", |p| {
                    p.execution.as_mut().unwrap().attempts[0]
                        .stop
                        .as_mut()
                        .unwrap()
                        .report
                        .as_mut()
                        .unwrap()
                        .reused = true;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(json!(engine.store.get(&p.plan_id).unwrap()), final_state);
    }
}
