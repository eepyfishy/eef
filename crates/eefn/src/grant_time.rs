//! Fresh issuer lease challenge; never compare independent machines' wall clocks.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub const MODE: &str = "issuer_lease_challenge_v1";
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PENDING: usize = 64;

#[cfg(test)]
pub(crate) use crate::session_output::WriteGuard;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseQuery {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub request_id: String,
    pub node_id: String,
    pub session_id: String,
    pub grant_id: String,
    pub grant_sha256: String,
}
impl LeaseQuery {
    pub fn validate(&self) -> Result<()> {
        if self.kind != "grant_lease_probe" || self.schema_version != 1 {
            bail!("unsupported lease query")
        }
        crate::job_commands::validate_operation_id(&self.id)?;
        crate::request_drafts::validate_id(&self.request_id)?;
        crate::network::validate_remote_node_id(&self.node_id)?;
        crate::job_commands::validate_operation_id(&self.session_id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if self.grant_sha256.len() != 64
            || !self
                .grant_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("invalid lease scope digest")
        }
        Ok(())
    }
    pub fn confirms(&self, g: &crate::grants::ExecutionGrant) -> bool {
        self.validate().is_ok()
            && self.node_id == g.executor_node
            && self.session_id == g.session_id
            && self.grant_id == g.grant_id
            && crate::grants::definition_digest(&serde_json::json!(g))
                .is_ok_and(|digest| digest == self.grant_sha256)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseReply {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub id: String,
    pub request_id: String,
    pub node_id: String,
    pub session_id: String,
    pub grant_id: String,
    pub grant_sha256: String,
    pub remaining_ms: u64,
}
impl LeaseReply {
    pub fn new(query: &LeaseQuery, remaining_ms: u64) -> Self {
        Self {
            kind: "grant_lease".into(),
            schema_version: 1,
            id: query.id.clone(),
            request_id: query.request_id.clone(),
            node_id: query.node_id.clone(),
            session_id: query.session_id.clone(),
            grant_id: query.grant_id.clone(),
            grant_sha256: query.grant_sha256.clone(),
            remaining_ms,
        }
    }
    fn matches(&self, q: &LeaseQuery) -> bool {
        self.kind == "grant_lease"
            && self.schema_version == 1
            && self.id == q.id
            && self.request_id == q.request_id
            && self.node_id == q.node_id
            && self.session_id == q.session_id
            && self.grant_id == q.grant_id
            && self.grant_sha256 == q.grant_sha256
            && self.remaining_ms <= crate::grants::MAX_DURATION_MS
    }
}

/// Not deserializable or caller-supplied metadata. Created only by the node's
/// authenticated response demultiplexer, with a fresh nonce and local send time.
pub struct GrantLease {
    query: LeaseQuery,
    deadline: Instant,
    remaining_ms: u64,
}
impl GrantLease {
    fn from_reply(q: LeaseQuery, r: LeaseReply, sent: Instant) -> Result<Self> {
        q.validate()?;
        if !r.matches(&q) || r.remaining_ms == 0 || sent.elapsed() >= PROBE_TIMEOUT {
            bail!("lease response expired or scope changed")
        }
        // Starting at local SEND, not receive, subtracts the whole round trip
        // (including writer/issuer queue time). Additional conservative margin
        // covers millisecond rounding and ordinary monotonic timer differences.
        let margin_ms = 5 + r.remaining_ms / 100;
        let budget = r
            .remaining_ms
            .checked_sub(margin_ms)
            .filter(|ms| *ms > 0)
            .ok_or_else(|| anyhow::anyhow!("lease has no conservative budget"))?;
        let deadline = sent
            .checked_add(Duration::from_millis(budget))
            .ok_or_else(|| anyhow::anyhow!("lease time overflow"))?;
        if Instant::now() >= deadline {
            bail!("lease round trip consumed its remaining budget")
        }
        Ok(Self {
            query: q,
            deadline,
            remaining_ms: r.remaining_ms,
        })
    }
    pub(crate) fn deadline(&self, grant: &crate::grants::ExecutionGrant) -> Result<Instant> {
        grant.validate()?;
        if !self.query.confirms(grant) || Instant::now() >= self.deadline {
            bail!("lease proof changed or expired")
        }
        if self.remaining_ms
            > grant
                .limits
                .max_execution_ms
                .min(grant.expires_at_ms - grant.issued_at_ms)
        {
            bail!("lease proof widens the issued duration")
        }
        Ok(self.deadline)
    }
}
struct Pending {
    query: LeaseQuery,
    sent: Instant,
    sent_wall: u64,
    sender: oneshot::Sender<Result<GrantLease>>,
}
#[derive(Default)]
pub(crate) struct LeaseRequests {
    pending: Mutex<HashMap<String, Pending>>,
}
struct Cleanup {
    manager: Arc<LeaseRequests>,
    id: String,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.manager.pending.lock().unwrap().remove(&self.id);
    }
}
impl LeaseRequests {
    pub fn complete(&self, value: Value) {
        let Ok(reply) = serde_json::from_value::<LeaseReply>(value) else {
            return;
        };
        let mut pending = self.pending.lock().unwrap();
        if !pending
            .get(&reply.id)
            .is_some_and(|p| reply.matches(&p.query))
        {
            return;
        }
        let entry = pending.remove(&reply.id).unwrap();
        let expected_wall = entry
            .sent_wall
            .saturating_add(entry.sent.elapsed().as_millis().min(u64::MAX as u128) as u64);
        let lease = if crate::protocol::now_ms().abs_diff(expected_wall) > 1000 {
            Err(anyhow::anyhow!(
                "local clock changed during lease challenge"
            ))
        } else {
            GrantLease::from_reply(entry.query, reply, entry.sent)
        };
        let _ = entry.sender.send(lease);
    }
    pub async fn request<F, Fut>(
        self: &Arc<Self>,
        grant: &crate::grants::ExecutionGrant,
        request_id: &str,
        write: F,
    ) -> Result<GrantLease>
    where
        F: FnOnce(Value) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let q = LeaseQuery {
            kind: "grant_lease_probe".into(),
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            request_id: request_id.into(),
            node_id: grant.executor_node.clone(),
            session_id: grant.session_id.clone(),
            grant_id: grant.grant_id.clone(),
            grant_sha256: crate::grants::definition_digest(&serde_json::json!(grant))?,
        };
        q.validate()?;
        let (sender, receiver) = oneshot::channel();
        let sent = Instant::now();
        {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, p| !p.sender.is_closed());
            if pending.len() >= MAX_PENDING {
                bail!("lease probe capacity full")
            }
            pending.insert(
                q.id.clone(),
                Pending {
                    query: q.clone(),
                    sent,
                    sent_wall: crate::protocol::now_ms(),
                    sender,
                },
            );
        }
        let _cleanup = Cleanup {
            manager: self.clone(),
            id: q.id.clone(),
        };
        tokio::time::timeout(PROBE_TIMEOUT, async {
            write(serde_json::json!(q)).await?;
            receiver
                .await
                .map_err(|_| anyhow::anyhow!("lease session ended"))?
        })
        .await
        .map_err(|_| anyhow::anyhow!("lease challenge timed out"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn query(g: &crate::grants::ExecutionGrant) -> LeaseQuery {
        LeaseQuery {
            kind: "grant_lease_probe".into(),
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            request_id: uuid::Uuid::new_v4().simple().to_string(),
            node_id: g.executor_node.clone(),
            session_id: g.session_id.clone(),
            grant_id: g.grant_id.clone(),
            grant_sha256: crate::grants::definition_digest(&json!(g)).unwrap(),
        }
    }
    #[tokio::test]
    async fn offset_wall_clocks_do_not_change_authority_and_durable_nonce_still_prevents_replay() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = crate::grants::GrantLedger::open(
            &dir.path().join("receipts.db"),
            Default::default(),
            "owner",
        )
        .unwrap();
        let engine = crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap();
        for offset in [-172_800_000i64, 172_800_000] {
            let mut g = crate::grants::tests::grant();
            g.issued_at_ms = crate::protocol::now_ms()
                .checked_add_signed(offset)
                .unwrap();
            g.expires_at_ms = g.issued_at_ms + 10_000;
            g.validate().unwrap();
            let q = query(&g);
            let r = LeaseReply::new(&q, 4000);
            let proof = GrantLease::from_reply(q, r, Instant::now()).unwrap();
            let accepted = crate::grants::AcceptedGrant::prepare_with_lease(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner",
                proof,
            )
            .unwrap();
            assert_eq!(
                accepted.run(&engine).await.state,
                crate::grants::ReceiptState::Completed
            );
            assert_eq!(
                ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
                "completed"
            );
            let q = query(&g);
            let proof =
                GrantLease::from_reply(q.clone(), LeaseReply::new(&q, 4000), Instant::now())
                    .unwrap();
            assert!(
                crate::grants::AcceptedGrant::prepare_with_lease(
                    g.clone(),
                    ledger.clone(),
                    &engine,
                    &g.issuer_id,
                    &g.session_id,
                    "owner",
                    proof
                )
                .is_err()
            );
        }
    }
    #[test]
    fn lease_scope_round_trip_expiry_and_duration_cannot_be_widened() {
        let g = crate::grants::tests::grant();
        let q = query(&g);
        let reply = LeaseReply::new(&q, 1000);
        let proof = GrantLease::from_reply(q.clone(), reply.clone(), Instant::now()).unwrap();
        assert!(proof.deadline(&g).unwrap() <= Instant::now() + Duration::from_millis(985));
        let mut changed = g.clone();
        changed.params = json!({"extra":true});
        assert!(proof.deadline(&changed).is_err());
        for field in [
            "id",
            "request_id",
            "node_id",
            "session_id",
            "grant_id",
            "grant_sha256",
        ] {
            let mut bad = json!(reply);
            bad[field] = json!("other");
            assert!(
                GrantLease::from_reply(
                    q.clone(),
                    serde_json::from_value(bad).unwrap(),
                    Instant::now()
                )
                .is_err(),
                "{field}"
            );
        }
        for remaining in [0, 2, crate::grants::MAX_DURATION_MS + 1] {
            assert!(
                GrantLease::from_reply(q.clone(), LeaseReply::new(&q, remaining), Instant::now())
                    .is_err()
            );
        }
        assert!(
            GrantLease::from_reply(q.clone(), reply, Instant::now() - Duration::from_secs(1))
                .is_err()
        );
        let widened =
            GrantLease::from_reply(q.clone(), LeaseReply::new(&q, 6000), Instant::now()).unwrap();
        assert!(widened.deadline(&g).is_err());
        let mut extra = json!(LeaseReply::new(&q, 1000));
        extra["extend_until"] = json!(123);
        assert!(serde_json::from_value::<LeaseReply>(extra).is_err());
    }
    #[tokio::test]
    async fn expired_proof_never_claims_and_ack_delay_never_restarts_authority() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = crate::grants::GrantLedger::open(
            &dir.path().join("receipts.db"),
            Default::default(),
            "owner",
        )
        .unwrap();
        let policy: crate::NodePolicy = serde_json::from_value(json!({
            "filesystem":{"write":true,"roots":[dir.path()]}
        }))
        .unwrap();
        let engine = crate::NodeEngine::new(false, vec![], dir.path().into())
            .unwrap()
            .with_policy(policy)
            .unwrap();
        let mut g = crate::grants::tests::grant();
        let output = dir.path().join("must-not-be-written.txt");
        g.capability = "filesystem".into();
        g.action = "write".into();
        g.params = json!({"path":output,"content":"expired work"});
        let q = query(&g);
        let mut expired =
            GrantLease::from_reply(q.clone(), LeaseReply::new(&q, 1000), Instant::now()).unwrap();
        expired.deadline = Instant::now() - Duration::from_secs(1);
        assert!(
            crate::grants::AcceptedGrant::prepare_with_lease(
                g.clone(),
                ledger.clone(),
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner",
                expired,
            )
            .is_err()
        );
        assert!(ledger.receipt(&g.issuer_id, &g.grant_id).is_err());
        assert!(!output.exists());

        let proof =
            GrantLease::from_reply(q.clone(), LeaseReply::new(&q, 1000), Instant::now()).unwrap();
        let deadline = proof.deadline;
        let accepted = crate::grants::AcceptedGrant::prepare_with_lease(
            g.clone(),
            ledger.clone(),
            &engine,
            &g.issuer_id,
            &g.session_id,
            "owner",
            proof,
        )
        .unwrap();
        assert!(accepted.acknowledgement().confirms(&g));
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "accepted"
        );
        // Model ACK/queue residence after durable consumption, before effects.
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            deadline + Duration::from_millis(1),
        ))
        .await;
        let report = accepted.run(&engine).await;
        assert_eq!(report.state, crate::grants::ReceiptState::Unknown);
        assert_eq!(
            ledger.receipt(&g.issuer_id, &g.grant_id).unwrap()["state"],
            "unknown"
        );
        assert!(!output.exists());
        let fresh =
            GrantLease::from_reply(q.clone(), LeaseReply::new(&q, 1000), Instant::now()).unwrap();
        assert!(
            crate::grants::AcceptedGrant::prepare_with_lease(
                g.clone(),
                ledger,
                &engine,
                &g.issuer_id,
                &g.session_id,
                "owner",
                fresh,
            )
            .is_err()
        );
        assert!(!output.exists());
    }
    #[tokio::test]
    async fn response_demux_keeps_wrong_scope_out_and_cancellation_cleans_pending_challenges() {
        let manager = Arc::new(LeaseRequests::default());
        let g = crate::grants::tests::grant();
        let wire = uuid::Uuid::new_v4().simple().to_string();
        let proof = manager
            .request(&g, &wire, |value| async {
                let q: LeaseQuery = serde_json::from_value(value).unwrap();
                let mut bad = LeaseReply::new(&q, 1000);
                bad.session_id = uuid::Uuid::new_v4().to_string();
                manager.complete(json!(bad));
                assert_eq!(manager.pending.lock().unwrap().len(), 1);
                let mut extra = json!(LeaseReply::new(&q, 1000));
                extra["renew"] = json!(true);
                manager.complete(extra);
                assert_eq!(manager.pending.lock().unwrap().len(), 1);
                manager.complete(json!(LeaseReply::new(&q, 1000)));
                manager.complete(json!(LeaseReply::new(&q, 2000)));
                Ok(())
            })
            .await
            .unwrap();
        assert!(proof.deadline(&g).is_ok());
        assert!(manager.pending.lock().unwrap().is_empty());
        let moved = manager
            .request(&g, &wire, |value| async {
                let q: LeaseQuery = serde_json::from_value(value).unwrap();
                manager
                    .pending
                    .lock()
                    .unwrap()
                    .get_mut(&q.id)
                    .unwrap()
                    .sent_wall += 4000;
                manager.complete(json!(LeaseReply::new(&q, 1000)));
                Ok(())
            })
            .await;
        assert!(moved.is_err());
        assert!(manager.pending.lock().unwrap().is_empty());
        let (sent, seen) = oneshot::channel();
        let local = manager.clone();
        let copy = g.clone();
        let wire = wire.clone();
        let task = tokio::spawn(async move {
            local
                .request(&copy, &wire, |value| async move {
                    sent.send(value).unwrap();
                    std::future::pending::<Result<()>>().await
                })
                .await
        });
        let late: LeaseQuery = serde_json::from_value(seen.await.unwrap()).unwrap();
        assert_eq!(manager.pending.lock().unwrap().len(), 1);
        task.abort();
        let _ = task.await;
        assert!(manager.pending.lock().unwrap().is_empty());
        manager.complete(json!(LeaseReply::new(&late, 1000)));
        assert!(manager.pending.lock().unwrap().is_empty());
        let (signal, receiver) = tokio::sync::watch::channel(false);
        drop(WriteGuard::new(signal.clone()));
        assert!(*receiver.borrow());
        signal.send_replace(false);
        let mut good = WriteGuard::new(signal);
        good.complete();
        drop(good);
        assert!(!*receiver.borrow());
    }
}
