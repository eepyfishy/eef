//! Keep read control outside the connection future's inline frame.
use super::*;

impl NodeClient {
    pub(super) async fn dispatch_workload_inspection(
        &self,
        message: Value,
        writer: &Arc<Mutex<tokio::io::WriteHalf<crate::tls::NodeStream>>>,
        write_failed: &tokio::sync::watch::Sender<bool>,
        tasks: &mut tokio::task::JoinSet<()>,
        issuer: Option<&str>,
    ) -> Result<()> {
        let request_id = message["id"].as_str().unwrap_or("").to_owned();
        let parsed = match message["type"].as_str() {
            Some("workload_analyses") => {
                serde_json::from_value::<crate::vision_result_inspection::WireRequest>(message)
                    .map(Inspection::Analyses)
            }
            Some(crate::workload_stop_exit::KIND) => {
                serde_json::from_value::<crate::workload_inspection::WireRequest>(message)
                    .map(Inspection::StopExit)
            }
            _ => serde_json::from_value::<crate::workload_inspection::WireRequest>(message)
                .map(Inspection::Receipt),
        };
        let prepared = match (
            parsed,
            self.request_budget.admit_receipt(),
            self.grant_ledger.clone(),
            issuer.map(str::to_owned),
        ) {
            (Ok(request), Some(permit), Some(ledger), Some(issuer))
                if self.tls_config.is_some()
                    && request.validate(&issuer, &self.config.node_id).is_ok() =>
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
            let (execution, reply) = permit.into_parts();
            tasks.spawn(async move {
                // Capacity stays owned by the actual reader, even if its waiter
                // disconnects. No source factory, lease or journal mutator here.
                let read = tokio::task::spawn_blocking(move || (execution, request.read(&ledger, &issuer, &node))).await;
                let (held, response) = match read {
                    Ok((permit, Ok(receipt))) => (Some(permit), success_response(&request_id, json!(receipt))),
                    Ok((permit, Err(_))) => (Some(permit), json!({"type":"response","id":request_id,"success":false,"error_type":"WorkloadReceiptUnavailable","error":"Retained workload evidence unavailable; no execution, renewal or fallback."})),
                    Err(_) => (None, json!({"type":"response","id":request_id,"success":false,"error_type":"WorkloadReceiptUnavailable","error":"Workload receipt reader unavailable; no recovery requested."})),
                };
                drop(held);
                let _ = write_reply_frame(&crypto, &writer, &failed, &response, Some(reply)).await;
            });
        } else {
            let response = json!({"type":"response","id":request_id,"success":false,"error_type":"WorkloadReceiptDenied","error":"Workload inspection requires pinned credentials, exact scope and bounded read capacity."});
            write_session_frame(&self.crypto, writer, write_failed, &response).await?;
        }
        Ok(())
    }
}

enum Inspection {
    Receipt(crate::workload_inspection::WireRequest),
    Analyses(crate::vision_result_inspection::WireRequest),
    StopExit(crate::workload_inspection::WireRequest),
}
impl Inspection {
    fn validate(&self, issuer: &str, node: &str) -> Result<()> {
        match self {
            Self::Receipt(r) => r.validate(issuer, node),
            Self::Analyses(r) => r.validate(issuer, node),
            Self::StopExit(r) => r.validate_kind(crate::workload_stop_exit::KIND, issuer, node),
        }
    }
    fn read(&self, ledger: &crate::grants::GrantLedger, issuer: &str, node: &str) -> Result<Value> {
        match self {
            Self::Receipt(r) => Ok(json!(r.read(ledger, issuer, node)?)),
            Self::Analyses(r) => Ok(json!(r.read(ledger, issuer, node)?)),
            Self::StopExit(r) => {
                r.validate_kind(crate::workload_stop_exit::KIND, issuer, node)?;
                Ok(json!(ledger.inspect_workload_stop_exit(&r.scope)?))
            }
        }
    }
}
