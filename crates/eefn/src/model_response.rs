//! Bounded chat response normalization shared by the existing inference adapters.
use crate::model_selection::Backend;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// Static observations only: never provider text, token counts or reasoning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IncompleteReason {
    ReasoningOnlyOutputLimit,
    OutputLimit,
    ToolCall,
    ContentFilter,
    NotFinished,
    UnconfirmedFinish,
}
impl IncompleteReason {
    pub(crate) fn engine_code(self) -> &'static str {
        match self {
            Self::ReasoningOnlyOutputLimit => "interpretation_incomplete_reasoning_limit",
            Self::OutputLimit => "interpretation_incomplete_output_limit",
            Self::ToolCall => "interpretation_incomplete_tool_call",
            Self::ContentFilter => "interpretation_incomplete_content_filter",
            Self::NotFinished => "interpretation_incomplete_not_finished",
            Self::UnconfirmedFinish => "interpretation_incomplete_unconfirmed_finish",
        }
    }
    pub(crate) fn from_engine_code(code: &str) -> Option<Self> {
        [
            Self::ReasoningOnlyOutputLimit,
            Self::OutputLimit,
            Self::ToolCall,
            Self::ContentFilter,
            Self::NotFinished,
            Self::UnconfirmedFinish,
        ]
        .into_iter()
        .find(|reason| reason.engine_code() == code)
    }
    pub(crate) fn explanation(self) -> &'static str {
        match self {
            Self::ReasoningOnlyOutputLimit => {
                "The backend exhausted its output allowance on reasoning without a final answer. A thinking request hint is not proof it was honored. Choose an appropriate installed interpreter; reasoning was not used as an answer."
            }
            Self::OutputLimit => {
                "The backend reported its output limit before a complete final answer. Partial output was not repaired or used as a proposal."
            }
            Self::ToolCall => {
                "The backend requested a tool/function call instead of completing an interpretation. No tool was called."
            }
            Self::ContentFilter => {
                "The backend reported a filtered completion. No substitute answer was generated."
            }
            Self::NotFinished => {
                "The backend explicitly reported that generation was not finished. Partial output was not used."
            }
            Self::UnconfirmedFinish => {
                "The backend did not confirm a supported completed answer. No completion was invented."
            }
        }
    }
}

pub(crate) async fn chat(response: reqwest::Response, backend: Backend) -> Result<Value> {
    let response = crate::model_manager::bounded_json(response).await?;
    normalize(&response, backend)
}

