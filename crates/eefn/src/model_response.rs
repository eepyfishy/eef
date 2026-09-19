//! Bounded chat response normalization shared by the existing inference adapters.
use crate::model_selection::Backend;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

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
    Ok(json!({"content":content,"finish_reason":reason,"completion_complete":complete}))
}

#[cfg(test)]
mod tests {
    use super::*;
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
