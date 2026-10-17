//! Explicit owner-request inference timing, not model output or execution time.
use std::time::Duration;

pub const DEFAULT_SECONDS: u64 = 20;
pub const MAX_SECONDS: u64 = 120;

#[derive(Clone, Debug, Default, clap::Args)]
pub struct InferenceOptions {
    /// Per model call, 1-120 seconds. Omit to keep the 20-second default.
    /// Does not retry, increase output limits, approve or execute anything.
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=MAX_SECONDS))]
    pub inference_timeout_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InferenceBudget(u64);
impl InferenceBudget {
    pub fn new(seconds: Option<u64>) -> Result<Self, &'static str> {
        let seconds = seconds.unwrap_or(DEFAULT_SECONDS);
        if !(1..=MAX_SECONDS).contains(&seconds) {
            return Err("invalid_inference_timeout");
        }
        Ok(Self(seconds))
    }
    pub fn duration(self) -> Duration {
        Duration::from_secs(self.0)
    }
    /// Preserve existing backend headroom; the outer inference deadline wins.
    pub fn backend_seconds(self) -> u64 {
        30.max(self.0 + 10)
    }
    /// Existing workflow/transport bounds already include the default inference
    /// allowance. Add only an explicitly larger allowance for each possible call.
    pub fn extend_seconds(self, existing: u64, calls: u64) -> u64 {
        existing.saturating_add(calls.saturating_mul(self.0.saturating_sub(DEFAULT_SECONDS)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_timing_is_finite_and_default_is_unchanged() {
        for value in [None, Some(1), Some(20), Some(120)] {
            let budget = InferenceBudget::new(value).unwrap();
            assert_eq!(budget.duration().as_secs(), value.unwrap_or(20));
            assert_eq!(budget.backend_seconds(), 30.max(value.unwrap_or(20) + 10));
        }
        for value in [0, 121, u64::MAX] {
            assert!(InferenceBudget::new(Some(value)).is_err());
        }
    }
    #[test]
    fn all_workflow_and_transport_margins_cover_the_explicit_allowance() {
        for (base, calls, maximum) in [
            (25, 1, 125),
            (150, 1, 250),
            (65, 1, 165),
            (70, 1, 170),
            (135, 2, 335),
            (150, 2, 350),
            (180, 2, 380),
            (195, 2, 395),
        ] {
            assert_eq!(
                InferenceBudget::new(None)
                    .unwrap()
                    .extend_seconds(base, calls),
                base
            );
            assert_eq!(
                InferenceBudget::new(Some(1))
                    .unwrap()
                    .extend_seconds(base, calls),
                base
            );
            assert_eq!(
                InferenceBudget::new(Some(120))
                    .unwrap()
                    .extend_seconds(base, calls),
                maximum
            );
        }
    }
    #[test]
    fn owner_cli_requires_an_explicit_bounded_numeric_value() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            options: InferenceOptions,
        }
        assert_eq!(
            Cli::try_parse_from(["test"])
                .unwrap()
                .options
                .inference_timeout_seconds,
            None
        );
        for value in ["1", "20", "120"] {
            assert!(Cli::try_parse_from(["test", "--inference-timeout-seconds", value]).is_ok());
        }
        for value in ["0", "121", "-1", "unlimited", "1.5"] {
            assert!(Cli::try_parse_from(["test", "--inference-timeout-seconds", value]).is_err());
        }
        assert!(Cli::try_parse_from(["test", "--inference-timeout-seconds"]).is_err());
    }
    #[test]
    fn omitted_request_timing_preserves_wire_shapes_and_rejects_noninteger_options() {
        use serde_json::json;
        let old = json!({"schema_version":1,"expected_node_id":"node","request_id":uuid::Uuid::new_v4().to_string(),"text":"Inspect status"});
        let request: crate::request_preview::PreviewRequest =
            serde_json::from_value(old.clone()).unwrap();
        assert_eq!(request.inference_timeout_seconds, None);
        assert_eq!(serde_json::to_value(request).unwrap(), old);
        let command = json!({"id":"00112233445566778899aabbccddeeff","expected_revision":1,"operation_id":uuid::Uuid::new_v4().to_string()});
        let mut envelope = json!({"schema_version":1,"expected_node_id":"node","command":command});
        assert!(
            serde_json::from_value::<crate::request_resolution::ResolveRequest>(envelope.clone())
                .unwrap()
                .inference_timeout_seconds
                .is_none()
        );
        assert!(
            serde_json::from_value::<crate::request_preparation::PrepareRequest>(envelope.clone())
                .unwrap()
                .inference_timeout_seconds
                .is_none()
        );
        envelope["command"]["text"] = json!("Answer");
        let response: crate::request_response::RespondRequest =
            serde_json::from_value(envelope.clone()).unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), envelope);
        for value in [json!(-1), json!(1.5), json!("120"), json!(true)] {
            let mut malformed = old.clone();
            malformed["inference_timeout_seconds"] = value;
            assert!(
                serde_json::from_value::<crate::request_preview::PreviewRequest>(malformed)
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn every_owner_workflow_refuses_invalid_timing_before_runtime_or_writes() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.json");
        let service = crate::NodeService::new(path.clone(), "node".into());
        for seconds in [0, 121, u64::MAX] {
            let preview = json!({"schema_version":1,"expected_node_id":"node","request_id":uuid::Uuid::new_v4().to_string(),
                "text":"Inspect status","inference_timeout_seconds":seconds});
            let reply = service
                .preview_request(serde_json::from_value(preview.clone()).unwrap())
                .await;
            assert_eq!(reply["error_code"], "invalid_inference_timeout");
            assert_eq!(reply["execution_authorized"], false);
            let reply = service
                .input_request(serde_json::from_value(preview).unwrap())
                .await;
            assert_eq!(reply["error_code"], "invalid_input_request");
            assert_eq!(reply["creation_attempted"], false);
            let mut body = json!({"schema_version":1,"expected_node_id":"node","inference_timeout_seconds":seconds,
                "command":{"id":"00112233445566778899aabbccddeeff","expected_revision":1,"operation_id":uuid::Uuid::new_v4().to_string()}});
            let reply = service
                .resolve_request(serde_json::from_value(body.clone()).unwrap())
                .await;
            assert_eq!(reply["error_code"], "invalid_inference_timeout");
            assert_eq!(reply["execution_authorized"], false);
            let reply = service
                .prepare_request(serde_json::from_value(body.clone()).unwrap())
                .await;
            assert_eq!(reply["error_code"], "invalid_inference_timeout");
            assert_eq!(reply["model_inference_requested"], false);
            body["command"]["text"] = json!("Answer");
            let reply = service
                .respond_request(serde_json::from_value(body).unwrap())
                .await;
            assert_eq!(reply["error_code"], "invalid_response_request");
            assert_eq!(reply["answer_attempted"], false);
            assert!(!path.exists());
        }
    }
}
