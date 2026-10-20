//! Optional read control kept out of the main connection future's inline frame.
use super::*;

impl NodeClient {
    pub(super) async fn dispatch_stop_inspection(
        &self,
        message: Value,
        writer: &Arc<Mutex<tokio::io::WriteHalf<crate::tls::NodeStream>>>,
        write_failed: &tokio::sync::watch::Sender<bool>,
        tasks: &mut tokio::task::JoinSet<()>,
        issuer: Option<&str>,
    ) -> Result<()> {
        let request_id = message["id"].as_str().unwrap_or("").to_owned();
        let parsed =
            serde_json::from_value::<crate::grants::stop::inspection::WireRequest>(message);
        let prepared = match (
            parsed,
            self.request_budget.admit_receipt(),
            self.grant_ledger.clone(),
            issuer.map(str::to_owned),
        ) {
            (Ok(request), Some(permit), Some(ledger), Some(issuer))
                if request.validate(&issuer, &self.config.node_id).is_ok() =>
            {
                Some((request, permit, ledger, issuer))
            }
            _ => None,
        };
        if let Some((request, permit, ledger, issuer)) = prepared {
            let crypto = self.crypto.clone();
            let writer = writer.clone();
            let failed = write_failed.clone();
            let node = self.config.node_id.clone();
            tasks.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let result = ledger.inspect_stop(&request.scope, &issuer, &node);
                    (permit, result)
                }).await;
                let (held, response) = match result {
                    Ok((permit, Ok(report))) => (Some(permit), success_response(&request_id, json!(report))),
                    Ok((permit, Err(_))) => (Some(permit), json!({"type":"response","id":request_id,"success":false,"error_type":"StopInspectionUnavailable","error":"Retained stop inspection unconfirmed; no signal, execution or fallback."})),
                    Err(_) => (None, json!({"type":"response","id":request_id,"success":false,"error_type":"StopInspectionUnavailable","error":"Stop inspection callback unavailable; no mutation requested."})),
                };
                let _held = held;
                let _ = write_session_frame(&crypto, &writer, &failed, &response).await;
            });
        } else {
            let response = json!({"type":"response","id":request_id,"success":false,"error_type":"StopInspectionDenied","error":"Read-only stop inspection requires the original pinned issuer, exact node and bounded control capacity."});
            write_session_frame(&self.crypto, writer, write_failed, &response).await?;
        }
        Ok(())
    }
}
