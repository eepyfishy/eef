//! Origin/owner read-only stop inspection through existing submission services.
use crate::grants::stop::inspection::Inspection;
pub use crate::request_receipt::{ReceiptCommand as Command, ReceiptRequest as Request};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
const NOTE: &str = "Read-only stop intent inspection. Coordinator tracking is unchanged; retained intent is not termination, rollback or proof no effects occurred. No stop signal, execution, retry, reconciliation or permission change was requested.";

fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorReport {
    pub schema_version: u32,
    pub report_type: String,
    pub success: bool,
    pub runtime_id: String,
    pub id: String,
    pub revision: u64,
    pub request_context: crate::context::RequestContext,
    pub coordinator_attempt_state: String,
    pub inspection: Inspection,
    pub read_only: bool,
    pub execution_requested: bool,
    pub stop_requested: bool,
    pub automatically_retried: bool,
    pub reconciled: bool,
    pub note: String,
}
pub fn confirmed(
    value: &Value,
    cmd: &Command,
    origin: Option<&str>,
    runtime: Option<&str>,
) -> bool {
    let Ok(r) = serde_json::from_value::<CoordinatorReport>(value.clone()) else {
        return false;
    };
    cmd.validate().is_ok()
        && r.inspection.validate().is_ok()
        && r.schema_version == 1
        && r.report_type == "coordinator_request_stop_receipt"
        && r.success
        && r.id == cmd.id
        && r.revision > 0
        && crate::automation_occurrence::uuid(&r.runtime_id).is_ok()
        && runtime.is_none_or(|id| id == r.runtime_id)
        && origin.is_none_or(|id| id == r.request_context.origin_node())
        && r.inspection
            .scope
            .as_ref()
            .is_some_and(|s| s.job_id == cmd.id && s.grant_id == cmd.grant_id)
        && matches!(
            r.coordinator_attempt_state.as_str(),
            "issued" | "accepted" | "completed" | "rejected" | "unknown"
        )
        && r.read_only
        && !r.execution_requested
        && !r.stop_requested
        && !r.automatically_retried
        && !r.reconciled
        && r.note == NOTE
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeReport {
    schema_version: u32,
    report_type: String,
    node_id: String,
    request_id: String,
    id: String,
    grant_id: String,
    success: bool,
    #[serde(deserialize_with = "required_option")]
    result: Option<Value>,
    #[serde(deserialize_with = "required_option")]
    error_code: Option<String>,
    read_only: bool,
    execution_requested: bool,
    stop_requested: bool,
    automatically_retried: bool,
    reconciled: bool,
    note: String,
}
pub fn confirmed_node(value: &Value, cmd: &Command, node: &str, request: &str) -> bool {
    let Ok(r) = serde_json::from_value::<NodeReport>(value.clone()) else {
        return false;
    };
    cmd.validate().is_ok()
        && crate::network::validate_remote_node_id(node).is_ok()
        && crate::automation_occurrence::uuid(request).is_ok()
        && r.schema_version == 1
        && r.report_type == "request_stop_receipt"
        && r.node_id == node
        && r.request_id == request
        && r.id == cmd.id
        && r.grant_id == cmd.grant_id
        && r.read_only
        && !r.execution_requested
        && !r.stop_requested
        && !r.automatically_retried
        && !r.reconciled
        && r.note == NOTE
        && if r.success {
            r.error_code.is_none()
                && r.result
                    .as_ref()
                    .is_some_and(|v| confirmed(v, cmd, Some(node), None))
        } else {
            r.result.is_none() && r.error_code.as_deref() == Some("stop_receipt_unavailable")
        }
}
pub fn unavailable(node: &str, request: &str, cmd: &Command) -> Value {
    json!(NodeReport {
        schema_version: 1,
        report_type: "request_stop_receipt".into(),
        node_id: node.into(),
        request_id: request.into(),
        id: cmd.id.clone(),
        grant_id: cmd.grant_id.clone(),
        success: false,
        result: None,
        error_code: Some("stop_receipt_unavailable".into()),
        read_only: true,
        execution_requested: false,
        stop_requested: false,
        automatically_retried: false,
        reconciled: false,
        note: NOTE.into()
    })
}
impl crate::NodeService {
    pub async fn request_stop_receipt(&self, request: Request) -> Result<Value> {
        request.command.validate()?;
        crate::automation_occurrence::uuid(&request.request_id)?;
        if request.schema_version != 1 || request.expected_node_id != self.node_id {
            bail!("Stop inspection targets another node")
        }
        let mut report = unavailable(&self.node_id, &request.request_id, &request.command);
        if let Ok(value) = self
            .submissions
            .request_correlated(
                "requests.stop_receipt",
                json!(request.command),
                Some(request.request_id.clone()),
            )
            .await
        {
            if confirmed(&value, &request.command, Some(&self.node_id), None) {
                report["success"] = json!(true);
                report["result"] = value;
                report["error_code"] = Value::Null;
            }
        }
        if !confirmed_node(
            &report,
            &request.command,
            &self.node_id,
            &request.request_id,
        ) {
            bail!("Stop inspection reply failed confirmation")
        }
        Ok(report)
    }
}
pub fn note() -> String {
    NOTE.into()
}
pub fn render(value: &Value) -> Result<String> {
    let r: NodeReport = serde_json::from_value(value.clone())?;
    let cmd = Command {
        id: r.id.clone(),
        grant_id: r.grant_id.clone(),
    };
    if !confirmed_node(value, &cmd, &r.node_id, &r.request_id) {
        bail!("Unconfirmed stop inspection report")
    }
    let summary = if let Some(result) = &r.result {
        let report: CoordinatorReport = serde_json::from_value(result.clone())?;
        if report.inspection.evidence.is_some() {
            format!(
                "Stop intent is retained on node {}.",
                report.inspection.node_id
            )
        } else {
            "No retained stop record was found; this does not prove no stop/work was accepted."
                .into()
        }
    } else {
        "Inspection unconfirmed; no fallback or new stop was sent.".into()
    };
    Ok(format!(
        "Stop receipt for job {} / grant {}\n{}\n{}\n",
        r.id, r.grant_id, summary, NOTE
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::stop::{Scope, inspection};

    fn fixture(retained: bool) -> (Command, String, Value) {
        let g = crate::grants::tests::grant();
        let scope = Scope::from_grant(&g).unwrap();
        let cmd = Command {
            id: g.job_id.clone(),
            grant_id: g.grant_id.clone(),
        };
        let runtime = uuid::Uuid::new_v4().to_string();
        let evidence = retained.then(|| inspection::Evidence {
            schema_version: 1,
            scope: scope.clone(),
            recorded_at_ms: 1,
            receipt_preexisted: true,
            persisted: true,
            stop_requested: true,
            execution_stopped: false,
            external_effects_stopped: false,
        });
        let report = json!(CoordinatorReport {
            schema_version: 1,
            report_type: "coordinator_request_stop_receipt".into(),
            success: true,
            runtime_id: runtime.clone(),
            id: cmd.id.clone(),
            revision: 5,
            request_context: g.request_context.clone(),
            coordinator_attempt_state: "unknown".into(),
            inspection: Inspection {
                schema_version: 1,
                report_type: "grant_stop_inspection".into(),
                node_id: g.executor_node.clone(),
                grant_id: g.grant_id.clone(),
                scope: Some(scope),
                availability: if retained {
                    inspection::Availability::Retained
                } else {
                    inspection::Availability::Missing
                },
                evidence,
                read_only: true,
                execution_requested: false,
                stop_requested: false,
                automatically_retried: false,
                reconciled: false,
                note: inspection::NOTE.into(),
            },
            read_only: true,
            execution_requested: false,
            stop_requested: false,
            automatically_retried: false,
            reconciled: false,
            note: NOTE.into(),
        });
        (cmd, runtime, report)
    }

    #[test]
    fn stop_inspection_reports_bind_origin_runtime_scope_and_no_action_contract() {
        for retained in [false, true] {
            let (cmd, runtime, report) = fixture(retained);
            assert!(confirmed(&report, &cmd, Some("owner"), Some(&runtime)));
            assert!(!confirmed(&report, &cmd, Some("other"), Some(&runtime)));
            assert!(!confirmed(
                &report,
                &cmd,
                None,
                Some(&uuid::Uuid::new_v4().to_string())
            ));
            for (pointer, bad) in [
                ("/read_only", json!(false)),
                ("/execution_requested", json!(true)),
                ("/stop_requested", json!(true)),
                ("/automatically_retried", json!(true)),
                ("/reconciled", json!(true)),
                ("/revision", json!(0)),
                ("/note", json!("Stopped safely")),
                ("/coordinator_attempt_state", json!("recovered")),
                ("/inspection/stop_requested", json!(true)),
                ("/inspection/scope/job_id", json!("0".repeat(32))),
                (
                    "/inspection/scope/grant_id",
                    json!(uuid::Uuid::new_v4().to_string()),
                ),
            ] {
                let mut bad_report = report.clone();
                *bad_report.pointer_mut(pointer).unwrap() = bad;
                assert!(!confirmed(&bad_report, &cmd, None, None), "{pointer}");
            }
        }
    }

    #[test]
    fn stop_inspection_node_reports_require_correlation_and_explicit_optional_fields() {
        let (cmd, _, report) = fixture(true);
        let request = uuid::Uuid::new_v4().to_string();
        let mut value = unavailable("owner", &request, &cmd);
        assert!(confirmed_node(&value, &cmd, "owner", &request));
        assert!(render(&value).unwrap().contains("unconfirmed"));
        value["success"] = json!(true);
        value["error_code"] = Value::Null;
        value["result"] = report;
        assert!(confirmed_node(&value, &cmd, "owner", &request));
        assert!(render(&value).unwrap().contains("intent is retained"));
        assert!(!confirmed_node(&value, &cmd, "other", &request));
        assert!(!confirmed_node(
            &value,
            &cmd,
            "owner",
            &uuid::Uuid::new_v4().to_string()
        ));
        for key in ["result", "error_code"] {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(!confirmed_node(&bad, &cmd, "owner", &request));
        }
        value["retry"] = json!(true);
        assert!(!confirmed_node(&value, &cmd, "owner", &request));
        let (_, _, missing) = fixture(false);
        value.as_object_mut().unwrap().remove("retry");
        value["id"] = missing["id"].clone();
        value["grant_id"] = missing["inspection"]["grant_id"].clone();
        value["result"] = missing;
        assert!(render(&value).unwrap().contains("does not prove"));
    }

    #[tokio::test]
    async fn stop_inspection_offline_service_returns_unconfirmed_without_creating_a_journal() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("node.json");
        let service = crate::NodeService::new(config.clone(), "owner".into());
        let (cmd, _, _) = fixture(false);
        let request = uuid::Uuid::new_v4().to_string();
        let result = service
            .request_stop_receipt(Request {
                schema_version: 1,
                expected_node_id: "owner".into(),
                request_id: request.clone(),
                command: cmd.clone(),
            })
            .await
            .unwrap();
        assert!(confirmed_node(&result, &cmd, "owner", &request));
        assert_eq!(result["success"], false);
        assert_eq!(result["error_code"], "stop_receipt_unavailable");
        assert!(!config.exists());
        assert!(!crate::grants::journal_path(&config).exists());
    }
}
