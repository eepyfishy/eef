//! Close coordinator tracking explicitly; never claim remote stop/success or authorize replay.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbandonActor {
    CoordinatorOwner,
    OriginNode,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonCommand {
    pub id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub grant_id: String,
    pub acknowledge_effects_may_continue: bool,
}
impl AbandonCommand {
    pub fn validate(&self) -> Result<()> {
        crate::request_drafts::validate_id(&self.id)?;
        crate::job_commands::validate_operation_id(&self.operation_id)?;
        crate::job_commands::validate_operation_id(&self.grant_id)?;
        if uuid::Uuid::parse_str(&self.operation_id)?.is_nil()
            || uuid::Uuid::parse_str(&self.grant_id)?.is_nil()
            || self.operation_id == self.grant_id
            || self.expected_revision == 0
            || self.expected_revision == u64::MAX
            || !self.acknowledge_effects_may_continue
        {
            bail!("Exact uncertainty acknowledgement and receipt identities required")
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonRecord {
    pub schema_version: u32,
    pub command: AbandonCommand,
    pub actor: AbandonActor,
    pub decided_at_ms: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonRequest {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub command: AbandonCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub status: String,
    pub request_context: crate::context::RequestContext,
    pub abandonment: AbandonRecord,
    pub reused: bool,
    pub persisted: bool,
    pub tracking_abandoned: bool,
    pub remote_stop_confirmed: bool,
    pub remote_effects_unknown: bool,
    pub execution_requested: bool,
    pub automatically_retried: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    command: &AbandonCommand,
    actor: AbandonActor,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<AbandonReport>(value.clone()) else {
        return false;
    };
    command.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_abandonment"
        && r.success
        && r.id == command.id
        && r.status == "abandoned"
        && r.revision == command.expected_revision + 1
        && r.abandonment.schema_version == 1
        && r.abandonment.command == *command
        && r.abandonment.actor == actor
        && r.abandonment.decided_at_ms > 0
        && r.note.len() <= 2048
        && crate::job_commands::validate_operation_id(&r.runtime_id).is_ok()
        && runtime.is_none_or(|s| s == r.runtime_id)
        && origin.is_none_or(|s| s == r.request_context.origin_node())
        && r.persisted
        && r.tracking_abandoned
        && !r.remote_stop_confirmed
        && r.remote_effects_unknown
        && !r.execution_requested
        && !r.automatically_retried
}
impl crate::NodeService {
    pub async fn abandon_request_command(&self, req: AbandonRequest) -> Result<Value> {
        if req.schema_version != 1 || req.expected_node_id != self.node_id {
            bail!("Abandonment schema/node changed")
        }
        req.command.validate()?;
        let mut report = json!({"schema_version":1,"report_type":"request_abandonment","success":false,"node_id":self.node_id,
            "id":req.command.id,"operation_id":req.command.operation_id,"coordinator_acknowledged":false,"outcome_unknown":false,
            "execution_requested":false,"automatically_retried":false,"remote_stop_confirmed":false,"remote_effects_unknown":true});
        let result = self
            .submissions
            .request_correlated(
                "requests.abandon",
                json!(req.command),
                Some(req.command.operation_id.clone()),
            )
            .await;
        match result {
            Ok(data)
                if confirmed(
                    &data,
                    &req.command,
                    AbandonActor::OriginNode,
                    Some(&self.node_id),
                    None,
                ) =>
            {
                report["success"] = json!(true);
                report["coordinator_acknowledged"] = json!(true);
                report["result"] = data;
            }
            Err(e) => {
                report["error_code"] = json!("abandonment_unconfirmed");
                report["outcome_unknown"] = json!(!matches!(
                    e.downcast_ref::<crate::submission::SubmissionFailure>(),
                    Some(crate::submission::SubmissionFailure::NotSent(_))
                ));
            }
            _ => {
                report["error_code"] = json!("abandonment_unconfirmed");
                report["outcome_unknown"] = json!(true);
            }
        }
        report["note"] = json!(
            "One submission only. Inspect the retained request and reuse only this exact receipt after uncertainty. Closing tracking is not remote stop, success, lost-output recovery or retry authorization."
        );
        Ok(report)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abandonment_contract_requires_acknowledgement_and_never_confirms_remote_stop_success_or_retry()
     {
        let command = AbandonCommand {
            id: uuid::Uuid::new_v4().simple().to_string(),
            expected_revision: 6,
            operation_id: uuid::Uuid::new_v4().to_string(),
            grant_id: uuid::Uuid::new_v4().to_string(),
            acknowledge_effects_may_continue: true,
        };
        command.validate().unwrap();
        let runtime = uuid::Uuid::new_v4().to_string();
        let report = json!(AbandonReport {
            schema_version: 1,
            report_type: "coordinator_request_abandonment".into(),
            success: true,
            runtime_id: runtime.clone(),
            id: command.id.clone(),
            revision: 7,
            status: "abandoned".into(),
            request_context: crate::context::RequestContext::new(
                uuid::Uuid::new_v4().to_string(),
                "owner".into(),
                vec![]
            )
            .unwrap(),
            abandonment: AbandonRecord {
                schema_version: 1,
                command: command.clone(),
                actor: AbandonActor::OriginNode,
                decided_at_ms: crate::protocol::now_ms()
            },
            reused: false,
            persisted: true,
            tracking_abandoned: true,
            remote_stop_confirmed: false,
            remote_effects_unknown: true,
            execution_requested: false,
            automatically_retried: false,
            note: "Remote effects remain unknown".into()
        });
        assert!(confirmed(
            &report,
            &command,
            AbandonActor::OriginNode,
            Some("owner"),
            Some(&runtime)
        ));
        assert!(!confirmed(
            &report,
            &command,
            AbandonActor::CoordinatorOwner,
            None,
            None
        ));
        assert!(!confirmed(
            &report,
            &command,
            AbandonActor::OriginNode,
            Some("other"),
            None
        ));
        for (field, bad) in [
            ("execution_requested", json!(true)),
            ("remote_stop_confirmed", json!(true)),
            ("remote_effects_unknown", json!(false)),
            ("automatically_retried", json!(true)),
            ("persisted", json!(false)),
            ("tracking_abandoned", json!(false)),
            ("revision", json!(8)),
            ("status", json!("cancelled")),
        ] {
            let mut wrong = report.clone();
            wrong[field] = bad;
            assert!(!confirmed(
                &wrong,
                &command,
                AbandonActor::OriginNode,
                None,
                None
            ));
        }
        let mut extra = report;
        extra["retry_authorized"] = json!(true);
        assert!(!confirmed(
            &extra,
            &command,
            AbandonActor::OriginNode,
            None,
            None
        ));
        let mut wrong = command.clone();
        wrong.acknowledge_effects_may_continue = false;
        assert!(wrong.validate().is_err());
        let mut wrong = command.clone();
        wrong.operation_id = wrong.grant_id.clone();
        assert!(wrong.validate().is_err());
        let mut extra = json!(command);
        extra["effects_stopped"] = json!(true);
        assert!(serde_json::from_value::<AbandonCommand>(extra).is_err());
    }
}
