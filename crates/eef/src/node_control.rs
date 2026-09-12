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
    async fn model_command(
        &self,
        _request: &eefn::model_selection::ModelCommandRequest,
    ) -> Result<Value> {
        bail!("typed model commands unavailable")
    }
}
#[async_trait::async_trait]
impl NodeControl for eefn::NodeServer {
    async fn model_command(
        &self,
        request: &eefn::model_selection::ModelCommandRequest,
    ) -> Result<Value> {
        self.invoke_remote(
            &request.expected_node_id,
            "node.configure",
            "model_command",
            serde_json::to_value(request)?,
            Duration::from_secs(5),
        )
        .await
    }
    async fn call(&self, id: &str, action: &str, wait: Duration) -> Result<Value> {
        self.invoke_remote(id, "node.configure", action, json!({}), wait)
            .await
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsRequest {
    pub schema_version: u32,
    pub node_id: String,
    pub command: eefn::model_selection::ModelCommand,
}

pub async fn models(server: &eefn::NodeServer, request: ModelsRequest) -> Result<Value> {
    models_on(server, request).await
}

async fn models_on(server: &impl NodeControl, request: ModelsRequest) -> Result<Value> {
    use eefn::model_selection::{ModelCommand, ModelCommandRequest};
    eefn::network::validate_node_id(&request.node_id)?;
    if request.schema_version != 1 {
        bail!("unsupported command schema")
    }
    let mut result = json!({"schema_version":1,"success":false,"report_type":"remote_model_command",
        "operation_id":uuid::Uuid::new_v4().to_string(),"node_id":request.node_id,
        "mutation_requested":false,"acknowledged":false,"outcome_unknown":false});
    let mut command = ModelCommandRequest {
        schema_version: 1,
        expected_node_id: request.node_id.clone(),
        command: ModelCommand::Show {},
    };
    let valid = |reply: &Value| {
        reply["success"] == true
            && reply["data"]["schema_version"] == 1
            && reply["data"]["node_id"] == request.node_id
            && reply["data"]["success"] == true
            && reply["data"]["report_type"] == "model_selections"
    };
    let preview = server.model_command(&command).await;
    let Ok(preview) = preview else {
        result["error_code"] = json!("model_commands_unavailable");
        result["note"] = json!(
            "Could not inspect node model commands. No mutation sent; check connection and node version."
        );
        return Ok(result);
    };
    if !valid(&preview) {
        result["error_code"] = json!("model_commands_unavailable");
        result["note"] = json!(
            "Node did not provide the expected typed model report. Upgrade the node; no legacy save fallback was used."
        );
        return Ok(result);
    }
    if matches!(request.command, ModelCommand::Show {}) {
        result["success"] = json!(true);
        result["acknowledged"] = json!(true);
        result["data"] = preview["data"].clone();
        return Ok(result);
    }
    if preview["data"]["remote_management_allowed"] != true {
        result["error_code"] = json!("approval_required");
        result["note"] =
            json!("Enable management from EEF locally on this node. No mutation sent.");
        return Ok(result);
    }
    command.command = request.command;
    result["mutation_requested"] = json!(true);
    // One send only. A lost response must never cause an automatic replay.
    match server.model_command(&command).await {
        Ok(reply) if valid(&reply) => {
            result["success"] = json!(true);
            result["acknowledged"] = json!(true);
            result["data"] = reply["data"].clone();
        }
        Ok(reply)
            if reply["success"] == true
                && reply["data"]["schema_version"] == 1
                && reply["data"]["node_id"] == request.node_id
                && reply["data"]["success"] == false
                && reply["data"]["error_code"] == "approval_required" =>
        {
            result["error_code"] = json!("approval_required");
            result["note"] = json!(
                "Node approval was revoked or unavailable at application time. No selection change applied."
            );
        }
        _ => {
            result["outcome_unknown"] = json!(true);
            result["error_code"] = json!("model_change_unconfirmed");
            result["note"] = json!(
                "Model change not confirmed. Inspect saved selections before retrying; no automatic retry was sent."
            );
        }
    }
    Ok(result)
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
    struct ModelFake(Mutex<VecDeque<(&'static str, Option<Value>)>>);
    #[async_trait::async_trait]
    impl NodeControl for ModelFake {
        async fn call(&self, _: &str, _: &str, _: Duration) -> Result<Value> {
            panic!("typed model commands must not use a legacy save fallback")
        }
        async fn model_command(
            &self,
            request: &eefn::model_selection::ModelCommandRequest,
        ) -> Result<Value> {
            assert_eq!(request.expected_node_id, "node-b");
            assert_eq!(request.schema_version, 1);
            let (operation, reply) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected replay");
            assert_eq!(
                serde_json::to_value(&request.command)?["operation"],
                operation
            );
            reply.ok_or_else(|| anyhow::anyhow!("lost response"))
        }
    }
    fn model_report(allowed: bool) -> Value {
        json!({"success":true,"data":{"schema_version":1,"success":true,"node_id":"node-b",
            "report_type":"model_selections","remote_management_allowed":allowed,"changed":false}})
    }
    fn model_request(show: bool) -> ModelsRequest {
        ModelsRequest {
            schema_version: 1,
            node_id: "node-b".into(),
            command: if show {
                eefn::model_selection::ModelCommand::Show {}
            } else {
                eefn::model_selection::ModelCommand::Provider {
                    provider: "auto".into(),
                }
            },
        }
    }
    #[tokio::test]
    async fn remote_models_inspect_without_approval_but_do_not_mutate() {
        for show in [true, false] {
            let fake = ModelFake(Mutex::new(VecDeque::from([(
                "show",
                Some(model_report(false)),
            )])));
            let result = models_on(&fake, model_request(show)).await.unwrap();
            assert_eq!(result["success"], show);
            assert_eq!(result["mutation_requested"], false);
            if !show {
                assert_eq!(result["error_code"], "approval_required");
            }
        }
    }
    #[tokio::test]
    async fn remote_model_mutations_send_once_and_distinguish_revocation_from_uncertainty() {
        for reply in [
            None,
            Some(model_report(true)),
            Some(json!({"success":true,"data":{
            "schema_version":1,"node_id":"node-b","success":false,"error_code":"approval_required"}})),
        ] {
            let unknown = reply.is_none();
            let approved = reply.as_ref().is_some_and(|r| r["data"]["success"] == true);
            let fake = ModelFake(Mutex::new(VecDeque::from([
                ("show", Some(model_report(true))),
                ("provider", reply),
            ])));
            let result = models_on(&fake, model_request(false)).await.unwrap();
            assert_eq!(result["success"], approved);
            assert_eq!(result["outcome_unknown"], unknown);
            assert_eq!(result["mutation_requested"], true);
            assert!(fake.0.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn old_or_wrong_node_model_reports_never_trigger_mutation() {
        let mut wrong = model_report(true);
        wrong["data"]["node_id"] = json!("other");
        for reply in [None, Some(json!({"success":false})), Some(wrong)] {
            let fake = ModelFake(Mutex::new(VecDeque::from([("show", reply)])));
            let result = models_on(&fake, model_request(false)).await.unwrap();
            assert_eq!(result["error_code"], "model_commands_unavailable");
            assert_eq!(result["mutation_requested"], false);
            assert_eq!(result["outcome_unknown"], false);
        }
    }
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
