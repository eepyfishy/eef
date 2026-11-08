//! Raw-first stages of normal input, sharing existing journal and model services.
use super::*;

impl NodeService {
    pub(super) async fn inspect_raw_input(
        &self,
        request: &PreviewRequest,
        id: &str,
        report: &mut Value,
    ) -> std::result::Result<(), &'static str> {
        let inspected = self
            .job_command(crate::job_commands::JobRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                operation_id: None,
                command: crate::job_commands::JobCommand::Get { id: id.into() },
            })
            .await
            .map_err(|_| "input_inspection_unconfirmed")?;
        if !confirms_raw_history(
            &inspected,
            &self.node_id,
            &request.request_id,
            &request.text,
        ) {
            return Err("input_receipt_conflict");
        }
        report["result"] = inspected["data"].clone();
        report["raw_history"] = inspected;
        report["source_confirmed"] = json!(true);
        report["opening_reused"] = json!(true);
        report["status"] = json!("historical_snapshot");
        report["success"] = json!(true);
        Ok(())
    }

    pub(super) async fn open_raw_input(
        &self,
        request: &PreviewRequest,
        report: &mut Value,
        admission: &tokio::sync::OwnedSemaphorePermit,
    ) -> std::result::Result<Option<Value>, &'static str> {
        let runtime = report["runtime_id"]
            .as_str()
            .ok_or("runtime_changed")?
            .to_owned();
        let save_request = save_request(request, &runtime);
        report["creation_attempted"] = json!(true);
        let saved = self
            .save_input(save_request.clone())
            .await
            .map_err(|_| "input_save_unconfirmed")?;
        report["raw_save"] = saved.clone();
        report["outcome_unknown"] = saved["outcome_unknown"].clone();
        if !crate::request_intent::confirmed(&saved, &save_request) || saved["success"] != true {
            return Err("input_save_unconfirmed");
        }
        report["source_confirmed"] = json!(true);
        report["result"] = saved["result"].clone();
        if saved["result"]["reused"] == true {
            // A race with another sender cannot license model inference.
            report["opening_reused"] = json!(true);
            report["status"] = json!("historical_snapshot");
            report["success"] = json!(true);
            return Ok(None);
        }
        let id = saved["result"]["id"]
            .as_str()
            .ok_or("input_save_unconfirmed")?;
        let interpretation_request = interpretation_request(request, &runtime, id);
        report["status"] = json!("interpretation_not_confirmed");
        // Conservative until an exact stage reply replaces this uncertainty.
        report["outcome_unknown"] = json!(true);
        let interpreted = self
            .interpret_saved_input_admitted(interpretation_request.clone(), admission)
            .await
            .map_err(|_| "input_interpretation_unconfirmed")?;
        report["interpretation"] = interpreted.clone();
        report["outcome_unknown"] = interpreted["outcome_unknown"].clone();
        if !crate::saved_input::confirmed(&interpreted, &interpretation_request) {
            return Err("input_interpretation_unconfirmed");
        }
        if interpreted["success"] != true {
            let code = match interpreted["error_code"].as_str() {
                Some("interpreter_unavailable") => "interpreter_unavailable",
                Some("interpreter_ambiguous") => "interpreter_ambiguous",
                _ => "input_interpretation_unconfirmed",
            };
            if code == "interpreter_unavailable" {
                report["setup_hint"] = json!(format!(
                    "Input {id} is saved. Select exactly one request_interpreter model, restart this node to apply it, then use `eefn requests interpret-saved {id}`. No automatic retry is requested."
                ));
            }
            return Err(code);
        }
        if interpreted["historical_snapshot"] != false {
            // Another attempt owns this input. Do not silently prepare its result.
            return Err("input_interpretation_reused");
        }
        let opened = self
            .draft_command(crate::request_drafts::DraftRequest {
                schema_version: 1,
                expected_node_id: self.node_id.clone(),
                request_id: request.request_id.clone(),
                command: crate::request_drafts::DraftCommand::Get { id: id.into() },
            })
            .await
            .map_err(|_| "input_inspection_unconfirmed")?;
        if !confirms_opening(
            &opened,
            &interpreted,
            &saved,
            &self.node_id,
            &request.request_id,
            &request.text,
        ) {
            return Err("input_source_changed");
        }
        // A real scoped draft-get reply, never a fabricated opening/proposal.
        report["opening"] = opened.clone();
        Ok(Some(opened))
    }
}

