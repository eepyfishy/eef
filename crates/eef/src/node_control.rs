//! Owner commands over the existing authenticated node channel. Never replay restart.
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartRequest {
    pub node_id: String,
    #[serde(default = "default_wait")]
    pub wait_seconds: u64,
}
fn default_wait() -> u64 {
    30
}

pub async fn restart(server: &eefn::NodeServer, request: RestartRequest) -> Result<Value> {
    restart_on(server, request).await
}

#[async_trait::async_trait]
trait NodeControl: Sync {
    async fn call(&self, id: &str, action: &str, wait: Duration) -> Result<Value>;
}
#[async_trait::async_trait]
impl NodeControl for eefn::NodeServer {
    async fn call(&self, id: &str, action: &str, wait: Duration) -> Result<Value> {
        self.invoke_remote(id, "node.configure", action, json!({}), wait)
            .await
    }
}

async fn restart_on(server: &impl NodeControl, request: RestartRequest) -> Result<Value> {
    eefn::network::validate_node_id(&request.node_id)?;
    if request.wait_seconds > 60 {
        bail!("wait_seconds must be 0-60")
    }
    let id = &request.node_id;
    let mut result = json!({"schema_version":1,"success":false,"operation_id":uuid::Uuid::new_v4().to_string(),
        "node_id":id,"restart_requested":false,"acknowledged":false,"completed":false,
        "outcome_unknown":false});
    let permitted = server.call(id, "get", Duration::from_secs(2)).await?;
    if permitted["success"] != true {
        bail!("node management status unavailable")
    }
    if permitted.pointer("/data/remote_allowed") != Some(&json!(true)) {
        result["error_code"] = json!("approval_required");
        result["note"] =
            json!("Enable remote management in the local node app before requesting a restart.");
        return Ok(result);
    }
    let before = runtime_id(server, id).await?;
    result["previous_runtime_id"] = json!(before);
    result["restart_requested"] = json!(true);
    let reply = server.call(id, "restart", Duration::from_secs(3)).await;
    match reply {
        Ok(value)
            if value["success"] == true
                && value.pointer("/data/restarting") == Some(&json!(true)) =>
        {
            result["acknowledged"] = json!(true)
        }
        Ok(value) if value["success"] == false => {
            result["error_code"] = json!("restart_rejected");
            result["note"] =
                json!("The node rejected restart. Recheck local approval; no retry was sent.");
            return Ok(result);
        }
        _ => {
            result["outcome_unknown"] = json!(true);
        }
    }
    if request.wait_seconds == 0 && result["acknowledged"] == true {
        result["success"] = json!(true);
        result["note"] = json!("Restart acknowledged; completion was not requested or verified.");
        return Ok(result);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(request.wait_seconds);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(current)) = tokio::time::timeout_at(deadline, runtime_id(server, id)).await {
            if current != before {
                result["success"] = json!(true);
                result["completed"] = json!(true);
                result["outcome_unknown"] = json!(false);
                result["runtime_id"] = json!(current);
                result["note"] = json!(
                    "Same stable node ID reconnected with a new runtime ID. No workload correctness claim."
                );
                return Ok(result);
            }
        }
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(400)).min(deadline),
        )
        .await;
    }
    result["error_code"] = json!("restart_unconfirmed");
    result["outcome_unknown"] = json!(true);
    result["note"] = json!(
        "Restart completion could not be confirmed within the wait limit. It may still finish; inspect diagnostics, do not automatically replay."
    );
    Ok(result)
}

