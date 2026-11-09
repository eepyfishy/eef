//! Exact granted interpreter execution, not a delegation broker or policy issuer.
use crate::{NodeEngine, grants::ExecutionGrant, interpreter_task::Task};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    model_id: String,
    backend: String,
    permission_revision: u64,
    interpreter_task: Task,
}

pub(crate) fn zero(value: &u64) -> bool {
    *value == 0
}

/// Backend-owned generation: ordinary saves/restore cannot undo revocation by
/// restoring the old boolean or a caller-supplied revision. No-op saves preserve it.
pub(crate) fn preserve_permission(old: &Value, next: &mut Value) -> Result<()> {
    let before: crate::NodePolicy =
        serde_json::from_value(old.get("permissions").cloned().unwrap_or_else(|| json!({})))?;
    let after: crate::NodePolicy = serde_json::from_value(
        next.get("permissions")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )?;
    let revision = if before.interpreter_sharing == after.interpreter_sharing {
        before.interpreter_sharing_revision
    } else {
        match before.interpreter_sharing_revision.checked_add(1) {
            Some(value) => value,
            None if !after.interpreter_sharing => u64::MAX,
            None => anyhow::bail!("Interpreter sharing revision exhausted; no reset or re-enable"),
        }
    };
    if revision != 0
        || old
            .pointer("/permissions/interpreter_sharing_revision")
            .is_some()
    {
        let permissions = next
            .as_object_mut()
            .context("Node settings must be an object")?
            .entry("permissions")
            .or_insert(json!({}))
            .as_object_mut()
            .context("Permissions must be an object")?;
        permissions.insert("interpreter_sharing_revision".into(), json!(revision));
    } else if let Some(permissions) = next.get_mut("permissions").and_then(Value::as_object_mut) {
        permissions.remove("interpreter_sharing_revision");
    }
    Ok(())
}

/// Content binding only. Authenticated lease admission and serving permission
/// remain mandatory; this function cannot issue, consume or run a grant.
pub(crate) fn binding(grant: &ExecutionGrant) -> Result<Task> {
    let params: Params = serde_json::from_value(grant.params.clone())
        .context("Invalid interpreter task parameters")?;
    params.interpreter_task.validate()?;
    // Its value is pinned by the complete grant digest; the executor compares
    // it with both applied and current serving permission before any inference.
    let _permission_revision = params.permission_revision;
    ensure!(
        grant.capability == "llm.infer"
            && grant.action == "run"
            && grant.resource_id.is_none()
            && grant.model_role.as_deref() == Some("request_interpreter")
            && grant.model_id.as_deref() == Some(params.model_id.as_str())
            && grant.backend.as_deref() == Some(params.backend.as_str())
            && matches!(params.backend.as_str(), "ollama" | "llamacpp")
            && grant.job_id == params.interpreter_task.snapshot.job_id
            && grant.request_context == params.interpreter_task.snapshot.request_context
            && grant.plan_sha256 == definition_digest(&params.interpreter_task)?,
        "Interpreter task differs from its exact grant binding"
    );
    Ok(params.interpreter_task)
}

/// Uses the existing grant definition field for this model-service task, never
/// a fabricated compiled tool plan or a statement of per-request human approval.
pub(crate) fn definition_digest(task: &Task) -> Result<String> {
    crate::grants::definition_digest(&json!({"domain":"eef.interpreter.task.v1","task":task}))
}

/// Only the accepted-grant driver invokes this operation under its existing
/// ledger, deadline, stop/policy watcher and durable completion checkpoint.
pub(crate) async fn execute(
    engine: &NodeEngine,
    grant: &ExecutionGrant,
    deadline: Instant,
) -> Result<Value> {
    engine.check_grant_policy(grant)?;
    let task = binding(grant)?;
    let spec = task.validate()?;
    let backend = grant
        .backend
        .as_deref()
        .context("Missing interpreter backend")?;
    let model = grant
        .model_id
        .as_deref()
        .context("Missing interpreter model")?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "Interpreter grant deadline elapsed");
    // Backend timeout rounding never extends the accepted driver's deadline.
    let seconds = remaining
        .as_secs()
        .saturating_add(1)
        .min(crate::grants::MAX_DURATION_MS / 1000);
    let response = engine
        .infer_with_schema(
            "llm.infer",
            json!({"backend":backend,"model":model,"messages":spec.messages,
            "temperature":0,"max_tokens":spec.max_output_tokens,"timeout":seconds}),
            spec.output_schema,
            Some(engine.applied_interpreter_sampling()),
        )
        .await?;
    // A revocation during the final provider poll cannot become an accepted
    // candidate. Shared-provider computation may still have occurred.
    engine.check_grant_policy(grant)?;
    ensure!(
        Instant::now() < deadline,
        "Interpreter completion exceeded its grant"
    );
    ensure!(
        response["completion_complete"] == true,
        "Interpreter completion is incomplete"
    );
    let output = response["output"]
        .as_str()
        .context("Missing interpreter output")?;
    let candidate = task.candidate(output.as_bytes(), backend, model)?;
    Ok(
        json!({"schema_version":1,"report_type":"node_interpreter_candidate",
        "task_sha256":grant.plan_sha256,"snapshot_sha256":task.snapshot_sha256,
        "job_id":task.snapshot.job_id,"source_revision":task.snapshot.revision,
        "model":{"backend":backend,"model_id":model},"candidate":candidate,
        "permission_revision":grant.params["permission_revision"],
        "model_output_is_untrusted":true,"execution_authorized":false,
        "delegation_requested":false,"automatically_retried":false}),
    )
}

#[cfg(test)]
#[path = "interpreter_serving_tests.rs"]
mod tests;