fn save_request(request: &PreviewRequest, runtime: &str) -> crate::request_intent::Request {
    crate::request_intent::Request {
        schema_version: 1,
        expected_node_id: request.expected_node_id.clone(),
        expected_runtime_id: runtime.into(),
        command: crate::request_intent::Command {
            operation_id: request.request_id.clone(),
            original_text: request.text.clone(),
        },
    }
}
fn interpretation_request(
    request: &PreviewRequest,
    runtime: &str,
    id: &str,
) -> crate::request_interpretation::Request {
    crate::request_interpretation::Request {
        schema_version: 1,
        expected_node_id: request.expected_node_id.clone(),
        expected_runtime_id: runtime.into(),
        command: crate::request_interpretation::Command {
            id: id.into(),
            operation_id: request.request_id.clone(),
            inference_timeout_seconds: request.inference_timeout_seconds,
        },
    }
}
fn confirms_raw_history(value: &Value, node: &str, input: &str, text: &str) -> bool {
    value["schema_version"] == 1
        && value["report_type"] == "node_job_command"
        && value["success"] == true
        && value["acknowledged"] == true
        && value["outcome_unknown"] == false
        && value["node_id"] == node
        && crate::saved_input::Source::read(
            &value["data"],
            value["data"]["id"].as_str().unwrap_or(""),
            node,
        )
        .is_ok_and(|s| s.confirms_input(input, text))
}
fn confirms_opening(
    opened: &Value,
    interpreted: &Value,
    saved: &Value,
    node: &str,
    input: &str,
    text: &str,
) -> bool {
    let checkpoint = &interpreted["checkpoint"];
    opened["success"] == true
        && opened["coordinator_acknowledged"] == true
        && opened["schema_version"] == 1
        && opened["report_type"] == "request_draft"
        && opened["outcome_unknown"] == false
        && opened["node_id"] == node
        && opened["request_id"] == input
        && opened["execution_authorized"] == false
        && opened["dispatched"] == false
        && opened["automatically_retried"] == false
        && crate::request_drafts::confirmed_report(
            &opened["result"],
            node,
            input,
            saved["result"]["id"].as_str(),
        )
        && opened["result"]["request_context"] == saved["result"]["request_context"]
        && opened["result"]["revision"] == checkpoint["revision"]
        && opened["result"]["status"] == checkpoint["status"]
        && opened["result"]["draft"] == checkpoint["draft"]
        && opened["result"]["draft"]["original_text"] == text
        && validate_source(
            &opened["result"],
            &PreviewRequest {
                schema_version: 1,
                expected_node_id: node.into(),
                request_id: input.into(),
                text: text.into(),
                inference_timeout_seconds: None,
            },
        )
        .is_ok()
}

