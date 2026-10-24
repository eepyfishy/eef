//! Native grant delivery. All work remains owned by the current control
//! connection's JoinSet; reconnect cannot resume or replay a consumed pair.
use super::*;
use crate::{
    grants::{GrantDenied, GrantReport, ReceiptState},
    peer::{
        Recipient, SealedMaterial,
        admission::{AdmittedPeer, Definition},
        scope::Scope,
    },
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    scope: Scope,
    material: SealedMaterial,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    grant: crate::grants::ExecutionGrant,
    peer: Delivery,
}

impl NodeClient {
    pub(super) fn session_capabilities(
        &self,
        models: &[Value],
        peer: Option<&crate::peer::plan::Advertisement>,
    ) -> Vec<String> {
        let mut caps = advertised_capabilities(&self.engine.capabilities(), models);
        if peer.is_some() {
            caps.push("peer.transfer".into());
            caps.sort();
            caps.dedup();
        }
        caps
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn dispatch_peer(
        &self,
        message: Value,
        writer: &Arc<Mutex<tokio::io::WriteHalf<crate::tls::NodeStream>>>,
        write_failed: &tokio::sync::watch::Sender<bool>,
        tasks: &mut tokio::task::JoinSet<()>,
        issuer: Option<&str>,
        session: Option<&str>,
        coordinator_runtime: Option<&str>,
        advertised: bool,
        leases: Arc<crate::grant_time::LeaseRequests>,
    ) -> Result<()> {
        let id = message["id"].as_str().unwrap_or("").to_owned();
        let parsed = serde_json::from_value::<Request>(message);
        let (
            Ok(request),
            Some(ledger),
            Some(issuer),
            Some(session),
            Some(epoch),
            Some(runtime),
            Some(generation),
        ) = (
            parsed,
            self.grant_ledger.clone(),
            issuer,
            session,
            coordinator_runtime,
            self.peer_runtime.clone(),
            self.registration_runtime.as_deref(),
        )
        else {
            let response = json!({"type":"response","id":id,"success":false,"error_type":"GrantDenied",
                "not_started":true,"error":"Peer delivery requires an available producer binding, journal and current pinned issuer session"});
            return write_session_frame(&self.crypto, writer, write_failed, &response).await;
        };
        let grant = request.grant;
        let permit = self.request_budget.admit(&grant.capability, &grant.action);
        let checked = (|| -> Result<Option<Arc<[u8]>>> {
            if !advertised
                || self.tls_config.is_none()
                || request.kind != "execute_peer_grant"
                || crate::request_drafts::validate_id(&request.id).is_err()
                || !grant.matches_session(issuer, session, &self.config.node_id)
            {
                return Err(GrantDenied("grant_identity_changed").into());
            }
            if permit.is_none() {
                return Err(GrantDenied("node_grant_capacity_full").into());
            }
            grant
                .validate()
                .map_err(|_| GrantDenied("invalid_grant_scope"))?;
            self.engine
                .check_grant_policy(&grant)
                .map_err(|_| GrantDenied("local_policy_denied_or_changed"))?;
            let (role, definition) = Definition::from_grant(&grant)
                .map_err(|_| GrantDenied("invalid_peer_definition"))?;
            runtime
                .payload(role, &definition, &request.peer.scope)
                .map_err(|_| GrantDenied("peer_producer_or_route_changed").into())
        })();
        let source_payload = match checked {
            Ok(payload) => payload,
            Err(error) => {
                let reply = permit.map(|permit| permit.into_reply());
                let report = GrantReport::new(
                    &grant,
                    ReceiptState::Rejected,
                    Value::Null,
                    Some(crate::grants::denial_code(&error)),
                );
                return write_reply_frame(
                    &self.crypto,
                    writer,
                    write_failed,
                    &json!({"type":"response","id":id,"success":false,"grant":report}),
                    reply,
                )
                .await;
            }
        };
        let config = self.config.clone();
        let epoch = epoch.to_owned();
        let generation = generation.to_owned();
        let session = session.to_owned();
        let engine = self.engine.clone();
        let crypto = self.crypto.clone();
        let writer = writer.clone();
        let write_failed = write_failed.clone();
        let status = self.status.clone();
        tasks.spawn(async move {
            let lease = leases.request(&grant, &id, |query| {
                let (crypto,writer,write_failed) = (&crypto,&writer,&write_failed);
                async move { write_session_frame(crypto,writer,write_failed,&query).await }
            }).await;
            let admitted = lease.map_err(|_| GrantDenied("grant_lease_unavailable").into()).and_then(|lease|
                AdmittedPeer::prepare(grant.clone(), ledger, &engine, &request.peer.scope, &request.peer.material,
                    Recipient { auth_mode: config.auth_mode, node_id: &config.node_id,
                        control_session_id: &session, runtime_generation: &generation, coordinator_runtime_id: &epoch,
                        coordinator_certificate: config.coordinator_certificate.as_deref().unwrap_or(""), credential: &config.psk }, lease));
            let report = match admitted {
                Err(error) => GrantReport::new(&grant, ReceiptState::Rejected, Value::Null, Some(crate::grants::denial_code(&error))),
                Ok(admitted) => admitted.run_tcp(&engine, source_payload, |ack| {
                    let (crypto, writer, write_failed, status, grant, id) = (&crypto, &writer, &write_failed, &status, &grant, &id);
                    async move {
                    write_session_frame(&crypto,&writer,&write_failed,&json!({"type":"grant_ack","id":id,"grant":ack})).await?;
                    let mut live = status.lock().expect("status");
                    live["last_activity"] = json!(grant.capability);
                    live["last_request_context"] = json!(grant.request_context);
                    live["last_resource_id"] = json!(grant.resource_id);
                    live["last_workload"] = json!({"schema_version":1,"grant_id":grant.grant_id,
                        "capability":grant.capability,"request_context":grant.request_context,"resource_id":grant.resource_id,
                        "model_id":grant.model_id,"backend":grant.backend,"execution_may_have_started":true});
                    Ok(())
                }}).await,
            };
            status.lock().expect("status")["last_grant"] = json!({"grant_id":report.grant_id,"state":report.state,"persisted":report.persisted,"success":report.success});
            let reply=permit.map(|permit|permit.into_reply());
            // A pre-readiness bind failure is consumed Unknown, never Rejected.
            let _ = write_reply_frame(&crypto,&writer,&write_failed,
                &json!({"type":"response","id":id,"success":report.success,"grant":report}),reply).await;
        });
        Ok(())
    }
}
