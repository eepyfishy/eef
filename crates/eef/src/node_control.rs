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
    async fn management_status(&self, id: &str) -> Result<Value> {
        self.call(id, "get", Duration::from_secs(2)).await
    }
    async fn runtime_status(&self, id: &str) -> Result<Value> {
        self.call(id, "diagnostics", Duration::from_secs(2)).await
    }
    async fn model_command(
        &self,
        _request: &eefn::model_selection::ModelCommandRequest,
    ) -> Result<Value> {
        bail!("typed model commands unavailable")
    }
}
#[async_trait::async_trait]
impl NodeControl for eefn::NodeServer {
    async fn management_status(&self, id: &str) -> Result<Value> {
        let status = self.call(id, "status", Duration::from_secs(2)).await?;
        if status["success"] == false || control_status(&status, id) {
            return Ok(status);
        }
        require_legacy_status(&status, id)?;
        // Older nodes lack the new status contract. Read-only compatibility;
        // never fallback on transport failure or a known capacity rejection.
        self.call(id, "get", Duration::from_secs(2)).await
    }
    async fn runtime_status(&self, id: &str) -> Result<Value> {
        let status = self.call(id, "status", Duration::from_secs(2)).await?;
        if status["success"] == false || control_status(&status, id) {
            return Ok(status);
        }
        require_legacy_status(&status, id)?;
        self.call(id, "diagnostics", Duration::from_secs(2)).await
    }
    async fn model_command(
        &self,
        request: &eefn::model_selection::ModelCommandRequest,
    ) -> Result<Value> {
        self.invoke_remote(
            &request.expected_node_id,
            "node.configure",
            "model_command",
            serde_json::to_value(request)?,
            Duration::from_secs(
                if matches!(
                    request.command,
                    eefn::model_selection::ModelCommand::SelectInstalledGguf { .. }
                ) {
                    75
                } else {
                    5
                },
            ),
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

#[derive(Clone, Debug, serde::Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionCommand {
    Show {},
    Set {
        feature: eefn::permission_commands::Feature,
        enabled: bool,
        expected_runtime_id: String,
        expected_digest: String,
    },
}
#[derive(Clone, Debug, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub schema_version: u32,
    pub node_id: String,
    pub command: PermissionCommand,
}

pub fn confirmed_permissions(value: &Value, request: &PermissionRequest) -> bool {
    const FIELDS: &[&str] = &[
        "schema_version",
        "report_type",
        "node_id",
        "command",
        "success",
        "mutation_requested",
        "outcome_unknown",
        "sending_attempted",
        "result",
        "execution_authorized",
        "restart_requested",
        "automatically_retried",
    ];
    let mutation = matches!(request.command, PermissionCommand::Set { .. });
    if request.schema_version != 1
        || eefn::network::validate_remote_node_id(&request.node_id).is_err()
        || value.as_object().is_none_or(|object| {
            object.len() != FIELDS.len() || FIELDS.iter().any(|field| !object.contains_key(*field))
        })
        || value["schema_version"] != 1
        || value["report_type"] != "remote_permission_command"
        || value["node_id"] != request.node_id
        || value["command"] != json!(request.command)
        || value["success"] != true
        || value["mutation_requested"] != mutation
        || value["outcome_unknown"] != false
        || value["sending_attempted"] != mutation
        || value["execution_authorized"] != false
        || value["restart_requested"] != false
        || value["automatically_retried"] != false
    {
        return false;
    }
    let (runtime, command) = match &request.command {
        PermissionCommand::Show {} => {
            let Some(runtime) = value["result"]["runtime_id"].as_str() else {
                return false;
            };
            (
                runtime.to_owned(),
                eefn::permission_commands::PermissionCommand::Show {},
            )
        }
        PermissionCommand::Set {
            feature,
            enabled,
            expected_runtime_id,
            expected_digest,
        } => (
            expected_runtime_id.clone(),
            eefn::permission_commands::PermissionCommand::Set {
                feature: *feature,
                enabled: *enabled,
                expected_digest: expected_digest.clone(),
            },
        ),
    };
    eefn::permission_commands::confirmed(
        &value["result"],
        &eefn::permission_commands::PermissionRequest {
            schema_version: 1,
            expected_node_id: request.node_id.clone(),
            expected_runtime_id: runtime,
            command,
        },
    )
}

pub async fn permissions(server: &eefn::NodeServer, request: PermissionRequest) -> Result<Value> {
    use eefn::permission_commands as p;
    eefn::network::validate_remote_node_id(&request.node_id)?;
    if request.schema_version != 1 {
        bail!("Unsupported remote permission schema")
    }
    let mutation = matches!(request.command, PermissionCommand::Set { .. });
    let mut result = json!({"schema_version":1,"report_type":"remote_permission_command","node_id":request.node_id,
        "command":request.command,"success":false,"mutation_requested":mutation,"outcome_unknown":false,
        "sending_attempted":false,"result":null,"execution_authorized":false,"restart_requested":false,"automatically_retried":false});
    let (runtime, command) = match &request.command {
        PermissionCommand::Show {} => {
            let status = server
                .invoke_remote(
                    &request.node_id,
                    "node.configure",
                    "status",
                    json!({}),
                    Duration::from_secs(5),
                )
                .await;
            let Ok(status) = status else {
                return Ok(result);
            };
            if !control_status(&status, &request.node_id) {
                return Ok(result);
            }
            (
                status["data"]["runtime_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing node runtime"))?
                    .to_owned(),
                p::PermissionCommand::Show {},
            )
        }
        PermissionCommand::Set {
            feature,
            enabled,
            expected_runtime_id,
            expected_digest,
        } => (
            expected_runtime_id.clone(),
            p::PermissionCommand::Set {
                feature: *feature,
                enabled: *enabled,
                expected_digest: expected_digest.clone(),
            },
        ),
    };
    let node_request = p::PermissionRequest {
        schema_version: 1,
        expected_node_id: request.node_id,
        expected_runtime_id: runtime,
        command,
    };
    node_request.validate()?;
    result["sending_attempted"] = json!(mutation);
    result["outcome_unknown"] = json!(mutation);
    let reply = server
        .invoke_remote(
            &node_request.expected_node_id,
            "node.configure",
            if mutation {
                "permission_command"
            } else {
                "permission_status"
            },
            serde_json::to_value(&node_request)?,
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true && p::confirmed(&reply["data"], &node_request) {
            result["success"] = json!(true);
            result["outcome_unknown"] = json!(false);
            result["result"] = reply["data"].clone();
        }
    }
    Ok(result)
}
pub async fn model_lifecycle(
    server: &eefn::NodeServer,
    request: eefn::model_lifecycle::LifecycleRequest,
) -> Result<Value> {
    request.validate()?;
    let mut result = json!({"schema_version":1,"report_type":"remote_model_lifecycle","success":false,
        "command":request.command,"expected_node_id":request.expected_node_id,"expected_runtime_id":request.expected_runtime_id,
        "mutation_requested":!matches!(request.command,eefn::model_lifecycle::LifecycleCommand::Show{..}),
        "outcome_unknown":!matches!(request.command,eefn::model_lifecycle::LifecycleCommand::Show{..}),"automatically_retried":false,"result":null});
    let reply = server
        .invoke_remote(
            &request.expected_node_id,
            "node.configure",
            match request.command {
                eefn::model_lifecycle::LifecycleCommand::Show { .. } => "model_lifecycle_status",
                eefn::model_lifecycle::LifecycleCommand::Cancel { .. } => "model_lifecycle_cancel",
                _ => "model_lifecycle",
            },
            serde_json::to_value(&request)?,
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true && eefn::model_lifecycle::confirmed(&reply["data"], &request) {
            result["success"] = json!(true);
            result["outcome_unknown"] = json!(false);
            result["result"] = reply["data"].clone();
        }
    }
    Ok(result)
}