pub(super) fn confirmed_raw(r: &InputReport, node: &str, input: &str, text: &str) -> bool {
    let Some(runtime) = r.runtime_id.as_deref() else {
        return false;
    };
    if crate::job_commands::validate_operation_id(runtime).is_err() || r.policy.is_none() {
        return false;
    }
    let request = PreviewRequest {
        schema_version: 1,
        expected_node_id: node.into(),
        request_id: input.into(),
        text: text.into(),
        inference_timeout_seconds: None,
    };
    if let Some(history) = &r.raw_history {
        return r.raw_save.is_none()
            && r.interpretation.is_none()
            && r.opening.is_none()
            && r.inspection.is_none()
            && r.preparation.is_none()
            && r.preparation_command.is_none()
            && !r.creation_attempted
            && r.opening_reused
            && r.success
            && r.source_confirmed
            && r.coordinator_acknowledged
            && !r.outcome_unknown
            && r.error_code.is_none()
            && r.status == "historical_snapshot"
            && confirms_raw_history(history, node, input, text)
            && r.result.as_ref() == Some(&history["data"]);
    }
    let Some(saved) = &r.raw_save else {
        return false;
    };
    if !r.creation_attempted
        || r.inspection.is_some()
        || !crate::request_intent::confirmed(saved, &save_request(&request, runtime))
    {
        return false;
    }
    if saved["success"] != true {
        return !r.success
            && !r.source_confirmed
            && !r.coordinator_acknowledged
            && r.error_code.is_some()
            && r.result.is_none()
            && r.interpretation.is_none()
            && r.opening.is_none()
            && r.preparation.is_none()
            && r.preparation_command.is_none()
            && r.outcome_unknown == (saved["outcome_unknown"] == true);
    }
    if !r.source_confirmed {
        return false;
    }
    let id = saved["result"]["id"].as_str().unwrap_or("");
    if r.opening_reused {
        return saved["result"]["reused"] == true
            && r.success
            && r.coordinator_acknowledged
            && !r.outcome_unknown
            && r.error_code.is_none()
            && r.status == "historical_snapshot"
            && r.interpretation.is_none()
            && r.opening.is_none()
            && r.preparation.is_none()
            && r.preparation_command.is_none()
            && r.result.as_ref() == Some(&saved["result"]);
    }
    if saved["result"]["reused"] != false {
        return false;
    }
    if let Some(interpreted) = &r.interpretation {
        let ir = interpretation_request(&request, runtime, id);
        if !crate::saved_input::confirmed(interpreted, &ir)
            || interpreted["source"].is_object()
                && (interpreted["source"]["source_sha256"] != saved["result"]["source_sha256"]
                    || interpreted["source"]["request_context"]
                        != saved["result"]["request_context"]
                    || interpreted["source"]["input"] != saved["result"]["input"])
        {
            return false;
        }
        if interpreted["model_inference_requested"] == true
            && (interpreted["source"]["revision"] != saved["result"]["revision"]
                || !interpreted["source"]["record"].is_null()
                || interpreted["checkpoint"]["record"]["claim"]["expected_revision"]
                    != saved["result"]["revision"])
        {
            return false;
        }
    }
    let Some(opened) = &r.opening else {
        return !r.success
            && !r.coordinator_acknowledged
            && r.error_code.is_some()
            && r.result.as_ref() == Some(&saved["result"])
            && r.preparation.is_none()
            && r.preparation_command.is_none();
    };
    let Some(interpreted) = &r.interpretation else {
        return false;
    };
    if interpreted["success"] != true
        || interpreted["historical_snapshot"] != false
        || !confirms_opening(opened, interpreted, saved, node, input, text)
    {
        return false;
    }
    if !r.success {
        return !r.coordinator_acknowledged
            && r.error_code.is_some()
            && r.result.as_ref() == Some(&opened["result"]);
    }
    if !r.coordinator_acknowledged || r.outcome_unknown || r.error_code.is_some() {
        return false;
    }
    let Ok(draft) = serde_json::from_value::<crate::request_drafts::DraftRecord>(
        opened["result"]["draft"].clone(),
    ) else {
        return false;
    };
    if let (Some(command), Some(prepared)) = (&r.preparation_command, &r.preparation) {
        r.policy.unwrap().prepare_on_input
            && crate::request_preparation::eligible(&draft).is_ok()
            && command.id == id
            && Some(command.expected_revision) == opened["result"]["revision"].as_u64()
            && preparation_receipt(node, input, id)
                .is_ok_and(|receipt| receipt == command.operation_id)
            && crate::request_preparation::confirmed(prepared, node, command)
            && confirms_preparation_source(prepared, &opened["result"], command)
            && prepared["runtime_id"] == runtime
            && r.result.as_ref() == Some(&prepared["result"])
            && r.status
                == if prepared["result"]["status"] == "awaiting_approval" {
                    "awaiting_approval"
                } else {
                    "clarification_required"
                }
    } else {
        r.preparation_command.is_none()
            && r.preparation.is_none()
            && r.result.as_ref() == Some(&opened["result"])
            && if draft.current().needs_clarification {
                r.status == "clarification_required"
            } else {
                r.status == "review_only"
                    && (!r.policy.unwrap().prepare_on_input
                        || crate::request_preparation::eligible(&draft).is_err())
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn request() -> PreviewRequest {
        PreviewRequest {
            schema_version: 1,
            expected_node_id: "raw-first-node".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            text: "  Keep my input\n".into(),
            inference_timeout_seconds: None,
        }
    }
    fn saved(request: &PreviewRequest) -> Value {
        let context = crate::context::RequestContext::new(
            request.request_id.clone(),
            request.expected_node_id.clone(),
            vec!["home".into()],
        )
        .unwrap();
        let input = crate::request_intent::Input {
            schema_version: 1,
            original_text: request.text.clone(),
        };
        json!(crate::request_intent::Saved {
            schema_version: 1,
            report_type: "coordinator_request_intent".into(),
            success: true,
            id: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            status: "review".into(),
            operation_id: request.request_id.clone(),
            source_sha256: input.digest(&context).unwrap(),
            request_context: context,
            input,
            reused: false,
            persisted: true,
            interpretation_requested: false,
            execution_authorized: false,
            dispatched: false,
        })
    }
    fn raw_view(saved: &Value) -> Value {
        json!({"id":saved["id"],"revision":saved["revision"],"status":"review",
            "request_context":saved["request_context"],"input_intent":saved["input"],
            "review_only":true,"task_count":0,"proposal_count":0,"planning_state":"not_interpreted","can_resume":false})
    }

    #[tokio::test]
    async fn normal_input_saves_before_model_selection_and_missing_role_retains_exact_source() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "raw-first-node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let mut receiver = service.submissions.connect();
        let request = request();
        let source = saved(&request);
        let expected = source.clone();
        let local = service.clone();
        let op = request.request_id.clone();
        let text = request.text.clone();
        let pending = tokio::spawn(async move { local.input_request(request).await });
        let lookup = receiver.recv().await.unwrap();
        assert_eq!(lookup.kind, "jobs.find");
        lookup
            .reply
            .send(Ok(
                json!({"operation_id":op,"total":0,"truncated":false,"jobs":[]}),
            ))
            .unwrap();
        let save = receiver.recv().await.unwrap();
        assert_eq!(save.kind, "requests.intent");
        assert_eq!(save.payload, source["input"]);
        save.reply.send(Ok(source.clone())).unwrap();
        let get = receiver.recv().await.unwrap();
        assert_eq!(get.kind, "jobs.get");
        get.reply.send(Ok(raw_view(&source))).unwrap();
        let report = pending.await.unwrap();
        assert!(confirmed(&report, "raw-first-node", &op, &text), "{report}");
        assert_eq!(report["success"], false);
        assert_eq!(report["error_code"], "interpreter_unavailable");
        assert_eq!(report["source_confirmed"], true);
        assert_eq!(report["outcome_unknown"], false);
        assert_eq!(report["result"], expected);
        assert_eq!(report["interpretation"]["claim_attempted"], false);
        assert_eq!(report["interpretation"]["model_inference_requested"], false);
        assert!(input_reply(&report).contains(source["id"].as_str().unwrap()));
        assert!(receiver.try_recv().is_err());
        assert!(!dir.path().join("config.json").exists());
        for (pointer, value) in [
            ("/raw_save/result/source_sha256", json!("0".repeat(64))),
            (
                "/interpretation/source/input/original_text",
                json!("changed"),
            ),
            (
                "/result/id",
                json!(uuid::Uuid::new_v4().simple().to_string()),
            ),
            ("/source_confirmed", json!(false)),
            ("/success", json!(true)),
        ] {
            let mut bad = report.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                !confirmed(&bad, "raw-first-node", &op, &text),
                "accepted {pointer}"
            );
        }
    }

    #[tokio::test]
    async fn refused_raw_save_never_starts_interpretation_or_fabricates_a_saved_request() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "raw-first-node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
        let mut receiver = service.submissions.connect();
        let request = request();
        let op = request.request_id.clone();
        let local = service.clone();
        let pending = tokio::spawn(async move { local.input_request(request).await });
        receiver
            .recv()
            .await
            .unwrap()
            .reply
            .send(Ok(
                json!({"operation_id":op,"total":0,"truncated":false,"jobs":[]}),
            ))
            .unwrap();
        let save = receiver.recv().await.unwrap();
        assert_eq!(save.kind, "requests.intent");
        save.reply
            .send(Err(crate::submission::SubmissionFailure::Rejected(
                "PRIVATE_REJECTION".into(),
            )
            .into()))
            .unwrap();
        let report = pending.await.unwrap();
        assert_eq!(report["error_code"], "input_save_unconfirmed");
        assert_eq!(report["source_confirmed"], false);
        assert_eq!(report["outcome_unknown"], false);
        assert_eq!(report["result"], Value::Null);
        assert_eq!(report["interpretation"], Value::Null);
        assert!(!report.to_string().contains("PRIVATE_REJECTION"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn repeated_raw_input_is_read_only_and_conflicting_text_cannot_trigger_inference() {
        for changed in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let service = NodeService::new(dir.path().join("config.json"), "raw-first-node".into());
            let engine =
                Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
            let _guard = service.attach_preview_runtime(&engine, uuid::Uuid::new_v4().to_string());
            let mut receiver = service.submissions.connect();
            let mut request = request();
            let source = saved(&request);
            let op = request.request_id.clone();
            if changed {
                request.text.push('!');
            }
            let text = request.text.clone();
            let local = service.clone();
            let pending = tokio::spawn(async move { local.input_request(request).await });
            receiver.recv().await.unwrap().reply.send(Ok(json!({"operation_id":op,"total":1,"truncated":false,"jobs":[{"id":source["id"],"planning_state":"not_interpreted"}]}))).unwrap();
            let get = receiver.recv().await.unwrap();
            assert_eq!(get.kind, "jobs.get");
            get.reply.send(Ok(raw_view(&source))).unwrap();
            let report = pending.await.unwrap();
            assert!(confirmed(&report, "raw-first-node", &op, &text), "{report}");
            assert_eq!(report["success"], !changed);
            assert_eq!(report["creation_attempted"], false);
            if changed {
                assert_eq!(report["error_code"], "input_receipt_conflict");
            } else {
                assert_eq!(report["status"], "historical_snapshot");
                assert_eq!(report["result"], raw_view(&source));
            }
            assert!(receiver.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn same_id_runtime_replacement_during_save_does_not_interpret_or_retry() {
        let dir = tempfile::tempdir().unwrap();
        let service = NodeService::new(dir.path().join("config.json"), "raw-first-node".into());
        let engine = Arc::new(crate::NodeEngine::new(false, vec![], dir.path().into()).unwrap());
        let runtime = uuid::Uuid::new_v4().to_string();
        let guard = service.attach_preview_runtime(&engine, runtime.clone());
        let mut receiver = service.submissions.connect();
        let request = request();
        let source = saved(&request);
        let op = request.request_id.clone();
        let local = service.clone();
        let pending = tokio::spawn(async move { local.input_request(request).await });
        receiver
            .recv()
            .await
            .unwrap()
            .reply
            .send(Ok(
                json!({"operation_id":op,"total":0,"truncated":false,"jobs":[]}),
            ))
            .unwrap();
        let save = receiver.recv().await.unwrap();
        assert_eq!(save.kind, "requests.intent");
        let _replacement = service.attach_preview_runtime(&engine, runtime);
        drop(guard);
        let _ = save.reply.send(Ok(source));
        let report = pending.await.unwrap();
        assert_eq!(report["success"], false);
        assert_eq!(report["outcome_unknown"], true);
        assert_eq!(report["error_code"], "runtime_changed");
        assert!(receiver.try_recv().is_err());
    }
}