fn normalize(response: &Value, backend: Backend) -> Result<Value> {
    if response.get("error").is_some_and(|value| !value.is_null()) {
        bail!("model backend reported an inference error");
    }
    let (content, reason, done) = match backend {
        Backend::Ollama => (
            response.pointer("/message/content"),
            response.get("done_reason"),
            response.get("done"),
        ),
        Backend::Llamacpp => (
            response.pointer("/choices/0/message/content"),
            response.pointer("/choices/0/finish_reason"),
            None,
        ),
    };
    let content = content
        .and_then(Value::as_str)
        .context("model backend returned no textual completion")?;
    let reason = match reason {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.len() <= 64 && !value.chars().any(char::is_control) => {
            Some(value.as_str())
        }
        _ => bail!("model backend returned invalid completion metadata"),
    };
    let done = match done {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        _ => bail!("model backend returned invalid completion metadata"),
    };
    let complete = if done == Some(false) {
        Some(false)
    } else {
        match reason {
            Some("stop") if backend == Backend::Llamacpp || done == Some(true) => Some(true),
            Some("length" | "tool_calls" | "function_call" | "content_filter") => Some(false),
            _ => None,
        }
    };
    // Additive metadata; existing consumers can continue reading `content`.
    // Provider completion is not semantic validity or authorization to execute.
    let mut normalized =
        json!({"content":content,"finish_reason":reason,"completion_complete":complete});
    if complete != Some(true) {
        let reasoning = match backend {
            Backend::Ollama => response.pointer("/message/thinking"),
            Backend::Llamacpp => response
                .pointer("/choices/0/message/reasoning_content")
                .or_else(|| response.pointer("/choices/0/message/reasoning")),
        }
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty());
        let issue = if done == Some(false) {
            IncompleteReason::NotFinished
        } else {
            match reason {
                Some("length") if content.trim().is_empty() && reasoning => {
                    IncompleteReason::ReasoningOnlyOutputLimit
                }
                Some("length") => IncompleteReason::OutputLimit,
                Some("tool_calls" | "function_call") => IncompleteReason::ToolCall,
                Some("content_filter") => IncompleteReason::ContentFilter,
                _ => IncompleteReason::UnconfirmedFinish,
            }
        };
        normalized["completion_reason"] = json!(issue);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_categories_never_promote_reasoning_or_echo_provider_diagnostics() {
        for backend in [Backend::Ollama, Backend::Llamacpp] {
            for (reason, content, thinking, done, expected) in [
                (
                    "length",
                    "",
                    "PRIVATE_REASONING",
                    true,
                    IncompleteReason::ReasoningOnlyOutputLimit,
                ),
                (
                    "length",
                    " \n",
                    "PRIVATE_REASONING",
                    true,
                    IncompleteReason::ReasoningOnlyOutputLimit,
                ),
                (
                    "length",
                    "PRIVATE_PARTIAL_ANSWER",
                    "PRIVATE_REASONING",
                    true,
                    IncompleteReason::OutputLimit,
                ),
                ("length", "", " \n", true, IncompleteReason::OutputLimit),
                (
                    "tool_calls",
                    "",
                    "PRIVATE_REASONING",
                    true,
                    IncompleteReason::ToolCall,
                ),
                ("function_call", "", "", true, IncompleteReason::ToolCall),
                (
                    "content_filter",
                    "",
                    "",
                    true,
                    IncompleteReason::ContentFilter,
                ),
                (
                    "PRIVATE_FINISH_REASON",
                    "",
                    "",
                    true,
                    IncompleteReason::UnconfirmedFinish,
                ),
            ] {
                let response = match backend {
                    Backend::Ollama => {
                        json!({"message":{"content":content,"thinking":thinking},"done":done,"done_reason":reason})
                    }
                    Backend::Llamacpp => {
                        json!({"choices":[{"message":{"content":content,"reasoning_content":thinking},"finish_reason":reason}]})
                    }
                };
                let value = normalize(&response, backend).unwrap();
                assert_ne!(value["completion_complete"], true);
                assert_eq!(value["content"], content);
                assert_eq!(value["completion_reason"], json!(expected));
                assert!(!value.to_string().contains("PRIVATE_REASONING"));
                assert!(value.get("thinking").is_none());
                assert_eq!(
                    IncompleteReason::from_engine_code(expected.engine_code()),
                    Some(expected)
                );
            }
        }
        let value=normalize(&json!({"message":{"content":"","thinking":"PRIVATE_REASONING"},"done":false,"done_reason":"length"}),Backend::Ollama).unwrap();
        assert_eq!(
            value["completion_reason"],
            json!(IncompleteReason::NotFinished)
        );
    }
    #[test]
    fn completed_shape_is_unchanged_and_unknown_reason_classes_are_not_accepted() {
        let value=normalize(&json!({"message":{"content":"answer","thinking":"PRIVATE_REASONING"},"done":true,"done_reason":"stop"}),Backend::Ollama).unwrap();
        assert_eq!(
            value,
            json!({"content":"answer","finish_reason":"stop","completion_complete":true})
        );
        for backend in [Backend::Ollama, Backend::Llamacpp] {
            let body = match backend {
                Backend::Ollama => json!({"message":{"content":""},"done_reason":"stop"}),
                Backend::Llamacpp => json!({"choices":[{"message":{"content":""}}]}),
            };
            assert_eq!(
                normalize(&body, backend).unwrap()["completion_reason"],
                json!(IncompleteReason::UnconfirmedFinish)
            );
        }
        for raw in ["PRIVATE_REASON", "length", "stop"] {
            assert!(serde_json::from_value::<IncompleteReason>(json!(raw)).is_err());
            assert_eq!(IncompleteReason::from_engine_code(raw), None);
        }
    }
    #[test]
    fn both_adapters_preserve_content_without_inventing_completion() {
        for backend in [Backend::Ollama, Backend::Llamacpp] {
            let response = |reason: Value| match backend {
                Backend::Ollama => {
                    json!({"message":{"content":"answer"},"done":true,"done_reason":reason})
                }
                Backend::Llamacpp => {
                    json!({"choices":[{"message":{"content":"answer"},"finish_reason":reason}]})
                }
            };
            for (reason, expected) in [
                (json!("stop"), json!(true)),
                (json!("length"), json!(false)),
                (json!("tool_calls"), json!(false)),
                (Value::Null, Value::Null),
                (json!("future"), Value::Null),
            ] {
                let result = normalize(&response(reason), backend).unwrap();
                assert_eq!(result["content"], "answer");
                assert_eq!(result["completion_complete"], expected);
            }
            for reason in [json!(true), json!("x".repeat(65)), json!("bad\nreason")] {
                assert!(normalize(&response(reason), backend).is_err());
            }
        }
        for done in [Value::Null, json!(false)] {
            let result = normalize(
                &json!({"message":{"content":"partial"},"done":done,"done_reason":"stop"}),
                Backend::Ollama,
            )
            .unwrap();
            assert_ne!(result["completion_complete"], true);
        }
    }
    #[test]
    fn missing_nontext_or_error_responses_are_not_empty_success() {
        for value in [
            json!({}),
            json!({"error":"private backend details"}),
            json!({"message":{"content":7}}),
            json!({"choices":[]}),
        ] {
            for backend in [Backend::Ollama, Backend::Llamacpp] {
                let error = normalize(&value, backend).unwrap_err().to_string();
                assert!(!error.contains("private backend details"));
            }
        }
        assert!(
            normalize(
                &json!({"message":{"content":"answer"},"done":"true"}),
                Backend::Ollama
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn chat_bodies_are_bounded_even_without_content_length() {
        use axum::{Router, body::Body, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/length", get(|| async { "x".repeat(4 * 1024 * 1024 + 1) }))
            .route(
                "/chunked",
                get(|| async {
                    Body::from_stream(futures_util::stream::iter(
                        (0..65).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 65536])),
                    ))
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for backend in [Backend::Ollama, Backend::Llamacpp] {
            for path in ["length", "chunked"] {
                let response = reqwest::get(format!("http://{address}/{path}"))
                    .await
                    .unwrap();
                assert!(
                    chat(response, backend)
                        .await
                        .unwrap_err()
                        .to_string()
                        .contains("too large")
                );
            }
        }
        server.abort();
    }
}
