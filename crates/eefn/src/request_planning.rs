//! Origin-scoped planning input and views. No approval, grants or local execution.

/// The authoritative existing template/action/parameter vocabulary shared with
/// interpretation. Entries are not grants, resource bindings or executable plans.
pub fn template_steps(
    template: &str,
) -> Result<Vec<(&'static str, &'static str, &'static [&'static str])>> {
    Ok(match template {
        "system_info" => vec![("system.info", "run", &[])],
        "generate_text" => vec![(
            "llm.infer",
            "run",
            &[
                "prompt",
                "messages",
                "model_id",
                "tier",
                "max_tokens",
                "temperature",
            ],
        )],
        "analyze_image" => vec![(
            "vlm.analyze",
            "run",
            &[
                "prompt",
                "messages",
                "images",
                "model_id",
                "tier",
                "max_tokens",
                "temperature",
            ],
        )],
        "launch_application" => {
            vec![("application.control", "launch", &["application", "args"])]
        }
        "read_sensor" => vec![("sensor", "read", &["name"])],
        "write_file" => vec![("filesystem", "write", &["path", "content"])],
        "read_file" => vec![("filesystem", "read", &["path"])],
        "list_directory" => vec![("filesystem", "list", &["path"])],
        "type_text" => vec![("input.control", "type", &["text", "interval"])],
        "press_key" => vec![("input.control", "press", &["key"])],
        "capture_screen" => vec![("screen.capture", "capture", &[])],
        "capture_camera" => vec![(
            "camera.capture",
            "capture",
            &["device", "width", "height", "quality"],
        )],
        "record_audio" => vec![(
            "audio.capture",
            "record",
            &["device", "duration_seconds", "sample_rate", "channels"],
        )],
        "play_audio" => vec![("audio.play", "play", &["data_base64"])],
        "speak_text" => vec![("tts.speak", "speak", &["text", "voice", "rate", "volume"])],
        "transcribe_audio" => vec![(
            "stt.transcribe",
            "transcribe",
            &["audio_base64", "audio_path", "filename"],
        )],
        "execute_process" => vec![(
            "process.exec",
            "run",
            &["program", "args", "cwd", "timeout_seconds"],
        )],
        "http_request" => vec![(
            "http.request",
            "send",
            &["url", "method", "headers", "body", "timeout_seconds"],
        )],
        "wake_computer" => vec![("network.wol", "wake", &["mac", "broadcast", "port"])],
        "observe_room" => vec![
            ("camera.capture", "capture", &["device"]),
            ("vlm.analyze", "analyze", &["prompt"]),
        ],
        _ => bail!("no template '{template}'"),
    })
}
/// All current one-step method templates. Compound wiring is intentionally not
/// advertised as a semantic method or a local continuous pipeline.
pub const METHOD_TEMPLATES: &[&str] = &[
    "system_info",
    "generate_text",
    "analyze_image",
    "launch_application",
    "read_sensor",
    "write_file",
    "read_file",
    "list_directory",
    "type_text",
    "press_key",
    "capture_screen",
    "capture_camera",
    "record_audio",
    "play_audio",
    "speak_text",
    "transcribe_audio",
    "execute_process",
    "http_request",
    "wake_computer",
];
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    pub schema_version: u32,
    pub template: String,
    pub params: Value,
    pub constraints: Value,
}
impl PlanInput {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.template)?;
        if self.schema_version != 1
            || !self.params.is_object()
            || !self.constraints.is_object()
            || serde_json::to_vec(self)?.len() > 64 * 1024
        {
            bail!("Invalid or oversized concrete plan input")
        }
        for (key, value) in self.constraints.as_object().unwrap() {
            match key.as_str() {
                "node_id" => crate::network::validate_remote_node_id(
                    value.as_str().context("node_id must be text")?,
                )?,
                "backend" | "model_role" | "gpu" => {
                    identifier(value.as_str().context("routing identifier must be text")?)?
                }
                "gpu_vram_min_mb" | "vram_min_mb" => {
                    value
                        .as_u64()
                        .context("VRAM constraint must be unsigned integer")?;
                }
                "area" => crate::context::validate_area(&serde_json::from_value::<Vec<String>>(
                    value.clone(),
                )?)?,
                "node_name" | "resource_id" => {
                    let text = value.as_str().context("selection must be text")?;
                    if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
                        bail!("Invalid selection")
                    }
                }
                _ => bail!("Unsupported concrete planning constraint '{key}'"),
            }
        }
        Ok(())
    }
}
fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        bail!("Expected bounded ASCII identifier")
    }
    Ok(())
}
pub fn read_input(path: &std::path::Path) -> Result<PlanInput> {
    use std::io::Read;
    let mut bytes = vec![];
    std::fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        bail!("Plan input exceeds 64 KiB")
    }
    let input: PlanInput =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
    input.validate()?;
    Ok(input)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanningCommand {
    Inspect {
        id: String,
    },
    Compile {
        id: String,
        expected_revision: u64,
        operation_id: String,
        input: PlanInput,
    },
}
impl PlanningCommand {
    pub fn id(&self) -> &str {
        match self {
            Self::Inspect { id } | Self::Compile { id, .. } => id,
        }
    }
    pub fn mutates(&self) -> bool {
        matches!(self, Self::Compile { .. })
    }
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(self.id())?;
        if let Self::Compile {
            expected_revision,
            operation_id,
            input,
            ..
        } = self
        {
            if *expected_revision == 0 || *expected_revision == u64::MAX {
                bail!("Exact positive planning revision required")
            }
            crate::job_commands::validate_operation_id(operation_id)?;
            input.validate()?;
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub command: PlanningCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedBinding {
    pub task_id: String,
    pub node_id: String,
    pub resource_id: Option<String>,
    pub model_id: Option<String>,
    pub backend: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedTask {
    pub task_id: String,
    pub capability: String,
    pub action: String,
    pub params: Value,
    pub constraints: Value,
    pub dependencies: Vec<String>,
    pub timeout_ms: u64,
    pub status: String,
}
/// Private original-node snapshot, not new discovery or an execution receipt.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningView {
    pub schema_version: u32,
    pub report_type: String,
    pub runtime_id: String,
    pub id: String,
    pub operation_id: String,
    pub revision: u64,
    pub status: String,
    pub request_context: crate::context::RequestContext,
    pub draft: crate::request_drafts::DraftRecord,
    pub input: Option<PlanInput>,
    pub planning_operation_id: Option<String>,
    pub source_revision: Option<u64>,
    pub plan_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<crate::request_preparation::PreparationSource>,
    pub bindings: Vec<PlannedBinding>,
    pub tasks: Vec<PlannedTask>,
    pub reused: bool,
    pub persisted: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, receipt: &str, command: &PlanningCommand) -> bool {
    let Ok(view) = serde_json::from_value::<PlanningView>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || crate::job_commands::validate_operation_id(receipt).is_err()
        || crate::job_commands::validate_operation_id(&view.runtime_id).is_err()
        || view.schema_version != 1
        || view.report_type != "coordinator_node_planning"
        || view.id != command.id()
        || view.operation_id != receipt
        || view.revision == 0
        || view.request_context.origin_node() != node
        || view.draft.validate().is_err()
        || view.draft.proposals[0].operation_id != view.request_context.request_id()
        || !view.persisted
        || view.execution_authorized
        || view.dispatched
        || view.automatically_retried
        || !matches!(
            view.status.as_str(),
            "review"
                | "awaiting_approval"
                | "approved"
                | "cancelled"
                | "running"
                | "stopping"
                | "completed"
                | "failed"
                | "interrupted"
                | "abandoned"
        )
        || !serde_json::to_vec(&view).is_ok_and(|b| b.len() <= 1024 * 1024)
    {
        return false;
    }
    match &view.input {
        None if view.planning_operation_id.is_some()
            || view.source_revision.is_some()
            || view.plan_sha256.is_some()
            || view.preparation.is_some()
            || !view.bindings.is_empty()
            || !view.tasks.is_empty() =>
        {
            return false;
        }
        Some(input) => {
            if let Some(source) = &view.preparation {
                if view.source_revision != Some(source.evidence.source_revision)
                    || !source.confirms(
                        &view.id,
                        source.evidence.source_revision,
                        &view.draft,
                        &view.request_context,
                    )
                    || crate::request_preparation::validate(
                        &view.draft,
                        &view.draft.current().suggested_capabilities,
                        &serde_json::to_vec(&crate::request_preparation::MethodProposal {
                            schema_version: 1,
                            method: Some(input.clone()),
                            clarification: None,
                        })
                        .unwrap_or_default(),
                    )
                    .is_err()
                {
                    return false;
                }
            }
            if input.validate().is_err()
                || view
                    .planning_operation_id
                    .as_ref()
                    .is_none_or(|id| crate::job_commands::validate_operation_id(id).is_err())
                || view
                    .source_revision
                    .is_none_or(|r| r == 0 || r >= view.revision)
                || view.plan_sha256.as_ref().is_none_or(|s| {
                    s.len() != 64
                        || !s
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                || view.tasks.is_empty()
                || view.tasks.len() > 16
                || view.bindings.len() != view.tasks.len()
            {
                return false;
            }
            let mut ids = std::collections::BTreeSet::new();
            for (task, binding) in view.tasks.iter().zip(&view.bindings) {
                if crate::request_drafts::validate_id(&task.task_id).is_err()
                    || task.dependencies.iter().any(|d| !ids.contains(d))
                    || task
                        .dependencies
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != task.dependencies.len()
                    || !ids.insert(&task.task_id)
                    || binding.task_id != task.task_id
                    || crate::network::validate_remote_node_id(&binding.node_id).is_err()
                    || task.constraints["node_id"] != binding.node_id
                    || !task.params.is_object()
                    || !task.constraints.is_object()
                    || identifier(&task.action).is_err()
                    || !view
                        .draft
                        .current()
                        .suggested_capabilities
                        .contains(&task.capability)
                    || task.timeout_ms == 0
                    || matches!(task.capability.as_str(), "llm.infer" | "vlm.analyze")
                        != (binding.model_id.is_some() && binding.backend.is_some())
                    || binding.model_id.is_some() != binding.backend.is_some()
                    || binding
                        .model_id
                        .as_ref()
                        .is_some_and(|m| crate::model_selection::validate_id(m).is_err())
                    || binding
                        .backend
                        .as_ref()
                        .is_some_and(|b| identifier(b).is_err())
                    || !matches!(
                        task.status.as_str(),
                        "pending"
                            | "running"
                            | "completed"
                            | "failed"
                            | "skipped"
                            | "cancelled"
                            | "interrupted"
                    )
                    || binding
                        .model_id
                        .as_ref()
                        .is_some_and(|m| task.params["model_id"] != *m)
                    || binding
                        .backend
                        .as_ref()
                        .is_some_and(|b| task.constraints["backend"] != *b)
                    || binding.resource_id.as_ref().is_some_and(|r| {
                        task.params["resource_id"] != *r || task.constraints["resource_id"] != *r
                    })
                {
                    return false;
                }
            }
            if view
                .tasks
                .iter()
                .any(|t| t.dependencies.iter().any(|d| !ids.contains(d)))
            {
                return false;
            }
        }
        None => {}
    }
    if view
        .draft
        .proposals
        .iter()
        .filter_map(|p| p.preparation.as_ref())
        .any(|source| {
            source.evidence.job_id != view.id
                || source.evidence.source_revision >= view.revision
                || source.request_context != view.request_context
        })
    {
        return false;
    }
    match command {
        PlanningCommand::Inspect { .. } => !view.reused,
        PlanningCommand::Compile {
            expected_revision,
            operation_id,
            input,
            ..
        } => {
            receipt == operation_id
                && view.input.as_ref() == Some(input)
                && view.planning_operation_id.as_ref() == Some(operation_id)
                && view.source_revision == Some(*expected_revision)
                && (view.reused
                    || (view.revision == expected_revision + 1
                        && view.status == "awaiting_approval"
                        && view.tasks.iter().all(|t| t.status == "pending")))
        }
    }
}
impl crate::NodeService {
    pub async fn planning_request(&self, request: PlanningRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Planning schema/node changed")
        }
        request.command.validate()?;
        crate::job_commands::validate_operation_id(&request.request_id)?;
        if let PlanningCommand::Compile { operation_id, .. } = &request.command {
            if operation_id != &request.request_id {
                bail!("Planning receipt mismatch")
            }
        }
        let mut report = json!({"schema_version":1,"report_type":"node_request_planning","node_id":self.node_id,
            "request_id":request.request_id,"command":request.command,"success":false,"coordinator_acknowledged":false,
            "outcome_unknown":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false});
        match self
            .submissions
            .request_correlated(
                "requests.planning",
                json!(request.command),
                Some(request.request_id.clone()),
            )
            .await
        {
            Ok(value)
                if confirmed(&value, &self.node_id, &request.request_id, &request.command) =>
            {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = value;
            }
            Err(error)
                if error
                    .downcast_ref::<crate::submission::SubmissionFailure>()
                    .is_some() =>
            {
                report["error_code"] = json!("planning_not_confirmed");
                report["outcome_unknown"] = json!(
                    request.command.mutates()
                        && !matches!(
                            error.downcast_ref::<crate::submission::SubmissionFailure>(),
                            Some(crate::submission::SubmissionFailure::NotSent(_))
                        )
                );
            }
            _ => {
                report["error_code"] = json!("planning_unconfirmed");
                report["outcome_unknown"] = json!(request.command.mutates());
            }
        }
        report["note"] = json!(
            "Private origin-owned plan only. No approval/grant/dispatch or automatic retry; inspect retained plan after an uncertain reply."
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, PlanningCommand, String) {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let task = uuid::Uuid::new_v4().simple().to_string();
        let opening = uuid::Uuid::new_v4().to_string();
        let receipt = uuid::Uuid::new_v4().to_string();
        let input = PlanInput {
            schema_version: 1,
            template: "system_info".into(),
            params: json!({}),
            constraints: json!({"node_id":"owner"}),
        };
        let command = PlanningCommand::Compile {
            id: id.clone(),
            expected_revision: 1,
            operation_id: receipt.clone(),
            input: input.clone(),
        };
        let view = json!({"schema_version":1,"report_type":"coordinator_node_planning","runtime_id":uuid::Uuid::new_v4().to_string(),
            "id":id,"operation_id":receipt,"revision":2,"status":"awaiting_approval",
            "request_context":crate::context::RequestContext::new(opening.clone(),"owner".into(),vec!["Room".into()]).unwrap(),
            "draft":{"schema_version":1,"original_text":"Inspect status","proposals":[{"operation_id":opening,"at_ms":1,
                "allowed_capabilities":["system.info"],"proposal":{"schema_version":1,"intent":"information","goal":"Inspect status",
                    "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],"complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null}}],"answers":[]},
            "input":input,"planning_operation_id":receipt,"source_revision":1,"plan_sha256":"a".repeat(64),
            "bindings":[{"task_id":task,"node_id":"owner","resource_id":null,"model_id":null,"backend":null}],
            "tasks":[{"task_id":task,"capability":"system.info","action":"run","params":{},"constraints":{"node_id":"owner"},"dependencies":[],"timeout_ms":30000,"status":"pending"}],
            "reused":false,"persisted":true,"execution_authorized":false,"dispatched":false,"automatically_retried":false});
        (view, command, receipt)
    }
    #[test]
    fn planning_views_bind_origin_receipt_input_revision_graph_and_no_execution() {
        let (view, command, receipt) = fixture();
        assert!(confirmed(&view, "owner", &receipt, &command));
        assert!(!confirmed(&view, "other", &receipt, &command));
        assert!(!confirmed(
            &view,
            "owner",
            &uuid::Uuid::new_v4().to_string(),
            &command
        ));
        for (pointer, value) in [
            ("/execution_authorized", json!(true)),
            ("/dispatched", json!(true)),
            ("/automatically_retried", json!(true)),
            ("/persisted", json!(false)),
            ("/source_revision", json!(0)),
            ("/revision", json!(1)),
            ("/input/template", json!("write_file")),
            ("/bindings/0/node_id", json!("other")),
            (
                "/bindings/0/task_id",
                json!(uuid::Uuid::new_v4().simple().to_string()),
            ),
            ("/bindings/0/model_id", json!("ungranted")),
            ("/bindings/0/resource_id", json!("other::cam")),
            ("/tasks/0/timeout_ms", json!(0)),
            (
                "/tasks/0/dependencies",
                json!([view["tasks"][0]["task_id"]]),
            ),
            ("/tasks/0/capability", json!("filesystem")),
            ("/plan_sha256", json!("A".repeat(64))),
        ] {
            let mut changed = view.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                !confirmed(&changed, "owner", &receipt, &command),
                "{pointer}"
            );
        }
        let mut widened = view.clone();
        widened["approved"] = json!(true);
        assert!(!confirmed(&widened, "owner", &receipt, &command));
        let mut reused = view.clone();
        reused["reused"] = json!(true);
        reused["revision"] = json!(3);
        reused["status"] = json!("approved");
        assert!(confirmed(&reused, "owner", &receipt, &command));
        let mut changed = command.clone();
        if let PlanningCommand::Compile {
            expected_revision, ..
        } = &mut changed
        {
            *expected_revision = 2;
        }
        assert!(!confirmed(&reused, "owner", &receipt, &changed));
        let inspect = PlanningCommand::Inspect {
            id: command.id().into(),
        };
        let mut review = view.clone();
        review["revision"] = json!(1);
        review["status"] = json!("review");
        for key in [
            "input",
            "planning_operation_id",
            "source_revision",
            "plan_sha256",
        ] {
            review[key] = Value::Null;
        }
        review["tasks"] = json!([]);
        review["bindings"] = json!([]);
        assert!(confirmed(&review, "owner", &receipt, &inspect));
        review["bindings"] = view["bindings"].clone();
        assert!(!confirmed(&review, "owner", &receipt, &inspect));
    }
    #[test]
    fn shared_input_rejects_unknown_authority_duplicate_fields_invalid_routes_and_large_files() {
        let (view, _, _) = fixture();
        let input: PlanInput = serde_json::from_value(view["input"].clone()).unwrap();
        input.validate().unwrap();
        for constraints in [
            json!({"node_id":"local"}),
            json!({"approved":true}),
            json!({"node_id":"*"}),
            json!({"area":[""]}),
            json!({"gpu_vram_min_mb":-1}),
            json!({"node_name":"x\n"}),
        ] {
            let mut changed = input.clone();
            changed.constraints = constraints;
            assert!(changed.validate().is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input.json");
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(serde_json::to_vec(&input).unwrap());
        std::fs::write(&file, bom).unwrap();
        assert_eq!(read_input(&file).unwrap(), input);
        std::fs::write(&file, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_input(&file).is_err());
        assert!(serde_json::from_str::<PlanInput>(r#"{"schema_version":1,"template":"system_info","template":"write_file","params":{},"constraints":{}}"#).is_err());
        assert!(
            serde_json::from_value::<PlanningCommand>(
                json!({"operation":"approve","id":view["id"],"approved":true})
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn disconnected_node_planning_remains_controllable_and_never_falls_back_to_execution() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let service = crate::NodeService::new(config.clone(), "owner".into());
        let (_, command, receipt) = fixture();
        let report = service
            .planning_request(PlanningRequest {
                schema_version: 1,
                expected_node_id: "owner".into(),
                request_id: receipt.clone(),
                command: command.clone(),
            })
            .await
            .unwrap();
        assert_eq!(report["success"], false);
        assert_eq!(report["outcome_unknown"], false);
        assert_eq!(report["execution_authorized"], false);
        assert!(!config.exists());
        assert!(!crate::grants::journal_path(&config).exists());
        assert!(
            service
                .planning_request(PlanningRequest {
                    schema_version: 1,
                    expected_node_id: "other".into(),
                    request_id: receipt.clone(),
                    command: command.clone()
                })
                .await
                .is_err()
        );
        assert!(
            service
                .planning_request(PlanningRequest {
                    schema_version: 1,
                    expected_node_id: "owner".into(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    command
                })
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn rejected_or_corrupt_compile_confirmation_is_not_proof_no_plan_was_saved() {
        let dir = tempfile::tempdir().unwrap();
        let service = crate::NodeService::new(dir.path().join("node.json"), "owner".into());
        let mut queue = service.submissions.connect();
        let (_, command, receipt) = fixture();
        for rejected in [true, false] {
            let local = service.clone();
            let command = command.clone();
            let receipt = receipt.clone();
            let pending = tokio::spawn(async move {
                local
                    .planning_request(PlanningRequest {
                        schema_version: 1,
                        expected_node_id: "owner".into(),
                        request_id: receipt,
                        command,
                    })
                    .await
            });
            let submission = queue.recv().await.unwrap();
            assert_eq!(submission.kind, "requests.planning");
            let result = if rejected {
                Err(crate::submission::SubmissionFailure::Rejected(
                    "confirmation could have failed after checkpoint".into(),
                )
                .into())
            } else {
                Ok(json!({"success":true,"approved":true}))
            };
            submission.reply.send(result).unwrap();
            let report = pending.await.unwrap().unwrap();
            assert_eq!(report["success"], false);
            assert_eq!(report["outcome_unknown"], true);
            assert_eq!(report["coordinator_acknowledged"], false);
            assert_eq!(report["execution_authorized"], false);
            assert!(
                queue.try_recv().is_err(),
                "No implicit retry or inspection submission"
            );
        }
    }
}
