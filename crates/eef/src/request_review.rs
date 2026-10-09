//! Stateless review of node-authored hints. Never plans or dispatches work.
use anyhow::{Result, bail};
use eefn::{context::RequestContext, interpretation::Proposal};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSubmission {
    schema_version: u32,
    original_text: String,
    proposal: Proposal,
}

/// Context and capability names must come from the authenticated gateway's
/// registration snapshot, never fields supplied in this payload. Advertised
/// capabilities constrain hints; they do not establish execution permission.
pub fn review(payload: Value, context: &RequestContext, capabilities: &[String]) -> Result<Value> {
    if serde_json::to_vec(&payload)?.len() > 64 * 1024 {
        bail!("review submission exceeds bounds");
    }
    let request: ReviewSubmission = serde_json::from_value(payload)?;
    if request.schema_version != 1 {
        bail!("unsupported review schema version");
    }
    let interpretation = eefn::interpretation::validate(
        &request.original_text,
        &serde_json::to_vec(&request.proposal)?,
        capabilities,
    )?;
    Ok(
        json!({"schema_version":1,"report_type":"coordinator_request_review",
        "request_context":context,"interpretation":interpretation,
        "execution_authorized":false,"dispatched":false,"persisted":false,
        "note":"Proposal validated for review only. Clarification and approval remain user decisions; no plan, job or execution grant was created."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> Value {
        json!({"schema_version":1,"original_text":"  Check this node\n",
            "proposal":{"schema_version":1,"intent":"information","goal":"Check status",
                "context_hints":["this node"],"constraints":[],"suggested_capabilities":["system.info"],
                "complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null}})
    }
    fn context() -> RequestContext {
        RequestContext::new("receipt".into(), "owner".into(), vec!["Bedroom".into()]).unwrap()
    }
    #[test]
    fn review_preserves_original_and_authenticated_context_without_authorizing() {
        let report = review(payload(), &context(), &["system.info".into()]).unwrap();
        assert_eq!(
            report["interpretation"]["original_text"],
            "  Check this node\n"
        );
        assert_eq!(report["request_context"], json!(context()));
        assert_eq!(report["interpretation"]["review_state"], "review_required");
        for key in ["execution_authorized", "dispatched", "persisted"] {
            assert_eq!(report[key], false);
        }
        assert_eq!(report["interpretation"]["execution_authorized"], false);
    }
    #[test]
    fn review_revalidates_schema_bounds_and_registered_capability_hints() {
        assert!(review(payload(), &context(), &[]).is_err());
        for key in [
            "request_context",
            "origin_node",
            "permissions",
            "execution_authorized",
            "command",
        ] {
            let mut value = payload();
            value[key] = json!("forged");
            assert!(
                review(value, &context(), &["system.info".into()]).is_err(),
                "{key}"
            );
            let mut value = payload();
            value["proposal"][key] = json!("forged");
            assert!(
                review(value, &context(), &["system.info".into()]).is_err(),
                "{key}"
            );
        }
        for value in [json!(0), json!(2)] {
            let mut input = payload();
            input["schema_version"] = value;
            assert!(review(input, &context(), &["system.info".into()]).is_err());
        }
        let mut input = payload();
        input["original_text"] = json!("x".repeat(32769));
        assert!(review(input, &context(), &["system.info".into()]).is_err());
        let mut input = payload();
        input["proposal"]["goal"] = json!("x".repeat(2049));
        assert!(review(input, &context(), &["system.info".into()]).is_err());
    }
    #[test]
    fn clarification_is_returned_without_guessing_an_answer_or_creating_work() {
        let mut input = payload();
        input["proposal"]["needs_clarification"] = json!(true);
        input["proposal"]["clarification"] = json!("Which node?");
        let report = review(input, &context(), &["system.info".into()]).unwrap();
        assert_eq!(
            report["interpretation"]["review_state"],
            "clarification_required"
        );
        assert_eq!(
            report["interpretation"]["proposal"]["clarification"],
            "Which node?"
        );
        assert_eq!(report["dispatched"], false);
    }
}
