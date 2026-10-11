//! Durable automation authoring contracts. No timer, approval or execution.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, clap::Subcommand)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationCommand {
    /// List retained definitions; does not start timers or work.
    List {
        #[arg(long, default_value_t = 25)]
        limit: u16,
        #[arg(long)]
        after: Option<String>,
    },
    /// Inspect one private definition and its source snapshot.
    Show { id: String },
    /// Save a disabled definition from an exact unexecuted compiled request.
    Define {
        #[arg(long)]
        operation_id: String,
        #[arg(long)]
        from_job: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        plan_sha256: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        every_ms: u64,
        #[arg(long)]
        max_runs: u16,
        #[arg(long)]
        valid_for_ms: u64,
    },
    /// Retain a compact removal receipt; never cancel unknown remote effects.
    Remove {
        id: String,
        #[arg(long)]
        revision: u64,
        #[arg(long)]
        operation_id: String,
    },
}
impl AutomationCommand {
    pub fn receipt(&self) -> Option<&str> {
        match self {
            Self::Define { operation_id, .. } | Self::Remove { operation_id, .. } => {
                Some(operation_id)
            }
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(id) = self.receipt() {
            crate::job_commands::validate_operation_id(id)?;
        }
        match self {
            Self::List { limit, after } => {
                if !(1..=100).contains(limit) {
                    bail!("Automation page limit must be 1-100")
                }
                if let Some(id) = after {
                    crate::job_commands::validate_operation_id(id)?;
                }
            }
            Self::Show { id } | Self::Remove { id, .. } => {
                crate::job_commands::validate_operation_id(id)?;
            }
            Self::Define {
                from_job,
                revision,
                plan_sha256,
                name,
                every_ms,
                max_runs,
                valid_for_ms,
                ..
            } => {
                crate::request_drafts::validate_id(from_job)?;
                if *revision == 0
                    || *revision == u64::MAX
                    || !crate::request_result::valid_digest(plan_sha256)
                    || name.trim().is_empty()
                    || name.len() > 128
                    || name.chars().any(char::is_control)
                    || !(1_000..=86_400_000).contains(every_ms)
                    || !(1..=128).contains(max_runs)
                    || !(1_000..=86_400_000).contains(valid_for_ms)
                    || valid_for_ms < every_ms
                {
                    bail!("Invalid source, name or bounded automation timing/run limits")
                }
            }
        }
        if matches!(self, Self::Remove {revision, ..} if *revision!=1) {
            bail!("Only a revision-one disabled definition can be removed")
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalReceipt {
    pub operation_id: String,
    pub source_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema_version: u32,
    pub id: String,
    pub owner_context: crate::context::RequestContext,
    pub created_at_ms: u64,
    pub creation: AutomationCommand,
    pub source_sha256: String,
    pub definition_sha256: String,
    pub revision: u64,
    pub state: String,
    pub source: Option<Value>,
    pub removal: Option<RemovalReceipt>,
}
impl Definition {
    pub fn digest(&self) -> Result<String> {
        crate::grants::definition_digest(&json!({"schema_version":self.schema_version,"id":self.id,
            "owner_context":self.owner_context,"created_at_ms":self.created_at_ms,"creation":self.creation,
            "source_sha256":self.source_sha256}))
    }
    pub fn validate(&self) -> Result<()> {
        self.creation.validate()?;
        let AutomationCommand::Define {
            operation_id,
            from_job,
            revision,
            plan_sha256,
            ..
        } = &self.creation
        else {
            bail!("Missing definition creation")
        };
        if self.schema_version != 1
            || self.id != *operation_id
            || self.created_at_ms == 0
            || !crate::request_result::valid_digest(&self.source_sha256)
            || self.digest()? != self.definition_sha256
        {
            bail!("Damaged or future automation definition; retain for inspection")
        }
        crate::network::validate_remote_node_id(self.owner_context.origin_node())?;
        match (&self.source, &self.removal) {
            (Some(source), None) if self.revision == 1 && self.state == "disabled" => {
                if crate::grants::definition_digest(source)? != self.source_sha256
                    || source["plan_id"] != *from_job
                    || source["revision"].as_u64() != Some(*revision)
                    || source["planning"]["plan_sha256"] != *plan_sha256
                    || source["status"] != "awaiting_approval"
                    || source["goal"]["request_context"] != json!(self.owner_context)
                    || !source["approval"].is_null()
                    || !source["execution"].is_null()
                    || serde_json::to_vec(source)?.len() > 1024 * 1024
                {
                    bail!("Invalid automation source snapshot")
                }
            }
            (None, Some(receipt)) if self.revision == 2 && self.state == "removed" => {
                crate::job_commands::validate_operation_id(&receipt.operation_id)?;
                if receipt.source_revision != 1 || receipt.operation_id == self.id {
                    bail!("Invalid removal receipt")
                }
            }
            _ => bail!("Unsupported automation lifecycle; no timers or work started"),
        }
        Ok(())
    }
    pub fn summary(&self) -> Value {
        json!({"id":self.id,"revision":self.revision,"state":self.state,"owner_node":self.owner_context.origin_node(),
            "creation":self.creation,"definition_sha256":self.definition_sha256,
            "timers_started":false,"execution_authorized":false})
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub command: AutomationCommand,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationReport {
    pub schema_version: u32,
    pub report_type: String,
    pub command: AutomationCommand,
    pub definition: Option<Definition>,
    pub items: Vec<Value>,
    pub next_after: Option<String>,
    pub reused: bool,
    pub timers_started: bool,
    pub execution_authorized: bool,
    pub dispatched: bool,
    pub automatically_retried: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    id: String,
    revision: u64,
    state: String,
    owner_node: String,
    creation: AutomationCommand,
    definition_sha256: String,
    timers_started: bool,
    execution_authorized: bool,
}
fn confirmed_summary(value: &Value, owner: Option<&str>, after: Option<&str>) -> bool {
    let Ok(summary) = serde_json::from_value::<Summary>(value.clone()) else {
        return false;
    };
    summary.creation.validate().is_ok()
        && matches!(&summary.creation,AutomationCommand::Define {operation_id,..} if *operation_id==summary.id)
        && crate::network::validate_remote_node_id(&summary.owner_node).is_ok()
        && owner.is_none_or(|node| summary.owner_node == node)
        && after.is_none_or(|id| summary.id.as_str() > id)
        && crate::request_result::valid_digest(&summary.definition_sha256)
        && matches!(
            (summary.revision, summary.state.as_str()),
            (1, "disabled") | (2, "removed")
        )
        && !summary.timers_started
        && !summary.execution_authorized
}
pub fn confirmed(value: &Value, command: &AutomationCommand, owner: Option<&str>) -> bool {
    let Ok(r) = serde_json::from_value::<AutomationReport>(value.clone()) else {
        return false;
    };
    if command.validate().is_err()
        || r.schema_version != 1
        || r.report_type != "automation_definition"
        || r.command != *command
        || r.timers_started
        || r.execution_authorized
        || r.dispatched
        || r.automatically_retried
    {
        return false;
    }
    match command {
        AutomationCommand::List { limit, after } => {
            r.definition.is_none()
                && !r.reused
                && r.items.len() <= *limit as usize
                && r.items
                    .iter()
                    .all(|v| confirmed_summary(v, owner, after.as_deref()))
                && r.items
                    .windows(2)
                    .all(|v| v[0]["id"].as_str() < v[1]["id"].as_str())
                && r.next_after.as_deref().is_none_or(|id| {
                    r.items.len() == *limit as usize
                        && r.items.last().is_some_and(|v| v["id"] == id)
                })
        }
        _ => {
            r.items.is_empty()
                && r.next_after.is_none()
                && r.definition.as_ref().is_some_and(|d| {
                    d.validate().is_ok()
                        && owner.is_none_or(|o| d.owner_context.origin_node() == o)
                        && match command {
                            AutomationCommand::Show { id } => d.id == *id && !r.reused,
                            AutomationCommand::Define { operation_id, .. } => {
                                d.id == *operation_id
                                    && d.creation == *command
                                    && (r.reused || d.state == "disabled")
                            }
                            AutomationCommand::Remove {
                                id,
                                revision,
                                operation_id,
                            } => {
                                d.id == *id
                                    && d.removal.as_ref().is_some_and(|v| {
                                        v.operation_id == *operation_id
                                            && v.source_revision == *revision
                                    })
                            }
                            _ => false,
                        }
                })
        }
    }
}
impl crate::NodeService {
    pub async fn automation_request(&self, request: AutomationRequest) -> Result<Value> {
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Automation schema/node changed")
        }
        request.command.validate()?;
        crate::job_commands::validate_operation_id(&request.request_id)?;
        if request
            .command
            .receipt()
            .is_some_and(|id| id != request.request_id)
        {
            bail!("Automation receipt mismatch")
        }
        let mut report = json!({"schema_version":1,"report_type":"node_automation_definition","node_id":self.node_id,
            "command":request.command,"request_id":request.request_id,"success":false,"coordinator_acknowledged":false,
            "outcome_unknown":false,"timers_started":false,"execution_authorized":false,"dispatched":false,"automatically_retried":false});
        match self
            .submissions
            .request_correlated(
                "automations.command",
                json!(request.command),
                Some(request.request_id),
            )
            .await
        {
            Ok(value) if confirmed(&value, &request.command, Some(&self.node_id)) => {
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
                report["error_code"] = json!("automation_not_sent");
            }
            Err(error)
                if matches!(
                    error.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::Rejected(_))
                ) =>
            {
                report["error_code"] = json!("automation_rejected");
            }
            _ => {
                report["outcome_unknown"] = json!(true);
                report["error_code"] = json!("automation_unconfirmed");
            }
        }
        report["note"] = json!(match report["error_code"].as_str() {
            Some("automation_not_sent") =>
                "No submission was sent. Start/connect this node explicitly; this command does not start or reconnect a runtime.",
            Some("automation_rejected") =>
                "Coordinator refused the command. Check ownership, the exact unexecuted source revision/hash and job/automation history limits. No timer or execution was started.",
            Some(_) =>
                "No confirmed reply. Retain the operation ID and inspect automations show with the definition ID. Delivery may be unknown; there is no automatic retry, timer or execution.",
            None =>
                "Durable definition authoring only. Disabled definitions do not schedule work; bounded reusable approval, triggering and safe restart/overlap execution are not implemented yet.",
        });
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Definition, AutomationCommand) {
        let job = uuid::Uuid::new_v4().simple().to_string();
        let context = crate::context::RequestContext::new(
            uuid::Uuid::new_v4().to_string(),
            "owner-node".into(),
            vec!["Room".into()],
        )
        .unwrap();
        let command = AutomationCommand::Define {
            operation_id: uuid::Uuid::new_v4().to_string(),
            from_job: job.clone(),
            revision: 2,
            plan_sha256: "a".repeat(64),
            name: "Fixture".into(),
            every_ms: 1000,
            max_runs: 3,
            valid_for_ms: 10000,
        };
        let source = json!({"plan_id":job,"revision":2,"planning":{"plan_sha256":"a".repeat(64)},"status":"awaiting_approval",
            "goal":{"request_context":context}});
        let mut definition = Definition {
            schema_version: 1,
            id: command.receipt().unwrap().into(),
            owner_context: context,
            created_at_ms: 1,
            creation: command.clone(),
            source_sha256: crate::grants::definition_digest(&source).unwrap(),
            definition_sha256: String::new(),
            revision: 1,
            state: "disabled".into(),
            source: Some(source),
            removal: None,
        };
        definition.definition_sha256 = definition.digest().unwrap();
        (definition, command)
    }
    fn report(
        command: AutomationCommand,
        definition: Option<Definition>,
        items: Vec<Value>,
    ) -> Value {
        json!(AutomationReport {
            schema_version: 1,
            report_type: "automation_definition".into(),
            command,
            definition,
            items,
            next_after: None,
            reused: false,
            timers_started: false,
            execution_authorized: false,
            dispatched: false,
            automatically_retried: false
        })
    }
    #[test]
    fn timing_run_source_and_unknown_fields_are_bounded() {
        let (_, command) = fixture();
        assert!(command.validate().is_ok());
        for (key, value) in [
            ("every_ms", json!(0)),
            ("every_ms", json!(86400001)),
            ("max_runs", json!(0)),
            ("max_runs", json!(129)),
            ("valid_for_ms", json!(999)),
            ("valid_for_ms", json!(86400001)),
            ("revision", json!(0)),
            ("plan_sha256", json!("no")),
            ("name", json!("\n")),
            ("name", json!("x".repeat(129))),
            ("from_job", json!("bad id")),
        ] {
            let mut wrong = json!(command);
            wrong[key] = value;
            assert!(
                serde_json::from_value::<AutomationCommand>(wrong)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut unknown = json!(command);
        unknown["auto_approve"] = json!(true);
        assert!(serde_json::from_value::<AutomationCommand>(unknown).is_err());
    }
    #[test]
    fn full_reply_confirmation_refuses_substitution_lifecycle_and_authority_flags() {
        let (definition, command) = fixture();
        let value = report(command.clone(), Some(definition), vec![]);
        assert!(confirmed(&value, &command, Some("owner-node")));
        assert!(!confirmed(&value, &command, Some("other-node")));
        for (key, val) in [
            ("timers_started", json!(true)),
            ("dispatched", json!(true)),
            ("execution_authorized", json!(true)),
            ("automatically_retried", json!(true)),
            ("schema_version", json!(2)),
            ("future_field", json!(true)),
        ] {
            let mut wrong = value.clone();
            wrong[key] = val;
            assert!(!confirmed(&wrong, &command, Some("owner-node")));
        }
        for key in ["revision", "state", "source_sha256", "definition_sha256"] {
            let mut wrong = value.clone();
            wrong["definition"][key] = json!("changed");
            assert!(!confirmed(&wrong, &command, None));
        }
        let mut wrong = value;
        wrong["definition"]["source"]["goal"]["request_context"]["origin_node"] =
            json!("other-node");
        assert!(!confirmed(&wrong, &command, None));
    }
    #[test]
    fn pagination_confirms_only_strict_owned_ordered_summaries() {
        let (a, _) = fixture();
        let (b, _) = fixture();
        let mut items = vec![a.summary(), b.summary()];
        items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let command = AutomationCommand::List {
            limit: 2,
            after: None,
        };
        let good = report(command.clone(), None, items);
        assert!(confirmed(&good, &command, Some("owner-node")));
        for key in ["state", "owner_node", "definition_sha256"] {
            let mut wrong = good.clone();
            wrong["items"][0][key] = json!("wrong");
            assert!(!confirmed(&wrong, &command, Some("owner-node")));
        }
        let mut unknown = good.clone();
        unknown["items"][0]["extra_authority"] = json!(true);
        assert!(!confirmed(&unknown, &command, None));
        let mut duplicate = good.clone();
        duplicate["items"][1] = duplicate["items"][0].clone();
        assert!(!confirmed(&duplicate, &command, None));
        let mut reversed = good.clone();
        reversed["items"].as_array_mut().unwrap().reverse();
        assert!(!confirmed(&reversed, &command, None));
        let mut cursor = good;
        cursor["next_after"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(!confirmed(&cursor, &command, None));
    }
    #[test]
    fn removed_creation_reuse_is_explicit_and_cannot_claim_new_disabled_work() {
        let (mut definition, creation) = fixture();
        let removal = AutomationCommand::Remove {
            id: definition.id.clone(),
            revision: 1,
            operation_id: uuid::Uuid::new_v4().to_string(),
        };
        definition.source = None;
        definition.revision = 2;
        definition.state = "removed".into();
        definition.removal = Some(RemovalReceipt {
            operation_id: removal.receipt().unwrap().into(),
            source_revision: 1,
        });
        let mut value = report(creation.clone(), Some(definition.clone()), vec![]);
        assert!(!confirmed(&value, &creation, None));
        value["reused"] = json!(true);
        assert!(confirmed(&value, &creation, None));
        assert!(confirmed(
            &report(removal.clone(), Some(definition), vec![]),
            &removal,
            None
        ));
    }
}
