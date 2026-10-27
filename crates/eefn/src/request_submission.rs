//! Caller-supplied concrete requests use eef review/compilation, never inference.
use crate::{
    interpretation::{Complexity, Intent, Proposal, Workload},
    request_planning::{PlanInput, PlanningCommand, PlanningView},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcreteInput {
    pub schema_version: u32,
    pub original_text: String,
    pub title: String,
    pub input: PlanInput,
}
impl ConcreteInput {
    pub fn validate(&self) -> Result<()> {
        self.input.validate()?;
        crate::interpretation::messages(&self.original_text, &[])?;
        if self.schema_version != 1
            || self.title.trim().is_empty()
            || self.title.len() > 512
            || self.title.chars().any(char::is_control)
            || (!crate::request_planning::METHOD_TEMPLATES.contains(&self.input.template.as_str())
                && !matches!(
                    self.input.template.as_str(),
                    "local_pipeline" | "conditional_pipeline" | "peer_transfer"
                ))
            || serde_json::to_vec(self)?.len() > 160 * 1024
        {
            bail!("Invalid explicit concrete request")
        }
        Ok(())
    }
    /// Deterministic description of an explicit template, not an LM prediction.
    pub fn proposal(&self) -> Result<Proposal> {
        self.validate()?;
        let capabilities = crate::request_planning::concrete_capabilities(&self.input)?;
        Ok(Proposal {
            schema_version: 1,
            intent: Intent::Action,
            goal: self.title.clone(),
            context_hints: vec![],
            constraints: vec![],
            suggested_capabilities: capabilities,
            complexity: Complexity::Unknown,
            workload: Workload::OneShot,
            needs_clarification: false,
            clarification: None,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcreteCommand {
    pub operation_id: String,
    pub request: ConcreteInput,
}
impl ConcreteCommand {
    pub fn validate(&self) -> Result<()> {
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        self.request.validate()
    }
}
/// File-free typed source authoring. This constructs the same concrete request;
/// it never interprets shell text, changes permissions or authorizes execution.
#[derive(Clone, Debug, clap::Args)]
pub struct ProcessSubmission {
    #[arg(long)]
    pub text: String,
    #[arg(long, default_value = "Explicit process request")]
    pub title: String,
    #[arg(long, value_name = "PROGRAM")]
    pub program: String,
    /// One literal argument; repeat this option. Use --arg=-flag for leading hyphens.
    #[arg(long = "arg", value_name = "ARG")]
    pub args: Vec<String>,
    #[arg(long)]
    pub cwd: Option<String>,
    /// Requested process timeout, still limited independently by executor policy/grants.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout_seconds: Option<u64>,
    /// Exact executor selection; only fresh self/owner-disclosed nodes are eligible.
    #[arg(long)]
    pub node: Option<String>,
    /// Retain the returned receipt after uncertainty; no automatic retry or execution.
    #[arg(long)]
    pub operation_id: Option<String>,
}
impl ProcessSubmission {
    pub fn command(&self) -> Result<ConcreteCommand> {
        if self.program.trim().is_empty()
            || self.program.contains('\0')
            || self.args.iter().any(|arg| arg.contains('\0'))
            || self
                .cwd
                .as_ref()
                .is_some_and(|cwd| cwd.trim().is_empty() || cwd.contains('\0'))
            || self
                .timeout_seconds
                .is_some_and(|n| !(1..=3600).contains(&n))
        {
            bail!(
                "Invalid explicit process source; use a program and literal arguments, not shell text"
            );
        }
        let args = crate::engine::validated_arguments(&json!(self.args))?;
        let mut params = json!({"program":self.program,"args":args});
        if let Some(cwd) = &self.cwd {
            params["cwd"] = json!(cwd);
        }
        if let Some(seconds) = self.timeout_seconds {
            params["timeout_seconds"] = json!(seconds);
        }
        let command = ConcreteCommand {
            operation_id: self
                .operation_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            request: ConcreteInput {
                schema_version: 1,
                original_text: self.text.clone(),
                title: self.title.clone(),
                input: PlanInput {
                    schema_version: 1,
                    template: "execute_process".into(),
                    params,
                    constraints: self
                        .node
                        .as_ref()
                        .map(|node| json!({"node_id":node}))
                        .unwrap_or_else(|| json!({})),
                },
            },
        };
        command.validate()?;
        Ok(command)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcreteRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: ConcreteCommand,
}

/// A separate, deterministic compilation receipt. Not a token or permission.
/// Including the saved job ID prevents conflating removed/recreated history.
pub fn compilation_receipt(origin: &str, opening: &str, job: &str) -> Result<String> {
    crate::network::validate_remote_node_id(origin)?;
    crate::job_commands::validate_operation_id(opening)?;
    crate::request_drafts::validate_id(job)?;
    let digest = Sha256::digest(serde_json::to_vec(&json!([
        "eef.concrete.compile.v1",
        origin,
        opening,
        job
    ]))?);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 custom UUID (version 8), derived rather than randomly generated.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcreteReport {
    pub schema_version: u32,
    pub report_type: String,
    pub operation_id: String,
    pub compilation_operation_id: String,
    pub creation_reused: bool,
    pub plan: PlanningView,
    pub job: Value,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
pub fn confirmed(value: &Value, node: &str, command: &ConcreteCommand) -> bool {
    confirmed_with_source(value, node, command, None)
}
pub fn confirmed_with_source(
    value: &Value,
    node: &str,
    command: &ConcreteCommand,
    source: Option<&crate::audio_submission::AudioInput>,
) -> bool {
    if let Some(source) = source {
        let Ok(pair) = crate::peer::plan::Pair::from_params(&command.request.input.params) else {
            return false;
        };
        if !source
            .concrete(&pair, Some(node))
            .is_ok_and(|expected| expected == command.request)
        {
            return false;
        }
    }
    if command.validate().is_err() {
        return false;
    }
    let Ok(r) = serde_json::from_value::<ConcreteReport>(value.clone()) else {
        return false;
    };
    let Ok(receipt) = compilation_receipt(node, &command.operation_id, &r.plan.id) else {
        return false;
    };
    let Ok(proposal) = command.request.proposal() else {
        return false;
    };
    let expected = PlanningCommand::Compile {
        id: r.plan.id.clone(),
        expected_revision: 1,
        operation_id: receipt.clone(),
        input: command.request.input.clone(),
    };
    r.schema_version == 1
        && r.report_type == "coordinator_concrete_request"
        && r.operation_id == command.operation_id
        && r.compilation_operation_id == receipt
        && !r.execution_authorized
        && !r.dispatched
        && !r.automatically_retried
        && r.plan.preparation.is_none()
        && r.plan.request_context.request_id() == command.operation_id
        && r.plan.draft.original_text == command.request.original_text
        && r.plan.draft.concrete_input.as_ref() == Some(&command.request.input)
        && r.plan.draft.audio_source.as_ref() == source
        && r.plan.draft.proposals.len() == 1
        && r.plan.draft.answers.is_empty()
        && r.plan.draft.proposals[0].proposal == proposal
        && r.job["id"] == r.plan.id
        && r.job["status"] == r.plan.status
        && r.job["revision"].as_u64() == Some(r.plan.revision)
        && r.job["request_context"] == json!(r.plan.request_context)
        && r.job["can_resume"] == false
        && r.job["task_count"].as_u64() == Some(r.plan.tasks.len() as u64)
        && crate::request_planning::confirmed(&json!(r.plan), node, &receipt, &expected)
}
impl crate::NodeService {
    pub async fn concrete_request(&self, request: ConcreteRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Explicit request schema/node changed")
        }
        request.command.validate()?;
        let mut report = json!({"schema_version":1,"report_type":"node_concrete_request","node_id":self.node_id,
            "command":request.command,"success":false,"coordinator_acknowledged":false,"outcome_unknown":false,
            "execution_authorized":false,"dispatched":false,"automatically_retried":false});
        match self
            .submissions
            .request_correlated(
                "requests.concrete",
                json!(request.command.request),
                Some(request.command.operation_id.clone()),
            )
            .await
        {
            Ok(value) if confirmed(&value, &self.node_id, &request.command) => {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = value;
            }
            Err(error)
                if matches!(
                    error.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                ) =>
            {
                report["error_code"] = json!("concrete_not_sent");
            }
            _ => {
                report["outcome_unknown"] = json!(true);
                report["error_code"] = json!("concrete_unconfirmed");
            }
        }
        report["note"] = json!(
            "Explicit input saved/compiled only; separate owner approval and execution are required. After uncertainty inspect jobs find with this operation ID, then requests plan; no automatic retry or inference fallback."
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_pipeline_union_is_exact_without_relaxing_model_hint_limits() {
        let request:ConcreteInput=serde_json::from_value(json!({"schema_version":1,"original_text":"Run the explicitly selected local stages.","title":"Finite local stages",
            "input":{"schema_version":1,"template":"local_pipeline","constraints":{},"params":{"pipeline":{"schema_version":1,"max_retained_bytes":1024,"steps":[
                {"id":"info","capability":"system.info","action":"run","params":{}},
                {"id":"file","capability":"filesystem","action":"write","params":{"path":"fixed.txt","content":"fixed"}},
                {"id":"process","capability":"process.exec","action":"run","params":{"program":"owner-fixed"}},
                {"id":"network","capability":"http.request","action":"send","params":{"url":"https://owner.example"}},
                {"id":"app","capability":"application.control","action":"launch","params":{"application":"owner-fixed"}},
                {"id":"tts","capability":"tts.speak","action":"speak","params":{"text":"fixed"}},
                {"id":"camera","capability":"camera.capture","action":"capture","params":{}},
                {"id":"audio","capability":"audio.capture","action":"record","params":{}},
                {"id":"ping","capability":"system.ping","action":"run","params":{}}
            ]}}}})).unwrap();
        let proposal = request.proposal().unwrap();
        assert!(proposal.suggested_capabilities.len() > 8);
        let allowed = proposal.suggested_capabilities.clone();
        crate::interpretation::validate_concrete(&request, &allowed).unwrap();
        assert!(
            crate::interpretation::validate(
                &request.original_text,
                &serde_json::to_vec(&proposal).unwrap(),
                &allowed
            )
            .is_err()
        );
        let draft = crate::request_drafts::DraftRecord {
            schema_version: 1,
            original_text: request.original_text,
            concrete_input: Some(request.input),
            audio_source: None,
            answers: vec![],
            proposals: vec![crate::request_drafts::ProposalRevision {
                operation_id: uuid::Uuid::new_v4().to_string(),
                proposal,
                allowed_capabilities: allowed,
                at_ms: 1,
                resolution: None,
                preparation: None,
            }],
        };
        draft.validate().unwrap();
        let mut changed = draft.clone();
        changed.proposals[0].proposal.suggested_capabilities.pop();
        assert!(changed.validate().is_err());
        let mut absent = draft.clone();
        absent.concrete_input = None;
        assert!(
            absent.validate().is_err(),
            "large model hints cannot claim the explicit-only limit"
        );
    }
    #[test]
    fn concrete_pipeline_keeps_all_stage_hints_without_semantic_or_authority_options() {
        let request:ConcreteInput=serde_json::from_value(json!({"schema_version":1,"original_text":"Copy the selected input locally.","title":"Local dataflow",
            "input":{"schema_version":1,"template":"local_pipeline","constraints":{"node_id":"owner"},"params":{"pipeline":{"schema_version":1,"max_retained_bytes":1024,"steps":[
                {"id":"read","capability":"filesystem","action":"read","params":{"path":"source.txt"}},
                {"id":"write","capability":"filesystem","action":"write","params":{"path":"target.txt"},"inputs":[{"parameter":"content","source_step":"read","pointer":"/content"}]}
            ]}}}})).unwrap();
        request.validate().unwrap();
        assert_eq!(
            request.proposal().unwrap().suggested_capabilities,
            vec!["pipeline.run", "filesystem"]
        );
        assert_eq!(request.proposal().unwrap().workload, Workload::OneShot);
        assert!(
            !crate::request_planning::METHOD_TEMPLATES.contains(&"local_pipeline"),
            "not silently advertised as an LM method"
        );
        for key in ["backend", "resource_id", "model_role"] {
            let mut bad = request.clone();
            bad.input.constraints[key] = json!("ignored");
            assert!(bad.validate().is_err());
        }
        let mut bad = request.clone();
        bad.input.params["approval"] = json!(true);
        assert!(bad.validate().is_err());
    }
    fn process_input() -> ProcessSubmission {
        ProcessSubmission {
            text: "  Check the selected tool\n".into(),
            title: "Selected tool check".into(),
            program: "C:\\Program Files\\Owner Tool\\tool.exe".into(),
            args: vec![
                "--version".into(),
                "literal spaces ; $()".into(),
                String::new(),
            ],
            cwd: Some("C:\\Owner workspace".into()),
            timeout_seconds: Some(12),
            node: Some("other-node".into()),
            operation_id: Some(uuid::Uuid::new_v4().to_string()),
        }
    }
    #[test]
    fn typed_process_source_preserves_literal_parameters_text_and_exact_receipt() {
        let input = process_input();
        let command = input.command().unwrap();
        assert_eq!(
            command.operation_id,
            input.operation_id.as_ref().unwrap().as_str()
        );
        assert_eq!(command.request.original_text, input.text);
        assert_eq!(command.request.title, input.title);
        assert_eq!(command.request.input.template, "execute_process");
        assert_eq!(
            command.request.input.params,
            json!({"program":input.program,"args":input.args,"cwd":input.cwd,"timeout_seconds":12})
        );
        assert_eq!(
            command.request.input.constraints,
            json!({"node_id":"other-node"})
        );
        assert_eq!(input.command().unwrap(), command);
        let wire = json!(command);
        for field in ["origin_node", "approval", "grant", "execute", "permissions"] {
            assert!(wire.get(field).is_none());
            assert!(wire["request"].get(field).is_none());
        }
    }
    #[test]
    fn typed_process_defaults_omit_unselected_limits_targets_and_generate_returnable_receipts() {
        let mut input = process_input();
        input.operation_id = None;
        input.cwd = None;
        input.timeout_seconds = None;
        input.node = None;
        input.args.clear();
        let one = input.command().unwrap();
        let two = input.command().unwrap();
        assert_ne!(one.operation_id, two.operation_id);
        assert!(!uuid::Uuid::parse_str(&one.operation_id).unwrap().is_nil());
        assert_eq!(
            one.request.input.params,
            json!({"program":input.program,"args":[]})
        );
        assert_eq!(one.request.input.constraints, json!({}));
    }
    #[test]
    fn typed_process_invalid_arguments_and_source_fields_fail_before_submission() {
        for field in [
            "program",
            "program_nul",
            "args_nul",
            "cwd",
            "cwd_nul",
            "text",
            "title",
            "node",
            "receipt",
            "timeout_zero",
            "timeout_large",
        ] {
            let mut bad = process_input();
            match field {
                "program" => bad.program = "  ".into(),
                "program_nul" => bad.program = "tool\0.exe".into(),
                "args_nul" => bad.args = vec!["one\0two".into()],
                "cwd" => bad.cwd = Some(" ".into()),
                "cwd_nul" => bad.cwd = Some("C:\\one\0two".into()),
                "text" => bad.text = String::new(),
                "title" => bad.title = "invalid\ntitle".into(),
                "node" => bad.node = Some("invalid\nnode".into()),
                "receipt" => bad.operation_id = Some("not-a-receipt".into()),
                "timeout_zero" => bad.timeout_seconds = Some(0),
                "timeout_large" => bad.timeout_seconds = Some(3601),
                _ => unreachable!(),
            }
            assert!(bad.command().is_err(), "{field}");
        }
    }
    #[test]
    fn typed_process_uses_existing_argument_count_byte_and_wire_bounds() {
        let mut input = process_input();
        input.args = vec![String::new(); 128];
        assert!(input.command().is_ok());
        input.args.push(String::new());
        assert!(input.command().is_err());
        input.args = vec!["x".repeat(8192)];
        assert!(input.command().is_ok());
        input.args = vec!["x".repeat(8193)];
        assert!(input.command().is_err());
        input.args = vec!["x".repeat(8192); 9];
        assert!(input.command().is_err());
        input.args.clear();
        input.program = "x".repeat(64 * 1024);
        assert!(input.command().is_err());
    }
    fn input() -> ConcreteInput {
        ConcreteInput {
            schema_version: 1,
            original_text: "  Inspect status\n".into(),
            title: "Status".into(),
            input: PlanInput {
                schema_version: 1,
                template: "system_info".into(),
                params: json!({}),
                constraints: json!({}),
            },
        }
    }
    fn fixture() -> (Value, ConcreteCommand) {
        let command = ConcreteCommand {
            operation_id: uuid::Uuid::new_v4().to_string(),
            request: input(),
        };
        let job = uuid::Uuid::new_v4().simple().to_string();
        let task = uuid::Uuid::new_v4().simple().to_string();
        let receipt = compilation_receipt("owner", &command.operation_id, &job).unwrap();
        let context = crate::context::RequestContext::new(
            command.operation_id.clone(),
            "owner".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let value = json!({"schema_version":1,"report_type":"coordinator_concrete_request","operation_id":command.operation_id,
            "compilation_operation_id":receipt,"creation_reused":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false,
            "plan":{"schema_version":1,"report_type":"coordinator_node_planning","runtime_id":uuid::Uuid::new_v4().to_string(),
                "id":job,"operation_id":receipt,"revision":2,"status":"awaiting_approval","request_context":context,
                "draft":{"schema_version":1,"original_text":command.request.original_text,"concrete_input":command.request.input,
                    "proposals":[{"operation_id":command.operation_id,"proposal":command.request.proposal().unwrap(),"allowed_capabilities":["system.info"],"at_ms":1}],"answers":[]},
                "input":command.request.input,"planning_operation_id":receipt,"source_revision":1,"plan_sha256":"a".repeat(64),
                "bindings":[{"task_id":task,"node_id":"owner","resource_id":null,"model_id":null,"backend":null}],
                "tasks":[{"task_id":task,"capability":"system.info","action":"run","params":{},"constraints":{"node_id":"owner"},
                    "dependencies":[],"timeout_ms":30000,"status":"pending"}],"reused":false,"persisted":true,
                "execution_authorized":false,"dispatched":false,"automatically_retried":false},
            "job":{"id":job,"status":"awaiting_approval","revision":2,"request_context":context,"can_resume":false,"task_count":1}});
        (value, command)
    }
    #[test]
    fn explicit_input_is_bounded_and_deterministic_not_model_attribution_or_authority() {
        let source = input();
        source.validate().unwrap();
        assert_eq!(source.proposal().unwrap().complexity, Complexity::Unknown);
        for field in [
            "version",
            "title",
            "text",
            "template",
            "params",
            "constraint",
        ] {
            let mut bad = source.clone();
            match field {
                "version" => bad.schema_version = 2,
                "title" => bad.title = "x".repeat(513),
                "text" => bad.original_text = "".into(),
                "template" => bad.input.template = "observe_room".into(),
                "params" => bad.input.params = json!([]),
                "constraint" => bad.input.constraints = json!({"approved":true}),
                _ => unreachable!(),
            }
            assert!(bad.validate().is_err(), "{field}");
        }
        let mut bad = json!(source);
        bad["origin_node"] = json!("forged");
        assert!(serde_json::from_value::<ConcreteInput>(bad).is_err());
        let (value, command) = fixture();
        let draft: crate::request_drafts::DraftRecord =
            serde_json::from_value(value["plan"]["draft"].clone()).unwrap();
        assert!(crate::request_preparation::eligible(&draft).is_err());
        let mut command = command;
        command.operation_id = "invalid".into();
        assert!(command.validate().is_err());
    }
    #[test]
    fn compile_receipts_and_confirmation_pin_opening_job_input_source_and_no_dispatch() {
        let (value, command) = fixture();
        assert!(confirmed(&value, "owner", &command));
        assert!(!confirmed(&value, "other", &command));
        let receipt = value["compilation_operation_id"].as_str().unwrap();
        assert_ne!(receipt, command.operation_id);
        assert_eq!(uuid::Uuid::parse_str(receipt).unwrap().get_version_num(), 8);
        assert_ne!(
            receipt,
            compilation_receipt(
                "other",
                &command.operation_id,
                value["plan"]["id"].as_str().unwrap()
            )
            .unwrap()
        );
        assert_ne!(
            receipt,
            compilation_receipt(
                "owner",
                &command.operation_id,
                &uuid::Uuid::new_v4().simple().to_string()
            )
            .unwrap()
        );
        for (pointer, replacement) in [
            ("/dispatched", json!(true)),
            ("/execution_authorized", json!(true)),
            ("/plan/draft/concrete_input/template", json!("http_request")),
            ("/plan/draft/original_text", json!("changed")),
            ("/plan/draft/proposals/0/proposal/goal", json!("changed")),
            (
                "/compilation_operation_id",
                json!(uuid::Uuid::new_v4().to_string()),
            ),
            ("/job/status", json!("completed")),
            ("/job/can_resume", json!(true)),
        ] {
            let mut bad = value.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(!confirmed(&bad, "owner", &command), "{pointer}");
        }
        let mut historical = value.clone();
        historical["creation_reused"] = json!(true);
        historical["plan"]["reused"] = json!(true);
        historical["plan"]["status"] = json!("cancelled");
        historical["plan"]["revision"] = json!(3);
        historical["plan"]["tasks"][0]["status"] = json!("cancelled");
        historical["job"]["status"] = json!("cancelled");
        historical["job"]["revision"] = json!(3);
        assert!(confirmed(&historical, "owner", &command));
    }
}