pub async fn model_lifecycle_policy(
    server: &eefn::NodeServer,
    request: eefn::model_lifecycle_policy::PolicyRequest,
) -> Result<Value> {
    request.validate()?;
    let mutation = !matches!(
        request.command,
        eefn::model_lifecycle_policy::PolicyCommand::Show {}
    );
    if mutation && request.expected_runtime_id.is_none() {
        bail!("Exact remote runtime required")
    }
    let mut result = json!({"schema_version":1,"report_type":"remote_model_lifecycle_policy","success":false,
        "command":request.command,"expected_node_id":request.expected_node_id,"expected_runtime_id":request.expected_runtime_id,
        "mutation_requested":mutation,"outcome_unknown":mutation,"automatically_retried":false,"restart_requested":false,"result":null});
    let reply = server
        .invoke_remote(
            &request.expected_node_id,
            "node.configure",
            if mutation {
                "model_lifecycle_policy"
            } else {
                "model_lifecycle_policy_status"
            },
            serde_json::to_value(&request)?,
            Duration::from_secs(10),
        )
        .await;
    if let Ok(reply) = reply {
        if reply["success"] == true
            && eefn::model_lifecycle_policy::confirmed(&reply["data"], &request)
        {
            result["success"] = json!(true);
            result["outcome_unknown"] = json!(false);
            result["result"] = reply["data"].clone();
        }
    }
    Ok(result)
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
    if matches!(
        request.command,
        ModelCommand::Interpreter { .. }
            | ModelCommand::ClearInterpreter {}
            | ModelCommand::CpuThreads { .. }
            | ModelCommand::MemoryLimit { .. }
            | ModelCommand::SelectInstalledGguf { .. }
    ) {
        result["command"] = serde_json::to_value(&request.command)?;
    }
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
    if not_started_busy(&preview) {
        result["error_code"] = json!("node_busy");
        result["not_started"] = json!(true);
        result["note"] = json!(
            "Node work capacity is full. Model inspection was rejected and no mutation was sent. No automatic retry."
        );
        return Ok(result);
    }
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
        Ok(reply) if not_started_busy(&reply) => {
            result["error_code"] = json!("node_busy");
            result["not_started"] = json!(true);
            result["note"] = json!(
                "Node rejected this model command before execution because its work capacity is full. No selection change was started and no automatic retry was sent."
            );
        }
        Ok(reply)
            if valid(&reply)
                && (!matches!(
                    command.command,
                    ModelCommand::Interpreter { .. }
                        | ModelCommand::ClearInterpreter {}
                        | ModelCommand::CpuThreads { .. }
                        | ModelCommand::MemoryLimit { .. }
                        | ModelCommand::SelectInstalledGguf { .. }
                ) || eefn::model_selection::confirmed_interpreter(
                    &reply["data"],
                    &request.node_id,
                    &command.command,
                )) =>
        {
            result["success"] = json!(true);
            result["acknowledged"] = json!(true);
            result["data"] = reply["data"].clone();
        }
        Ok(reply)
            if reply["success"] == true
                && eefn::model_selection::confirmed_installed_rejection(
                    &reply["data"],
                    &request.node_id,
                    &command.command,
                ) =>
        {
            result["error_code"] = reply["data"]["error_code"].clone();
            result["configuration_write_attempted"] = json!(false);
            result["data"] = reply["data"].clone();
            result["note"] = json!(
                "Node rejected installed-cache preparation before any configuration write. No automatic retry or fallback was sent."
            );
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
    let permitted = server.management_status(id).await?;
    if not_started_busy(&permitted) {
        result["error_code"] = json!("node_busy");
        result["not_started"] = json!(true);
        result["note"] = json!(
            "Node control capacity is full. No restart was sent and no automatic retry was attempted."
        );
        return Ok(result);
    }
    if permitted["success"] != true {
        bail!("node management status unavailable")
    }
    if permitted.pointer("/data/remote_allowed") != Some(&json!(true)) {
        result["error_code"] = json!("approval_required");
        result["note"] =
            json!("Enable remote management in the local node app before requesting a restart.");
        return Ok(result);
    }
    let before = if control_status(&permitted, id) {
        parse_runtime_id(&permitted, id)?
    } else {
        runtime_id(server, id).await?
    };
    result["previous_runtime_id"] = json!(before);
    result["restart_requested"] = json!(true);
    let reply = server.call(id, "restart", Duration::from_secs(3)).await;
    match reply {
        Ok(value) if not_started_busy(&value) => {
            result["error_code"] = json!("node_busy");
            result["not_started"] = json!(true);
            result["note"] = json!(
                "Node rejected restart before execution because its control capacity is full. No automatic retry was sent."
            );
            return Ok(result);
        }
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
    let response = server.runtime_status(id).await?;
    parse_runtime_id(&response, id)
}

fn parse_runtime_id(response: &Value, id: &str) -> Result<String> {
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

pub(crate) fn control_status(reply: &Value, id: &str) -> bool {
    reply["success"] == true
        && reply["data"]["control_schema_version"] == 1
        && reply["data"]["node_id"] == id
        && reply["data"]["remote_allowed"].is_boolean()
}

fn require_legacy_status(reply: &Value, id: &str) -> Result<()> {
    if reply["success"] != true
        || reply["data"]["node_id"] != id
        || reply["data"].get("control_schema_version").is_some()
    {
        bail!("node control status is malformed, mismatched or unsupported; no legacy fallback");
    }
    Ok(())
}

fn not_started_busy(reply: &Value) -> bool {
    reply["success"] == false && reply["error_type"] == "NodeBusy" && reply["not_started"] == true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_permission_wrapper_checks_exact_targets_and_rejects_outer_or_inner_authority_claims()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        std::fs::write(&path, b"{\"node_id\":\"owner\",\"permissions\":{}}").unwrap();
        let core = eefn::NodeService::new(path, "owner".into());
        let engine =
            std::sync::Arc::new(eefn::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let runtime = uuid::Uuid::new_v4().to_string();
        core.live.lock().unwrap()["runtime_id"] = json!(runtime);
        let _guard = core.attach_preview_runtime(&engine, runtime.clone());
        let request = PermissionRequest {
            schema_version: 1,
            node_id: "owner".into(),
            command: PermissionCommand::Show {},
        };
        let node_request = eefn::permission_commands::PermissionRequest {
            schema_version: 1,
            expected_node_id: "owner".into(),
            expected_runtime_id: runtime,
            command: eefn::permission_commands::PermissionCommand::Show {},
        };
        let actual = core.permission_command(node_request, false).unwrap();
        let value = json!({"schema_version":1,"report_type":"remote_permission_command","node_id":"owner","command":request.command,
            "success":true,"mutation_requested":false,"outcome_unknown":false,"sending_attempted":false,"result":actual,
            "execution_authorized":false,"restart_requested":false,"automatically_retried":false});
        assert!(confirmed_permissions(&value, &request));
        for (field, invalid) in [
            ("node_id", json!("other")),
            ("execution_authorized", json!(true)),
            ("sending_attempted", json!(true)),
            ("outcome_unknown", json!(true)),
            ("unknown", json!("PRIVATE")),
        ] {
            let mut bad = value.clone();
            bad[field] = invalid;
            assert!(!confirmed_permissions(&bad, &request));
        }
        let mut bad = value;
        bad["result"]["restart_requested"] = json!(true);
        assert!(!confirmed_permissions(&bad, &request));
    }
    use std::{collections::VecDeque, sync::Mutex};
    struct Fake(Mutex<VecDeque<(&'static str, Option<Value>)>>);
    struct ModelFake(Mutex<VecDeque<(&'static str, Option<Value>)>>);
    #[test]
    fn control_status_contract_rejects_forged_or_versioned_reports_without_legacy_fallback() {
        let legacy = json!({"success":true,"data":{"node_id":"node-b"}});
        assert!(require_legacy_status(&legacy, "node-b").is_ok());
        for value in [json!(0), json!(2), json!(null)] {
            let mut changed = legacy.clone();
            changed["data"]["control_schema_version"] = value;
            assert!(require_legacy_status(&changed, "node-b").is_err());
            assert!(!control_status(&changed, "node-b"));
        }
        let status = json!({"success":true,"data":{"node_id":"node-b","control_schema_version":1,"remote_allowed":true,
            "runtime_id":uuid::Uuid::new_v4().to_string(),"connection_state":"connected"}});
        assert!(control_status(&status, "node-b"));
        assert!(parse_runtime_id(&status, "node-b").is_ok());
        assert!(!control_status(&status, "other"));
        assert!(require_legacy_status(&status, "other").is_err());
        let mut changed = status;
        changed["data"]["runtime_id"] = json!("bad");
        assert!(parse_runtime_id(&changed, "node-b").is_err());
    }
    #[tokio::test]
    async fn restart_capacity_rejection_never_replays_or_reports_unknown_execution() {
        let busy = Some(json!({"success":false,"error_type":"NodeBusy","not_started":true}));
        for after_preflight in [false, true] {
            let mut calls = VecDeque::new();
            if after_preflight {
                calls.push_back(("get", response(json!({"remote_allowed":true}))));
                calls.push_back(("diagnostics", diagnostics(uuid::Uuid::new_v4())));
            }
            calls.push_back((
                if after_preflight { "restart" } else { "get" },
                busy.clone(),
            ));
            let fake = Fake(Mutex::new(calls));
            let result = restart_on(&fake, request(0)).await.unwrap();
            assert_eq!(result["error_code"], "node_busy");
            assert_eq!(result["not_started"], true);
            assert_eq!(result["restart_requested"], after_preflight);
            assert_eq!(result["outcome_unknown"], false);
            assert!(fake.0.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn model_capacity_rejections_are_known_not_started_without_mutation_replay() {
        let busy = json!({"success":false,"error_type":"NodeBusy","not_started":true});
        for after_preview in [false, true] {
            let mut replies = VecDeque::new();
            if after_preview {
                replies.push_back(("show", Some(model_report(true))));
            }
            replies.push_back((
                if after_preview { "provider" } else { "show" },
                Some(busy.clone()),
            ));
            let fake = ModelFake(Mutex::new(replies));
            let result = models_on(&fake, model_request(false)).await.unwrap();
            assert_eq!(result["success"], false);
            assert_eq!(result["error_code"], "node_busy");
            assert_eq!(result["mutation_requested"], after_preview);
            assert_eq!(result["outcome_unknown"], false);
            assert_eq!(result["not_started"], true);
            assert!(fake.0.lock().unwrap().is_empty());
        }
        // Missing not-started confirmation must remain uncertain after send.
        let fake = ModelFake(Mutex::new(VecDeque::from([
            ("show", Some(model_report(true))),
            (
                "provider",
                Some(json!({"success":false,"error_type":"NodeBusy"})),
            ),
        ])));
        let result = models_on(&fake, model_request(false)).await.unwrap();
        assert_eq!(result["outcome_unknown"], true);
        assert_eq!(result["error_code"], "model_change_unconfirmed");
        assert!(fake.0.lock().unwrap().is_empty());
    }
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
    async fn interpreter_activation_requires_exact_saved_provider_without_replay() {
        use eefn::model_selection::{Backend, ModelCommand};
        let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
        metadata.roles = Some(vec!["request_interpreter".into()]);
        for provider in ["ollama", "auto", "llamacpp"] {
            let reply = json!({"success":true,"data":{"schema_version":1,"success":true,"node_id":"node-b",
                "report_type":"model_selections","remote_management_allowed":true,"changed":true,
                "saved_selections":[{"backend":"ollama","model_id":"chosen","model_metadata":metadata}],
                "registered_models":null,"provider":provider,"restart_required":true,"note":"Saved only"}});
            let fake = ModelFake(Mutex::new(VecDeque::from([
                ("show", Some(model_report(true))),
                ("interpreter", Some(reply)),
            ])));
            let result = models_on(
                &fake,
                ModelsRequest {
                    schema_version: 1,
                    node_id: "node-b".into(),
                    command: ModelCommand::Interpreter {
                        backend: Backend::Ollama,
                        model_id: "chosen".into(),
                        activate_backend: true,
                    },
                },
            )
            .await
            .unwrap();
            assert_eq!(result["success"], provider == "ollama");
            assert_eq!(result["outcome_unknown"], provider != "ollama");
            assert!(fake.0.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn remote_interpreter_changes_confirm_exact_saved_assignment_without_replay() {
        use eefn::model_selection::{Backend, ModelCommand};
        for clear in [false, true] {
            let mut metadata = eefn::model_metadata::ModelMetadata::from_legacy(Some("text"));
            metadata.roles = Some(if clear {
                vec!["other_role".into()]
            } else {
                vec!["other_role".into(), "request_interpreter".into()]
            });
            let valid = json!({"success":true,"data":{"schema_version":1,"success":true,"node_id":"node-b",
                "report_type":"model_selections","remote_management_allowed":true,"changed":true,
                "saved_selections":[{"backend":"ollama","model_id":"chosen","model_metadata":metadata}],
                "registered_models":null,"provider":"auto","restart_required":true,"note":"Saved only"}});
            for case in 0..4 {
                let mut reply = valid.clone();
                match case {
                    1 => {
                        reply["data"]["saved_selections"][0]["model_metadata"]["roles"] =
                            json!(if clear {
                                vec!["request_interpreter"]
                            } else {
                                vec!["other_role"]
                            })
                    }
                    2 => {
                        let duplicate = reply["data"]["saved_selections"][0].clone();
                        reply["data"]["saved_selections"]
                            .as_array_mut()
                            .unwrap()
                            .push(duplicate);
                    }
                    3 => reply["data"]["execution_authorized"] = json!(true),
                    _ => {}
                }
                let fake = ModelFake(Mutex::new(VecDeque::from([
                    ("show", Some(model_report(true))),
                    (
                        if clear {
                            "clear_interpreter"
                        } else {
                            "interpreter"
                        },
                        Some(reply),
                    ),
                ])));
                let result = models_on(
                    &fake,
                    ModelsRequest {
                        schema_version: 1,
                        node_id: "node-b".into(),
                        command: if clear {
                            ModelCommand::ClearInterpreter {}
                        } else {
                            ModelCommand::Interpreter {
                                backend: Backend::Ollama,
                                model_id: "chosen".into(),
                                activate_backend: false,
                            }
                        },
                    },
                )
                .await
                .unwrap();
                assert_eq!(result["success"], case == 0);
                assert_eq!(result["acknowledged"], case == 0);
                assert_eq!(result["outcome_unknown"], case != 0);
                assert_eq!(result["mutation_requested"], true);
                assert_eq!(
                    result["command"]["operation"],
                    if clear {
                        "clear_interpreter"
                    } else {
                        "interpreter"
                    }
                );
                if !clear {
                    assert_eq!(result["command"]["model_id"], "chosen");
                    assert_eq!(result["command"]["backend"], "ollama");
                }
                if case != 0 {
                    assert_eq!(result["error_code"], "model_change_unconfirmed");
                }
                assert!(fake.0.lock().unwrap().is_empty());
            }
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
