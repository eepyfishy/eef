//! Optional command-based interpretation; no local-model retry or background job.
use crate::{
    NodeService,
    inference_budget::InferenceBudget,
    interpreter_command::{Command, Preferences, Request, Stage},
};
use serde_json::Value;

pub(crate) fn request(
    node: &str,
    runtime: &str,
    id: &str,
    stage: Stage,
    revision: Option<u64>,
    operation: &str,
    budget: InferenceBudget,
) -> Request {
    Request {
        schema_version: 1,
        expected_node_id: node.into(),
        expected_runtime_id: runtime.into(),
        command: Command {
            id: id.into(),
            stage,
            expected_revision: revision,
            operation_id: operation.into(),
            preferences: Preferences::default(),
            maximum_execution_ms: Some(budget.grant_ceiling_ms()),
        },
    }
}

/// The surrounding report pins the source; the broker receipt pins its provider,
/// issued limits and observed application. It is inspection data, not authority.
pub(crate) fn confirmed_receipt(
    value: &Value,
    node: &str,
    runtime: &str,
    id: &str,
    stage: Stage,
    revision: Option<u64>,
    operation: &str,
) -> bool {
    let Ok(command) = serde_json::from_value::<Command>(value["command"].clone()) else {
        return false;
    };
    command.id == id
        && command.stage == stage
        && command.expected_revision == revision
        && command.operation_id == operation
        && command.preferences == Preferences::default()
        && command.maximum_execution_ms.is_some()
        && crate::interpreter_command::confirmed_node(
            value,
            &Request {
                schema_version: 1,
                expected_node_id: node.into(),
                expected_runtime_id: runtime.into(),
                command,
            },
        )
}

impl NodeService {
    pub(crate) async fn prepare_network_method(
        &self,
        operation: &crate::request_preparation::PrepareRequest,
        runtime: &str,
        budget: InferenceBudget,
        admission: Option<&tokio::sync::OwnedSemaphorePermit>,
        report: &mut Value,
    ) -> Result<(), &'static str> {
        let command = &operation.command;
        let request = request(
            &self.node_id,
            runtime,
            &command.id,
            Stage::Method,
            Some(command.expected_revision),
            &command.operation_id,
            budget,
        );
        report["saving_attempted"] = true.into();
        report["outcome_unknown"] = true.into();
        let receipt = self.network_interpretation(request, admission).await?;
        report["network_interpreter"] = receipt.clone();
        report["outcome_unknown"] = receipt["outcome_unknown"].clone();
        if receipt["success"] != true {
            return Err("network_interpreter_unconfirmed");
        }
        let historical = receipt["result"]["historical_snapshot"] == true;
        report["inspection_only"] = historical.into();
        report["saving_attempted"] = (!historical).into();
        report["model_inference_requested"] =
            receipt["result"]["model_inference_requested"].clone();
        report["coordinator_operation_id"] = command.operation_id.clone().into();
        report["result"] = receipt["result"]["view"].clone();
        report["success"] = true.into();
        report["coordinator_acknowledged"] = true.into();
        report["checkpoint_saved"] = true.into();
        if !crate::request_preparation::confirmed(report, &self.node_id, command) {
            report["success"] = false.into();
            report["coordinator_acknowledged"] = false.into();
            report["checkpoint_saved"] = false.into();
            report["outcome_unknown"] = true.into();
            return Err("network_preparation_unconfirmed");
        }
        Ok(())
    }

    pub(crate) async fn resolve_network_clarification(
        &self,
        operation: &crate::request_resolution::ResolveRequest,
        runtime: &str,
        budget: InferenceBudget,
        admission: Option<&tokio::sync::OwnedSemaphorePermit>,
        report: &mut Value,
    ) -> Result<(), &'static str> {
        let command = &operation.command;
        let request = request(
            &self.node_id,
            runtime,
            &command.id,
            Stage::Clarification,
            Some(command.expected_revision),
            &command.operation_id,
            budget,
        );
        report["interpretation_source"] = "coordinator_interpreter_service".into();
        report["saving_attempted"] = true.into();
        report["outcome_unknown"] = true.into();
        let receipt = self.network_interpretation(request, admission).await?;
        report["network_interpreter"] = receipt.clone();
        report["outcome_unknown"] = receipt["outcome_unknown"].clone();
        if receipt["success"] != true {
            return Err("network_interpreter_unconfirmed");
        }
        let inspection = uuid::Uuid::new_v4().to_string();
        report["outcome_unknown"] = true.into();
        let read = self
            .draft_command(crate::request_drafts::DraftRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                request_id: inspection.clone(),
                command: crate::request_drafts::DraftCommand::Get {
                    id: command.id.clone(),
                },
            })
            .await
            .map_err(|_| "network_resolution_inspection_unconfirmed")?;
        if read["success"] != true
            || !crate::request_drafts::confirmed_report(
                &read["result"],
                &self.node_id,
                &inspection,
                Some(&command.id),
            )
        {
            report["outcome_unknown"] = true.into();
            return Err("network_resolution_inspection_unconfirmed");
        }
        // This is a real draft read, not a relabelled PlanningView. A concurrent
        // change is reported instead of presenting the earlier view as current.
        for key in ["id", "revision", "status", "draft", "request_context"] {
            if read["result"][key] != receipt["result"]["view"][key] {
                report["outcome_unknown"] = true.into();
                return Err("network_resolution_source_changed");
            }
        }
        let historical = receipt["result"]["historical_snapshot"] == true;
        report["reused"] = historical.into();
        report["saving_attempted"] = (!historical).into();
        report["model_inference_requested"] =
            receipt["result"]["model_inference_requested"].clone();
        report["coordinator_operation_id"] = inspection.into();
        report["result"] = read["result"].clone();
        report["success"] = true.into();
        report["coordinator_acknowledged"] = true.into();
        report["proposal_saved"] = true.into();
        report["outcome_unknown"] = false.into();
        if !crate::request_resolution::confirmed(report, &self.node_id, command) {
            report["success"] = false.into();
            report["coordinator_acknowledged"] = false.into();
            report["proposal_saved"] = false.into();
            report["outcome_unknown"] = true.into();
            return Err("network_resolution_unconfirmed");
        }
        Ok(())
    }

    pub(crate) async fn network_interpretation(
        &self,
        request: Request,
        admission: Option<&tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<Value, &'static str> {
        let Some((runtime, policy, _)) = self.input_workflow_runtime() else {
            return Err("runtime_unavailable");
        };
        if runtime != request.expected_runtime_id {
            return Err("runtime_changed");
        }
        if !policy.network_interpreter {
            return Err("network_interpreter_not_enabled");
        }
        let reply = match admission {
            Some(permit) => {
                self.interpreter_command_admitted(request.clone(), permit)
                    .await
            }
            None => self.interpreter_command(request.clone()).await,
        }
        .map_err(|_| "network_interpreter_unconfirmed")?;
        if !crate::interpreter_command::confirmed_node(&reply, &request) {
            return Err("network_interpreter_unconfirmed");
        }
        Ok(reply)
    }
}