async fn runtime_id(server: &impl NodeControl, id: &str) -> Result<String> {
    let response = server
        .call(id, "diagnostics", Duration::from_secs(2))
        .await?;
    let data = &response["data"];
    if response["success"] != true
        || data["node_id"] != id
        || data["connection_state"] != "connected"
    {
        bail!("connected node diagnostics unavailable")
    }
    let runtime = data["runtime_id"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok());
    runtime
        .map(|id| id.to_string())
        .ok_or_else(|| anyhow::anyhow!("node requires runtime-aware diagnostics"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    struct Fake(Mutex<VecDeque<(&'static str, Option<Value>)>>);
    #[async_trait::async_trait]
    impl NodeControl for Fake {
        async fn call(&self, id: &str, action: &str, _wait: Duration) -> Result<Value> {
            assert_eq!(id, "node-b");
            let (expected, value) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected call or unsafe replay");
            assert_eq!(action, expected);
            value.ok_or_else(|| anyhow::anyhow!("reply lost"))
        }
    }
    fn response(value: Value) -> Option<Value> {
        Some(json!({"success":true,"data":value}))
    }
    fn diagnostics(id: uuid::Uuid) -> Option<Value> {
        response(
            json!({"node_id":"node-b","runtime_id":id.to_string(),"connection_state":"connected"}),
        )
    }
    fn request(wait_seconds: u64) -> RestartRequest {
        RestartRequest {
            node_id: "node-b".into(),
            wait_seconds,
        }
    }
    #[tokio::test]
    async fn local_approval_is_required_before_restart() {
        let fake = Fake(Mutex::new(VecDeque::from([(
            "get",
            response(json!({"remote_allowed":false})),
        )])));
        let result = restart_on(&fake, request(30)).await.unwrap();
        assert_eq!(result["error_code"], "approval_required");
        assert_eq!(result["restart_requested"], false);
        assert!(fake.0.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn changed_runtime_confirms_completion_without_replaying() {
        let old = uuid::Uuid::new_v4();
        let new = uuid::Uuid::new_v4();
        let fake = Fake(Mutex::new(VecDeque::from([
            ("get", response(json!({"remote_allowed":true}))),
            ("diagnostics", diagnostics(old)),
            ("restart", response(json!({"restarting":true}))),
            ("diagnostics", diagnostics(new)),
        ])));
        let result = restart_on(&fake, request(1)).await.unwrap();
        assert_eq!(result["completed"], true);
        assert_eq!(result["runtime_id"], new.to_string());
        assert_eq!(result["previous_runtime_id"], old.to_string());
        assert!(fake.0.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn missing_reply_is_uncertain_not_an_automatic_retry() {
        let fake = Fake(Mutex::new(VecDeque::from([
            ("get", response(json!({"remote_allowed":true}))),
            ("diagnostics", diagnostics(uuid::Uuid::new_v4())),
            ("restart", None),
        ])));
        let result = restart_on(&fake, request(0)).await.unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(result["outcome_unknown"], true);
        assert_eq!(result["error_code"], "restart_unconfirmed");
        assert!(fake.0.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn acknowledgement_alone_is_not_completion_and_revocation_is_respected() {
        for accepted in [true, false] {
            let fake = Fake(Mutex::new(VecDeque::from([
                ("get", response(json!({"remote_allowed":true}))),
                ("diagnostics", diagnostics(uuid::Uuid::new_v4())),
                (
                    "restart",
                    if accepted {
                        response(json!({"restarting":true}))
                    } else {
                        Some(json!({"success":false}))
                    },
                ),
            ])));
            let result = restart_on(&fake, request(0)).await.unwrap();
            assert_eq!(result["success"], accepted);
            assert_eq!(result["completed"], false);
            if !accepted {
                assert_eq!(result["error_code"], "restart_rejected");
            }
            assert!(fake.0.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn invalid_wait_and_unknown_fields_are_rejected_before_contact() {
        let fake = Fake(Mutex::new(VecDeque::new()));
        assert!(restart_on(&fake, request(61)).await.is_err());
        assert!(
            serde_json::from_value::<RestartRequest>(json!({"node_id":"node-b","force":true}))
                .is_err()
        );
    }

    #[tokio::test]
    async fn unchanged_runtime_times_out_without_claiming_completion() {
        let old = uuid::Uuid::new_v4();
        let mut replies = VecDeque::from([
            ("get", response(json!({"remote_allowed":true}))),
            ("diagnostics", diagnostics(old)),
            ("restart", response(json!({"restarting":true}))),
        ]);
        for _ in 0..8 {
            replies.push_back(("diagnostics", diagnostics(old)));
        }
        let fake = Fake(Mutex::new(replies));
        let result = restart_on(&fake, request(1)).await.unwrap();
        assert_eq!(result["success"], false);
        assert_eq!(result["acknowledged"], true);
        assert_eq!(result["completed"], false);
        assert_eq!(result["error_code"], "restart_unconfirmed");
    }
}
