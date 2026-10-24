//! Fixed model-test source authoring. This is not approval or inference authority.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const PREFIX: &str = "EEF model verification v1\n";
const NOTE: &str = "Prepared model-test input only. Separate coordinator-owner approval and normal execution grants are required. No inference, model load/download, restart or permission change was requested. Inspect the exact source receipt after uncertainty; no automatic replay.";
pub const MAX_TOKENS: u64 = 32;
pub const MAX_RESULT_BYTES: usize = 4096;
pub const MAX_EXECUTION_MS: u64 = 30_000;

pub fn receipt_id(node: &str, operation: &str, stage: &str, job: Option<&str>) -> Result<String> {
    crate::network::validate_remote_node_id(node)?;
    crate::automation_occurrence::uuid(operation)?;
    if !matches!(stage, "source" | "approval" | "execution") || (stage == "source") != job.is_none()
    {
        bail!("Unsupported model verification receipt domain")
    }
    if let Some(job) = job {
        crate::request_drafts::validate_id(job)?;
    }
    let hash = Sha256::digest(serde_json::to_vec(&json!([
        "eef.model-verification.v1",
        node,
        operation,
        stage,
        job
    ]))?);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub node_id: String,
    pub node_runtime_id: String,
    pub operation_id: String,
    pub model: crate::setup_check::ModelRef,
}
impl Definition {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Unsupported model verification schema")
        }
        crate::network::validate_remote_node_id(&self.node_id)?;
        crate::automation_occurrence::uuid(&self.node_runtime_id)?;
        crate::automation_occurrence::uuid(&self.operation_id)?;
        crate::model_selection::validate_id(&self.model.model_id)
    }
    /// Distinct source/approval/execution UUIDs, not authentication or grants.
    pub fn receipt(&self, stage: &str, job: Option<&str>) -> Result<String> {
        self.validate()?;
        receipt_id(&self.node_id, &self.operation_id, stage, job)
    }
    pub fn source(&self) -> Result<crate::request_submission::ConcreteCommand> {
        self.validate()?;
        let backend = match self.model.backend {
            crate::model_selection::Backend::Ollama => "ollama",
            crate::model_selection::Backend::Llamacpp => "llamacpp",
        };
        let command = crate::request_submission::ConcreteCommand {
            operation_id: self.receipt("source", None)?,
            request: crate::request_submission::ConcreteInput {
                schema_version: 1,
                original_text: format!("{PREFIX}{}", serde_json::to_string(self)?),
                title: "Check the selected interpreter's inference".into(),
                input: crate::request_planning::PlanInput {
                    schema_version: 1,
                    template: "generate_text".into(),
                    params: json!({"model_id":self.model.model_id,"prompt":"Reply with a short greeting.","max_tokens":MAX_TOKENS,"temperature":0}),
                    constraints: json!({"node_id":self.node_id,"backend":backend,"model_role":"request_interpreter"}),
                },
            },
        };
        command.validate()?;
        Ok(command)
    }
    pub fn from_source(source: &crate::request_submission::ConcreteInput) -> Result<Self> {
        let text = source
            .original_text
            .strip_prefix(PREFIX)
            .ok_or_else(|| anyhow::anyhow!("Not a fixed model verification source"))?;
        let value: Self = serde_json::from_str(text)?;
        if value.source()?.request != *source {
            bail!("Model verification source changed or widened")
        }
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareReport {
    schema_version: u32,
    report_type: String,
    definition: Definition,
    source_operation_id: String,
    success: bool,
    preparation_attempted: bool,
    outcome_unknown: bool,
    source: Option<Value>,
    execution_authorized: bool,
    model_inference_requested: bool,
    restart_requested: bool,
    automatically_retried: bool,
    note: String,
}
pub fn preparation_confirmed(value: &Value, definition: &Definition) -> bool {
    let Ok(report) = serde_json::from_value::<PrepareReport>(value.clone()) else {
        return false;
    };
    let Ok(command) = definition.source() else {
        return false;
    };
    report.schema_version == 1
        && report.report_type == "node_model_verification_preparation"
        && report.definition == *definition
        && report.source_operation_id == command.operation_id
        && report.success
        && report.preparation_attempted
        && !report.outcome_unknown
        && !report.execution_authorized
        && !report.model_inference_requested
        && !report.restart_requested
        && !report.automatically_retried
        && report.note == NOTE
        && report.source.as_ref().is_some_and(|source| {
            crate::request_submission::confirmed(source, &definition.node_id, &command)
        })
}
impl crate::NodeService {
    pub async fn prepare_model_verification(
        self: &std::sync::Arc<Self>,
        definition: Definition,
        remote: bool,
    ) -> Result<Value> {
        definition.validate()?;
        if definition.node_id != self.node_id {
            bail!("Model test targets another node")
        }
        let _owned = self
            .model_verification
            .clone()
            .try_lock_owned()
            .map_err(|_| {
                anyhow::anyhow!("Model verification preparation busy; no queue or retry")
            })?;
        self.with_permission_runtime(&definition.node_runtime_id,|engine| {
            self.inspect_grant_configuration(|config| {
                if config["node_id"]!=self.node_id
                    || self.live.lock().unwrap()["runtime_id"]!=definition.node_runtime_id {
                    bail!("Model test node identity/runtime changed")
                }
                if remote && config.pointer("/management/allow_remote")!=Some(&json!(true)) {
                    bail!("Current local management consent required to prepare remote model-test input")
                }
                let backend=match definition.model.backend {
                    crate::model_selection::Backend::Ollama=>"ollama",
                    crate::model_selection::Backend::Llamacpp=>"llamacpp",
                };
                if engine.interpreter_candidates()!=vec![(backend,definition.model.model_id.clone())] {
                    bail!("Model test requires the exact single applied request interpreter")
                }
                Ok(())
            })
        })?;
        let (runtime, _, mut generation) = self
            .input_workflow_runtime()
            .ok_or_else(|| anyhow::anyhow!("Model test runtime unavailable"))?;
        if runtime != definition.node_runtime_id
            || generation.borrow_and_update().as_deref() != Some(&runtime)
        {
            bail!("Model test runtime changed before preparation")
        }
        let command = definition.source()?;
        let mut report = PrepareReport {
            schema_version: 1,
            report_type: "node_model_verification_preparation".into(),
            definition: definition.clone(),
            source_operation_id: command.operation_id.clone(),
            success: false,
            preparation_attempted: true,
            outcome_unknown: true,
            source: None,
            execution_authorized: false,
            model_inference_requested: false,
            restart_requested: false,
            automatically_retried: false,
            note: NOTE.into(),
        };
        let source = tokio::select! {
            biased;
            _=generation.changed()=>None,
            value=self.concrete_request(crate::request_submission::ConcreteRequest {
                schema_version:1,expected_node_id:self.node_id.clone(),command:command.clone(),
            })=>value.ok(),
        };
        if let Some(source) = source {
            if source["success"] == true
                && source["coordinator_acknowledged"] == true
                && crate::request_submission::confirmed(&source["result"], &self.node_id, &command)
            {
                report.success = true;
                report.outcome_unknown = false;
                report.source = Some(source["result"].clone());
            } else if source["outcome_unknown"] == false {
                report.outcome_unknown = false;
            }
        }
        let value = json!(report);
        if value["success"] == true && !preparation_confirmed(&value, &definition) {
            bail!("Model test source unconfirmed; inspect receipt, no retry")
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service() -> (
        tempfile::TempDir,
        std::sync::Arc<crate::NodeService>,
        std::sync::Arc<crate::NodeEngine>,
        crate::request_preview::PreviewGuard,
        Definition,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::setup::defaults();
        config["node_id"] = json!("node");
        config["name"] = json!("Verification fixture");
        let path = dir.path().join("node.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let service = crate::NodeService::new(path, "node".into());
        let engine = std::sync::Arc::new(
            crate::NodeEngine::new(false, vec![], dir.path().into())
                .unwrap()
                .with_service(service.clone()),
        );
        let d = definition();
        service.live.lock().unwrap()["runtime_id"] = json!(d.node_runtime_id);
        let guard = service.attach_preview_runtime(&engine, d.node_runtime_id.clone());
        (dir, service, engine, guard, d)
    }
    fn definition() -> Definition {
        Definition {
            schema_version: 1,
            node_id: "node".into(),
            node_runtime_id: uuid::Uuid::new_v4().to_string(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            model: crate::setup_check::ModelRef {
                backend: crate::model_selection::Backend::Llamacpp,
                model_id: "chosen".into(),
            },
        }
    }
    #[test]
    fn fixed_source_is_exact_self_scoped_and_not_approval_or_interpretation() {
        let d = definition();
        let command = d.source().unwrap();
        assert_eq!(Definition::from_source(&command.request).unwrap(), d);
        assert_eq!(command.request.input.template, "generate_text");
        assert_eq!(
            command.request.input.constraints,
            json!({"node_id":"node","backend":"llamacpp","model_role":"request_interpreter"})
        );
        assert_eq!(command.request.input.params["max_tokens"], 32);
        for field in ["prompt", "messages", "tools", "file", "download"] {
            let mut changed = command.request.clone();
            changed.input.params[field] = json!("substitution");
            assert!(Definition::from_source(&changed).is_err(), "{field}");
        }
        assert!(!preparation_confirmed(&json!({"success":true}), &d));
    }
    #[test]
    fn receipt_domains_and_node_scope_do_not_conflate_authority_or_new_runtime() {
        let d = definition();
        let source = d.receipt("source", None).unwrap();
        let job = "a".repeat(32);
        let approval = d.receipt("approval", Some(&job)).unwrap();
        let execution = d.receipt("execution", Some(&job)).unwrap();
        assert_ne!(source, approval);
        assert_ne!(approval, execution);
        assert!(d.receipt("source", Some(&job)).is_err());
        assert!(d.receipt("approval", None).is_err());
        let mut replaced = d.clone();
        replaced.node_runtime_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            replaced.receipt("source", None).unwrap(),
            source,
            "a runtime change must conflict with old intent, not allocate another source"
        );
        assert_ne!(
            replaced.source().unwrap().request,
            d.source().unwrap().request
        );
        replaced.node_id = "other".into();
        assert_ne!(replaced.receipt("source", None).unwrap(), source);
    }
    #[tokio::test]
    async fn admission_refuses_remote_without_consent_or_local_without_exact_interpreter() {
        let (_dir, service, _engine, _guard, d) = service();
        let before = std::fs::read(&service.config_path).unwrap();
        let remote = service
            .prepare_model_verification(d.clone(), true)
            .await
            .unwrap_err();
        assert!(remote.to_string().contains("management consent"));
        let local = service
            .prepare_model_verification(d.clone(), false)
            .await
            .unwrap_err();
        assert!(
            local
                .to_string()
                .contains("single applied request interpreter")
        );
        assert_eq!(std::fs::read(&service.config_path).unwrap(), before);
        assert_eq!(
            service.submissions.state(),
            crate::submission::InputChannelState::Unavailable
        );
        assert_eq!(service.live.lock().unwrap()["pending_restart"], false);
    }
    #[tokio::test]
    async fn wrong_identity_runtime_and_busy_preparation_do_not_submit_input() {
        let (_dir, service, _engine, guard, d) = service();
        let before = std::fs::read(&service.config_path).unwrap();
        let mut bad = d.clone();
        bad.node_id = "other".into();
        assert!(
            service
                .prepare_model_verification(bad, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("another node")
        );
        let mut bad = d.clone();
        bad.node_runtime_id = uuid::Uuid::new_v4().to_string();
        assert!(
            service
                .prepare_model_verification(bad, false)
                .await
                .unwrap_err()
                .to_string()
                .contains("runtime changed")
        );
        let held = service.model_verification.clone().lock_owned().await;
        assert!(
            service
                .prepare_model_verification(d.clone(), false)
                .await
                .unwrap_err()
                .to_string()
                .contains("busy")
        );
        drop(held);
        drop(guard);
        assert!(service.prepare_model_verification(d, false).await.is_err());
        assert_eq!(std::fs::read(&service.config_path).unwrap(), before);
        assert_eq!(
            service.submissions.state(),
            crate::submission::InputChannelState::Unavailable
        );
    }
}
