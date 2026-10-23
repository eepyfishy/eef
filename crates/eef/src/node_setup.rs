//! Owner read adapter for the node's shared setup service. Never a setup runner.
use anyhow::{Result, bail};
use eefn::setup_check::CheckRequest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub node_id: String,
    pub expected_runtime_id: Option<String>,
    pub request_id: String,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        eefn::network::validate_remote_node_id(&self.node_id)?;
        eefn::automation_occurrence::uuid(&self.request_id)?;
        if let Some(runtime) = &self.expected_runtime_id {
            eefn::automation_occurrence::uuid(runtime)?;
        }
        if self.schema_version != 1 {
            bail!("Unsupported node setup inspection schema")
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    schema_version: u32,
    report_type: String,
    node_id: String,
    request_id: String,
    runtime_id: Option<String>,
    success: bool,
    inspection_requested: bool,
    result: Option<Value>,
    changed: bool,
    model_inference_requested: bool,
    execution_authorized: bool,
    restart_requested: bool,
    automatically_retried: bool,
}
pub fn confirmed(value: &Value, request: &Request) -> bool {
    let Ok(report) = serde_json::from_value::<Report>(value.clone()) else {
        return false;
    };
    let Some(runtime) = report.runtime_id.as_deref() else {
        return false;
    };
    request.validate().is_ok()
        && report.schema_version == 1
        && report.report_type == "remote_node_setup"
        && report.node_id == request.node_id
        && report.request_id == request.request_id
        && report.success
        && report.inspection_requested
        && !report.changed
        && !report.model_inference_requested
        && !report.execution_authorized
        && !report.restart_requested
        && !report.automatically_retried
        && request
            .expected_runtime_id
            .as_deref()
            .is_none_or(|expected| expected == runtime)
        && report.result.as_ref().is_some_and(|result| {
            eefn::setup_check::confirmed(
                result,
                &CheckRequest {
                    schema_version: 1,
                    expected_node_id: request.node_id.clone(),
                    expected_runtime_id: runtime.into(),
                    request_id: request.request_id.clone(),
                },
            )
        })
}
#[async_trait::async_trait]
trait SetupTransport: Sync {
    async fn read(&self, node: &str, action: &str, params: Value, wait: Duration) -> Result<Value>;
}
#[async_trait::async_trait]
impl SetupTransport for eefn::NodeServer {
    async fn read(&self, node: &str, action: &str, params: Value, wait: Duration) -> Result<Value> {
        self.invoke_remote(node, "node.configure", action, params, wait)
            .await
    }
}
pub async fn command(server: &eefn::NodeServer, request: Request) -> Result<Value> {
    command_on(server, request).await
}
async fn command_on(server: &impl SetupTransport, request: Request) -> Result<Value> {
    request.validate()?;
    let mut report = Report {
        schema_version: 1,
        report_type: "remote_node_setup".into(),
        node_id: request.node_id.clone(),
        request_id: request.request_id.clone(),
        runtime_id: None,
        success: false,
        inspection_requested: false,
        result: None,
        changed: false,
        model_inference_requested: false,
        execution_authorized: false,
        restart_requested: false,
        automatically_retried: false,
    };
    let runtime = if let Some(runtime) = &request.expected_runtime_id {
        runtime.clone()
    } else {
        let Ok(status) = server
            .read(
                &request.node_id,
                "status",
                json!({}),
                Duration::from_secs(5),
            )
            .await
        else {
            return Ok(json!(report));
        };
        if !crate::node_control::control_status(&status, &request.node_id) {
            return Ok(json!(report));
        }
        let Some(runtime) = status["data"]["runtime_id"].as_str() else {
            return Ok(json!(report));
        };
        runtime.into()
    };
    let node = CheckRequest {
        schema_version: 1,
        expected_node_id: request.node_id.clone(),
        expected_runtime_id: runtime.clone(),
        request_id: request.request_id.clone(),
    };
    if node.validate().is_err() {
        return Ok(json!(report));
    }
    report.runtime_id = Some(runtime);
    report.inspection_requested = true;
    // Resolve at most once and send this exact read once. A lost/unsupported
    // reply never triggers a legacy get, another runtime lookup or setup action.
    if let Ok(reply) = server
        .read(
            &request.node_id,
            "setup_check",
            json!(node),
            Duration::from_secs(10),
        )
        .await
    {
        if reply["success"] == true && eefn::setup_check::confirmed(&reply["data"], &node) {
            report.result = Some(reply["data"].clone());
            report.success = true;
        }
    }
    let value = json!(report);
    if value["success"] == true && !confirmed(&value, &request) {
        bail!("Node setup observation unconfirmed");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Peer {
        status: Value,
        reply: Option<Value>,
        calls: Mutex<Vec<(String, Value)>>,
    }
    #[async_trait::async_trait]
    impl SetupTransport for Peer {
        async fn read(&self, _: &str, action: &str, params: Value, _: Duration) -> Result<Value> {
            self.calls.lock().unwrap().push((action.into(), params));
            if action == "status" {
                Ok(self.status.clone())
            } else {
                self.reply
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("PRIVATE lost reply"))
            }
        }
    }
    fn fixture() -> (tempfile::TempDir, Request, Peer) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        std::fs::write(
            &config,
            b"{\"node_id\":\"node\",\"models\":{\"provider\":\"auto\"},\"psk\":\"PRIVATE\"}",
        )
        .unwrap();
        let service = eefn::NodeService::new(config, "node".into());
        let runtime = uuid::Uuid::new_v4().to_string();
        service.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let request = Request {
            schema_version: 1,
            node_id: "node".into(),
            expected_runtime_id: None,
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        let node = CheckRequest {
            schema_version: 1,
            expected_node_id: "node".into(),
            expected_runtime_id: runtime.clone(),
            request_id: request.request_id.clone(),
        };
        let result = service.setup_check(node).unwrap();
        (
            dir,
            request,
            Peer {
                status: json!({"success":true,"data":{"control_schema_version":1,"node_id":"node","runtime_id":runtime,"remote_allowed":false}}),
                reply: Some(json!({"success":true,"data":result})),
                calls: Mutex::new(vec![]),
            },
        )
    }
    #[tokio::test]
    async fn setup_read_resolves_once_and_checks_exact_node_runtime_and_correlation() {
        let (_dir, request, peer) = fixture();
        let report = command_on(&peer, request.clone()).await.unwrap();
        assert!(confirmed(&report, &request));
        let calls = peer.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "status");
        assert_eq!(calls[1].0, "setup_check");
        assert_eq!(calls[1].1["request_id"], request.request_id);
        assert!(!report.to_string().contains("PRIVATE"));
        for pointer in [
            "/changed",
            "/model_inference_requested",
            "/execution_authorized",
            "/restart_requested",
            "/automatically_retried",
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = json!(true);
            assert!(!confirmed(&bad, &request));
        }
        for pointer in [
            "/request_id",
            "/runtime_id",
            "/result/runtime_id",
            "/result/request_id",
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = json!(uuid::Uuid::new_v4().to_string());
            assert!(!confirmed(&bad, &request));
        }
    }
    #[tokio::test]
    async fn lost_corrupt_and_unsupported_reads_never_retry_fallback_or_publish_private_replies() {
        for mode in [
            "lost",
            "unsupported",
            "wrong-node",
            "wrong-runtime",
            "wrong-correlation",
            "future",
        ] {
            let (_dir, request, mut peer) = fixture();
            match mode {
                "lost" => peer.reply = None,
                "unsupported" => {
                    peer.reply = Some(json!({"success":false,"error":"PRIVATE unsupported"}))
                }
                "wrong-node" => peer.reply.as_mut().unwrap()["data"]["node_id"] = json!("other"),
                "wrong-runtime" => {
                    peer.reply.as_mut().unwrap()["data"]["runtime_id"] =
                        json!(uuid::Uuid::new_v4().to_string())
                }
                "wrong-correlation" => {
                    peer.reply.as_mut().unwrap()["data"]["request_id"] =
                        json!(uuid::Uuid::new_v4().to_string())
                }
                _ => peer.reply.as_mut().unwrap()["data"]["schema_version"] = json!(99),
            }
            let report = command_on(&peer, request).await.unwrap();
            assert_eq!(report["success"], false, "{mode}");
            assert!(report["result"].is_null());
            assert!(!report.to_string().contains("PRIVATE"));
            assert_eq!(
                peer.calls
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|c| c.0.as_str())
                    .collect::<Vec<_>>(),
                vec!["status", "setup_check"]
            );
        }
    }
    #[tokio::test]
    async fn explicit_runtime_is_never_replaced_and_invalid_discovery_stops_before_setup() {
        let (_dir, mut request, peer) = fixture();
        request.expected_runtime_id = peer.status["data"]["runtime_id"]
            .as_str()
            .map(str::to_owned);
        assert!(confirmed(
            &command_on(&peer, request.clone()).await.unwrap(),
            &request
        ));
        assert_eq!(peer.calls.lock().unwrap().len(), 1);
        let (_dir, mut request, peer) = fixture();
        let expected = uuid::Uuid::new_v4().to_string();
        request.expected_runtime_id = Some(expected.clone());
        let report = command_on(&peer, request).await.unwrap();
        assert_eq!(report["success"], false);
        assert!(report["result"].is_null());
        let calls = peer.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "setup_check");
        assert_eq!(calls[0].1["expected_runtime_id"], expected);
        drop(calls);
        for field in ["node_id", "runtime_id", "control_schema_version"] {
            let (_dir, request, mut peer) = fixture();
            peer.status["data"][field] = json!("unsupported");
            let report = command_on(&peer, request).await.unwrap();
            assert_eq!(report["success"], false);
            assert_eq!(peer.calls.lock().unwrap().len(), 1);
        }
        let (_dir, mut request, peer) = fixture();
        request.request_id = "not-a-uuid".into();
        assert!(command_on(&peer, request).await.is_err());
        assert!(peer.calls.lock().unwrap().is_empty());
    }
}
