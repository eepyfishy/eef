//! Two-party issuance and outcome validation IN the existing reviewed journal.
//! No independent approval, automatic replay, producer/sink or public listener.
use crate::{
    request_execution::{AttemptState, GrantedAttempt},
    task::{Plan, PlanStatus, TaskStatus},
};
use anyhow::{Context, Result, bail};
use eefn::grants::GrantObserver;
use eefn::{
    grants::{ExecutionGrant, GrantReport, ReceiptState, definition_digest},
    peer::{
        SealedPair,
        plan::Pair,
        scope::{Party, Role, Scope},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) fn definition(p: &Plan) -> Result<Option<Pair>> {
    if p.goal.template == "peer_transfer" {
        Ok(Some(Pair::from_params(&p.goal.params)?))
    } else if p.tasks.iter().any(|t| t.capability == "peer.transfer") {
        bail!("Peer operations require an exact paired template")
    } else {
        Ok(None)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issuance {
    pub schema_version: u32,
    pub scope: Scope,
    pub coordinator_certificate: String,
    pub material: SealedPair,
}
impl Issuance {
    pub(crate) fn prepare(
        p: &Plan,
        grants: &[ExecutionGrant; 2],
        runtime: &str,
        generations: [&str; 2],
        address: SocketAddr,
        server: &eefn::NodeServer,
    ) -> Result<Self> {
        let pair = definition(p)?.context("Missing paired definition")?;
        eefn::peer::plan::validate_address(address)?;
        let party = |i: usize| -> Result<Party> {
            let g = &grants[i];
            Ok(Party {
                node_id: g.executor_node.clone(),
                control_session_id: g.session_id.clone(),
                runtime_generation: generations[i].into(),
                grant_id: g.grant_id.clone(),
                grant_sha256: definition_digest(&json!(g))?,
                resource_id: g.resource_id.clone().context("Missing peer resource")?,
            })
        };
        let scope = Scope {
            schema_version: 1,
            session_id: uuid::Uuid::new_v4().to_string(),
            issuer_id: grants[0].issuer_id.clone(),
            coordinator_runtime_id: runtime.into(),
            job_id: p.plan_id.clone(),
            plan_sha256: grants[0].plan_sha256.clone(),
            request_context: grants[0].request_context.clone(),
            source: party(0)?,
            destination: party(1)?,
            destination_address: address,
            issued_at_ms: grants.iter().map(|g| g.issued_at_ms).max().unwrap(),
            expires_at_ms: grants.iter().map(|g| g.expires_at_ms).min().unwrap(),
            limits: pair.limits,
        };
        let policy = server.authentication_policy();
        // NodeServer construction already validated independence from its actual
        // legacy secret. It is never a peer encryption/signing credential.
        let material = eefn::peer::seal_pair(&scope, policy, "", runtime)?;
        let issuance = Self {
            schema_version: 1,
            scope,
            material,
            coordinator_certificate: policy
                .tls
                .as_ref()
                .context("Missing pinned issuer")?
                .certificate
                .clone(),
        };
        issuance.validate_for(p, grants)?;
        Ok(issuance)
    }
    pub(crate) fn validate_for(&self, p: &Plan, grants: &[ExecutionGrant]) -> Result<()> {
        let pair = definition(p)?.context("Pair issuance on an ordinary job")?;
        let approval = p.approval.as_ref().context("Pair lacks owner approval")?;
        if self.schema_version != 1
            || approval.delegation.is_some()
            || p.tasks.len() != 2
            || grants.len() != 2
            || self.scope.job_id != p.plan_id
            || self.scope.issuer_id != approval.issuer_id
            || self.scope.plan_sha256 != approval.plan_sha256
            || Some(&self.scope.request_context) != p.goal.request_context.as_ref()
            || self.scope.limits != pair.limits
            || self.material.source.role != Role::Source
            || self.material.destination.role != Role::Destination
            || self.scope.issued_at_ms != grants.iter().map(|g| g.issued_at_ms).max().unwrap()
            || self.scope.expires_at_ms != grants.iter().map(|g| g.expires_at_ms).min().unwrap()
        {
            bail!("Pair issuance differs from reviewed approval")
        }
        self.scope.validate()?;
        for (i, role) in [Role::Source, Role::Destination].into_iter().enumerate() {
            let g = &grants[i];
            g.validate()?;
            let party = self.scope.party(role);
            let endpoint = pair.endpoint(role);
            let mut expected = crate::request_approval::scope(p, i, approval)?;
            expected.grant_id = g.grant_id.clone();
            expected.session_id = g.session_id.clone();
            expected.issued_at_ms = g.issued_at_ms;
            expected.expires_at_ms = g.expires_at_ms;
            if *g != expected
                || g.capability != "peer.transfer"
                || g.action != if i == 0 { "send" } else { "receive" }
                || g.executor_node != endpoint.node_id
                || g.resource_id.as_ref() != Some(&endpoint.resource_id)
                || g.params != json!(pair.definition(role))
                || g.task_id != p.tasks[i].task_id
                || party.grant_id != g.grant_id
                || party.grant_sha256 != definition_digest(&json!(g))?
                || party.node_id != g.executor_node
                || party.control_session_id != g.session_id
                || Some(&party.resource_id) != g.resource_id.as_ref()
                || !p.tasks[i].dependencies.is_empty()
            {
                bail!("Pair grant/resource/session differs")
            }
            eefn::peer::verify_envelope(
                &self.scope,
                if i == 0 {
                    &self.material.source
                } else {
                    &self.material.destination
                },
                &self.coordinator_certificate,
            )?;
        }
        Ok(())
    }
    pub(crate) fn confirms(&self, g: &ExecutionGrant, report: &GrantReport) -> bool {
        if !report.confirms(g) {
            return false;
        }
        if report.state != ReceiptState::Completed || !report.success {
            return true;
        }
        let Ok(r) =
            serde_json::from_value::<eefn::peer::admission::TransferReceipt>(report.data.clone())
        else {
            return false;
        };
        r.pair_session_id == self.scope.session_id
            && self.scope.digest().is_ok_and(|h| h == r.scope_sha256)
            && r.role
                == if g.grant_id == self.scope.source.grant_id {
                    Role::Source
                } else {
                    Role::Destination
                }
    }
}
pub(crate) fn issue(p: &mut Plan, grants: [ExecutionGrant; 2], issuance: Issuance) -> Result<()> {
    let e = p.execution.as_ref().context("Missing execution request")?;
    if p.status != PlanStatus::Running
        || !e.attempts.is_empty()
        || e.peer.is_some()
        || p.tasks.iter().any(|t| {
            t.status != TaskStatus::Pending || t.attempts != 0 || !t.assignments.is_empty()
        })
    {
        bail!("Pair issuance requires two untouched reviewed tasks")
    }
    issuance.validate_for(p, &grants)?;
    // Caller saves this entire mutation in ONE existing journal transaction.
    p.execution.as_mut().unwrap().peer = Some(issuance);
    for g in grants {
        crate::request_execution::append_attempt(p, g);
    }
    Ok(())
}
pub(crate) fn validate_retained(p: &Plan) -> Result<bool> {
    let paired = definition(p)?.is_some();
    let Some(e) = &p.execution else {
        return Ok(paired);
    };
    match (&e.peer, paired) {
        (Some(record), true) => record.validate_for(
            p,
            &e.attempts
                .iter()
                .map(|a| a.grant.clone())
                .collect::<Vec<_>>(),
        )?,
        (None, true) if e.attempts.is_empty() => {}
        (None, false) => {}
        _ => bail!("Incomplete or misplaced paired issuance"),
    }
    if e.peer.is_some() {
        let reports = e
            .attempts
            .iter()
            .filter_map(|a| a.report.as_ref())
            .filter(|r| r.state == ReceiptState::Completed && r.success)
            .collect::<Vec<_>>();
        if reports.len() == 2
            && (reports[0].data["payload_bytes"] != reports[1].data["payload_bytes"]
                || reports[0].data["payload_sha256"] != reports[1].data["payload_sha256"])
        {
            bail!("Paired endpoints reported different content; no joint completion")
        }
        if e.abandonment.is_some() || e.attempts.iter().any(|a| a.recovery.is_some()) {
            bail!("Unsupported partial paired recovery/closure")
        }
        let running = e.attempts.iter().any(active);
        if running && !matches!(p.status, PlanStatus::Running | PlanStatus::Stopping) {
            bail!("Active pair has a terminal parent")
        }
        if !running {
            let expected = if e.attempts.iter().any(|a| a.state == AttemptState::Unknown) {
                PlanStatus::Interrupted
            } else if p.tasks.iter().any(|t| t.status == TaskStatus::Failed) {
                PlanStatus::Failed
            } else {
                PlanStatus::Completed
            };
            if p.status != expected
                && !(expected == PlanStatus::Completed && p.status == PlanStatus::Cancelled)
            {
                bail!("Pair parent does not match both terminal outcomes")
            }
        }
    }
    for a in &e.attempts {
        if e.peer.as_ref().is_some_and(|r| {
            a.acknowledgement
                .as_ref()
                .is_some_and(|v| !r.confirms(&a.grant, v))
                || a.final_report().is_some_and(|v| !r.confirms(&a.grant, v))
        }) {
            bail!("Cross-session paired report")
        }
    }
    Ok(paired)
}
pub(crate) fn active(a: &GrantedAttempt) -> bool {
    matches!(a.state, AttemptState::Issued | AttemptState::Accepted)
}
pub(crate) fn settle(p: &mut Plan) {
    let e = p.execution.as_ref().unwrap();
    if e.attempts.iter().any(active) {
        return;
    }
    p.status = if e.attempts.iter().any(|a| a.state == AttemptState::Unknown) {
        PlanStatus::Interrupted
    } else if p.tasks.iter().any(|t| t.status == TaskStatus::Failed) {
        PlanStatus::Failed
    } else if p.status == PlanStatus::Stopping {
        PlanStatus::Cancelled
    } else {
        PlanStatus::Completed
    };
}

struct ReadyObserver {
    journal: crate::request_execution::AttemptJournal,
    ready: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}
impl GrantObserver for ReadyObserver {
    fn record(&self, report: &GrantReport) -> Result<()> {
        self.journal.record(report)?;
        if report.state == ReceiptState::Accepted {
            if let Some(sender) = self
                .ready
                .lock()
                .map_err(|_| anyhow::anyhow!("Peer ACK guard unavailable"))?
                .take()
            {
                let _ = sender.send(());
            }
        }
        Ok(())
    }
}
impl crate::request_execution::Driver {
    pub(crate) async fn drive_peer(&self, id: &str) -> Result<()> {
        let p = self.engine.store.get(id)?;
        if p.status == PlanStatus::Stopping
            && p.execution.as_ref().is_some_and(|e| e.attempts.is_empty())
        {
            self.engine
                .store
                .change(id, "Peer stop settled before any issuance", |p| {
                    p.status = PlanStatus::Cancelled;
                    crate::request_execution::cancel_pending(p);
                    Ok(())
                })?;
            return Ok(());
        }
        if p.status != PlanStatus::Running {
            return Ok(());
        }
        let e = p
            .execution
            .as_ref()
            .context("Missing paired execution request")?;
        if e.peer.is_some() || !e.attempts.is_empty() {
            bail!("An issued pair cannot be resumed or dispatched again")
        }
        let pair = definition(&p)?.context("Missing pair")?;
        let approval = p.approval.as_ref().context("Missing peer owner approval")?;
        if approval.delegation.is_some() {
            bail!("Paired transport requires explicit one-shot approval")
        }
        let sessions = [self.session(&p, 0).await?, self.session(&p, 1).await?];
        let support = if pair.audio.is_some() {
            "finite_audio"
        } else {
            "finite_transport"
        };
        if sessions.iter().any(|s| s[support] != true) {
            bail!("An endpoint does not support the approved peer content mode")
        }
        let mut reservations = Vec::new();
        for (i, s) in sessions.iter().enumerate() {
            let policy: eefn::grants::GrantPolicy = serde_json::from_value(s["limits"].clone())?;
            policy.validate()?;
            if s["issuer_id"] != approval.issuer_id
                || approval.limits.max_execution_ms > policy.max_duration_ms
                || approval.limits.max_result_bytes > policy.max_result_bytes
            {
                bail!("Peer issuer or owner bounds changed")
            }
            let t = &p.tasks[i];
            let role = if i == 0 {
                Role::Source
            } else {
                Role::Destination
            };
            let ep = pair.endpoint(role);
            let dependencies = if let Some(audio) = &pair.audio {
                vec![(
                    if i == 0 {
                        "audio.capture"
                    } else {
                        "audio.play"
                    }
                    .into(),
                    if i == 0 { "record" } else { "play" }.into(),
                    json!({"node_id":ep.node_id,"resource_id":audio.resource(role)}),
                )]
            } else {
                vec![]
            };
            let permit = self
                .registry
                .reserve_bundle_with_context(
                    &t.capability,
                    &t.action,
                    &t.constraints,
                    t.request_context.as_ref(),
                    &dependencies,
                )
                .context("Peer provider has no scheduling capacity")?;
            let ep = pair.endpoint(if i == 0 {
                Role::Source
            } else {
                Role::Destination
            });
            if permit.provider().node_id != ep.node_id
                || permit.provider().properties["selected_resource_id"] != ep.resource_id
            {
                bail!("Peer reservation cannot rebind the reviewed resource")
            }
            reservations.push(permit);
        }
        let minted = Instant::now();
        let now = eefn::protocol::now_ms();
        let grants = [0, 1].map(|i| -> Result<ExecutionGrant> {
            let mut g = crate::request_approval::scope(&p, i, approval)?;
            g.grant_id = uuid::Uuid::new_v4().to_string();
            g.session_id = sessions[i]["session_id"]
                .as_str()
                .context("Missing peer control session")?
                .into();
            g.issued_at_ms = now;
            g.expires_at_ms = now
                .checked_add(g.limits.max_execution_ms)
                .context("Peer time overflow")?
                .min(approval.expires_at_ms);
            g.validate()?;
            Ok(g)
        });
        let [a, b] = grants;
        let grants = [a?, b?];
        let address: SocketAddr = sessions[1]["destination_address"]
            .as_str()
            .context("Destination has no approved peer route")?
            .parse()?;
        let generations = [
            sessions[0]["runtime_generation"]
                .as_str()
                .context("No source generation")?,
            sessions[1]["runtime_generation"]
                .as_str()
                .context("No destination generation")?,
        ];
        let issuance = Issuance::prepare(
            &p,
            &grants,
            &self.runtime_id,
            generations,
            address,
            &self.server,
        )?;
        let deadline = minted + Duration::from_millis(issuance.scope.expires_at_ms - now);
        let revision = p.revision;
        self.engine.edit_review_policy_checkpoint(
            id,
            "Both peer grants and sealed delivery saved atomically before either dispatch",
            &self.runtime_id,
            &approval.issuer_id,
            |p| {
                if p.revision != revision {
                    bail!("Peer revision changed")
                }
                issue(p, grants.clone(), issuance.clone())?;
                Ok(true)
            },
        )?;
        let (ready, accepted) = tokio::sync::oneshot::channel();
        let observer = Arc::new(ReadyObserver {
            journal: crate::request_execution::AttemptJournal {
                store: self.engine.store.clone(),
                job_id: id.into(),
                grant_id: grants[1].grant_id.clone(),
            },
            ready: Mutex::new(Some(ready)),
        });
        let wait = Duration::from_millis(
            issuance
                .scope
                .expires_at_ms
                .saturating_sub(eefn::protocol::now_ms())
                + 5000,
        );
        let destination_rpc = self.server.invoke_peer_grant_observed(
            grants[1].clone(),
            issuance.scope.clone(),
            issuance.material.destination.clone(),
            wait,
            observer,
            deadline,
        );
        let destination =
            self.wait_with_stop(id, std::slice::from_ref(&grants[1]), destination_rpc);
        tokio::pin!(destination);
        // Destination must bind/retain its route before ACK. Only a durably
        // journaled ACK releases source dispatch; early rejection sends no source.
        tokio::select! {
            biased;
            _=tokio::time::sleep_until(deadline.into())=>bail!("Peer ready deadline expired; no source dispatch"),
            result=&mut destination=>{let _=result;bail!("Destination ended before durable readiness; no source dispatch")},
            ack=accepted=>ack.context("Destination readiness unavailable")?,
        }
        let current = self.engine.store.get(id)?;
        if current.status != PlanStatus::Running
            || current
                .execution
                .as_ref()
                .and_then(|e| e.peer.as_ref())
                .is_none_or(|r| r.scope != issuance.scope)
        {
            self.stop_issued(id, &grants).await?;
            bail!("Peer stopped or changed before source dispatch")
        }
        let source_observer = Arc::new(crate::request_execution::AttemptJournal {
            store: self.engine.store.clone(),
            job_id: id.into(),
            grant_id: grants[0].grant_id.clone(),
        });
        let source = self.server.invoke_peer_grant_observed(
            grants[0].clone(),
            issuance.scope.clone(),
            issuance.material.source.clone(),
            wait,
            source_observer,
            deadline,
        );
        let transfer = async { Ok::<_, anyhow::Error>(tokio::join!(source, destination)) };
        let results = self.wait_with_stop(id, &grants, transfer).await?;
        for (i, result) in [results.0, results.1].into_iter().enumerate() {
            let value =
                result.context("Peer outcome unavailable; inspect both receipts, never replay")?;
            let report: GrantReport = serde_json::from_value(value["grant"].clone())?;
            if !issuance.confirms(&grants[i], &report) {
                bail!("Peer outcome does not confirm the issued pair")
            }
        }
        drop(reservations);
        Ok(())
    }
}

#[cfg(test)]
#[path = "peer_jobs_tests.rs"]
mod tests;
