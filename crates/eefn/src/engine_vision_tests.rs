//! Local synthetic HTTP only; no model, device or workload admission exercised.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[cfg(all(windows, feature = "native-test-fixtures"))]
#[path = "engine_vision_native_tests.rs"]
mod native;

fn selection() -> ModelSelection {
    ModelSelection {
        backend: Backend::Ollama,
        model_id: "selected".into(),
        installed_digest: Some("a".repeat(64)),
    }
}
fn limits() -> AnalysisLimits {
    AnalysisLimits {
        max_tokens: 32,
        max_response_bytes: 2048,
        max_result_bytes: 256,
        timeout_ms: 2000,
    }
}
fn granted() -> GrantLimits {
    GrantLimits {
        max_execution_ms: 2000,
        max_result_bytes: 1024,
    }
}
fn image() -> PrivateJpeg {
    PrivateJpeg::checked(STANDARD.encode([0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9]), 128).unwrap()
}
fn tags() -> Value {
    json!({"models":[{"name":"selected","digest":"a".repeat(64),"size":64}]})
}
fn answer() -> Value {
    json!({"message":{"content":"movement detected","thinking":"PRIVATE_REASONING"},"done":true,"done_reason":"stop"})
}
fn response(value: Value) -> String {
    let body = value.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
fn engine(url: String) -> NodeEngine {
    let selected = serde_json::from_value(json!({"model_id":"selected","modality":"vlm"})).unwrap();
    NodeEngine::new(false, vec![], std::path::PathBuf::new())
        .unwrap()
        .with_ollama(url, [selected])
        .unwrap()
}
async fn provider(
    responses: Vec<String>,
) -> (String, tokio::task::JoinHandle<Vec<(String, Value)>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in responses {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut first = String::new();
            socket.read_line(&mut first).await.unwrap();
            let mut size = 0;
            loop {
                let mut header = String::new();
                assert!(socket.read_line(&mut header).await.unwrap() > 0);
                if header == "\r\n" {
                    break;
                }
                if let Some((key, value)) = header.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        size = value.trim().parse::<usize>().unwrap()
                    }
                }
            }
            assert!(size <= 65536);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            requests.push((
                first.trim().into(),
                if size == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&body).unwrap()
                },
            ));
            socket.get_mut().write_all(reply.as_bytes()).await.unwrap();
        }
        requests
    });
    (url, task)
}

#[test]
fn local_vision_endpoint_rejects_dns_remote_auth_paths_queries_and_fragments() {
    for raw in [
        "http://127.0.0.1:1234",
        "https://127.0.0.2/",
        "http://[::1]:1234/",
    ] {
        assert!(local_base(raw).is_ok(), "Refused local endpoint");
    }
    for raw in [
        "http://localhost:1234",
        "http://example.org",
        "http://192.168.1.2",
        "http://127.0.0.1:0",
        "http://user:secret@127.0.0.1/",
        "http://127.0.0.1/path",
        "http://127.0.0.1/?",
        "http://127.0.0.1/#",
        "file:///private",
        " http://127.0.0.1",
    ] {
        assert!(
            local_base(raw).is_err(),
            "Non-local or ambiguous endpoint accepted"
        );
    }
}

#[test]
fn local_vision_scope_requires_explicit_identity_and_finite_limits() {
    selection().validate().unwrap();
    limits().validate().unwrap();
    for digest in [None, Some("bad".into()), Some("A".repeat(64))] {
        let mut changed = selection();
        changed.installed_digest = digest;
        assert!(changed.validate().is_err());
    }
    let mut wrong = selection();
    wrong.backend = Backend::Llamacpp;
    assert!(wrong.validate().is_err());
    wrong.installed_digest = None;
    wrong.validate().unwrap();
    for field in [
        "max_tokens",
        "max_response_bytes",
        "max_result_bytes",
        "timeout_ms",
    ] {
        let mut raw = json!(limits());
        raw[field] = json!(u64::MAX);
        if let Ok(widened) = serde_json::from_value::<AnalysisLimits>(raw) {
            assert!(widened.validate().is_err())
        }
    }
    let mut raw = json!(selection());
    raw["endpoint"] = json!("http://PRIVATE");
    assert!(serde_json::from_value::<ModelSelection>(raw).is_err());
}

#[test]
fn local_vision_binding_never_selects_or_falls_back_to_another_model_or_backend() {
    let owner = engine("http://127.0.0.1:1".into());
    owner.bind_local_vision(&selection()).unwrap();
    let mut other = selection();
    other.model_id = "missing".into();
    assert!(owner.bind_local_vision(&other).is_err());
    other = selection();
    other.backend = Backend::Llamacpp;
    other.installed_digest = None;
    assert!(owner.bind_local_vision(&other).is_err());
    let text = serde_json::from_value(json!({"model_id":"selected","modality":"text"})).unwrap();
    let owner = NodeEngine::new(false, vec![], std::path::PathBuf::new())
        .unwrap()
        .with_ollama("http://127.0.0.1:1".into(), [text])
        .unwrap();
    assert!(owner.bind_local_vision(&selection()).is_err());
}

#[tokio::test]
async fn local_vision_reuses_selected_adapter_payload_and_confirms_model_before_and_after() {
    let (url, task) = provider(vec![response(tags()), response(answer()), response(tags())]).await;
    let owner = engine(url);
    let binding = owner.bind_local_vision(&selection()).unwrap();
    let frame = image();
    let text = binding
        .analyze(
            &frame,
            "Describe movement",
            &limits(),
            &granted(),
            Instant::now() + Duration::from_secs(2),
            || Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(text, "movement detected");
    let requests = task.await.unwrap();
    assert_eq!(
        requests.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        [
            "GET /api/tags HTTP/1.1",
            "POST /api/chat HTTP/1.1",
            "GET /api/tags HTTP/1.1"
        ]
    );
    let call = &requests[1].1;
    assert_eq!(call["model"], "selected");
    assert_eq!(call["stream"], false);
    assert_eq!(call["options"]["num_predict"], 32);
    assert_eq!(call["messages"][0]["content"], "Describe movement");
    assert_eq!(call["messages"][0]["images"], json!([frame.as_base64()]));
    assert!(call.get("tools").is_none());
}

#[tokio::test]
async fn local_vision_llamacpp_uses_the_existing_owned_model_pin_and_image_payload() {
    let completion =
        json!({"choices":[{"message":{"content":"movement detected"},"finish_reason":"stop"}]});
    let (url, task) = provider(vec![response(completion)]).await;
    // Existing synthetic resident process proves pin accounting, not inference.
    let mut server = crate::ModelServer::test_resident();
    let configured = Arc::get_mut(&mut server).unwrap();
    configured.slots[0].port = reqwest::Url::parse(&url).unwrap().port().unwrap();
    configured.slots[0].mmproj_path = Some("synthetic-mmproj".into());
    let owner = NodeEngine::new(false, vec![], std::path::PathBuf::new())
        .unwrap()
        .with_model_server(server.clone());
    let selected = ModelSelection {
        backend: Backend::Llamacpp,
        model_id: "owned-model".into(),
        installed_digest: None,
    };
    let binding = owner.bind_local_vision(&selected).unwrap();
    let frame = image();
    assert_eq!(
        binding
            .analyze(
                &frame,
                "Describe movement",
                &limits(),
                &granted(),
                Instant::now() + Duration::from_secs(2),
                || Ok(())
            )
            .await
            .unwrap(),
        "movement detected"
    );
    let requests = task.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "POST /v1/chat/completions HTTP/1.1");
    let payload = &requests[0].1;
    assert_eq!(payload["max_tokens"], 32);
    assert_eq!(
        payload["messages"][0]["content"][0]["text"],
        "Describe movement"
    );
    assert_eq!(
        payload["messages"][0]["content"][1]["image_url"]["url"],
        format!("data:image/jpeg;base64,{}", frame.as_base64())
    );
    assert_eq!(server.snapshot().unwrap()[0]["active_requests"], 0);
}

#[tokio::test]
async fn local_vision_refuses_remote_models_changed_digests_and_ambiguous_provider_identity() {
    let mut cloud = tags();
    cloud["models"][0]["remote_host"] = json!("PRIVATE-CLOUD");
    let mut changed = tags();
    changed["models"][0]["digest"] = json!("b".repeat(64));
    let mut duplicate = tags();
    duplicate["models"]
        .as_array_mut()
        .unwrap()
        .push(tags()["models"][0].clone());
    for report in [cloud, changed, duplicate, json!({"models":[]})] {
        let (url, task) = provider(vec![response(report)]).await;
        let owner = engine(url);
        let binding = owner.bind_local_vision(&selection()).unwrap();
        let error = binding
            .analyze(
                &image(),
                "Analyze",
                &limits(),
                &granted(),
                Instant::now() + Duration::from_secs(2),
                || Ok(()),
            )
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("PRIVATE"));
        assert_eq!(task.await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn local_vision_denies_revoked_authority_before_sending_any_image() {
    let (url, task) = provider(vec![response(tags())]).await;
    let owner = engine(url);
    let binding = owner.bind_local_vision(&selection()).unwrap();
    let calls = AtomicUsize::new(0);
    let error = binding
        .analyze(
            &image(),
            "Analyze",
            &limits(),
            &granted(),
            Instant::now() + Duration::from_secs(2),
            || {
                if calls.fetch_add(1, Ordering::SeqCst) >= 1 {
                    anyhow::bail!("PRIVATE-REVOKED")
                }
                Ok(())
            },
        )
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("PRIVATE"));
    let requests = task.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].0.starts_with("GET "));
}

#[tokio::test]
async fn local_vision_does_not_follow_image_post_redirects() {
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{}/receive\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        destination.local_addr().unwrap()
    );
    let (url, task) = provider(vec![response(tags()), redirect]).await;
    let owner = engine(url);
    let binding = owner.bind_local_vision(&selection()).unwrap();
    let result = binding
        .analyze(
            &image(),
            "Analyze",
            &limits(),
            &granted(),
            Instant::now() + Duration::from_secs(2),
            || Ok(()),
        )
        .await;
    assert!(result.is_err());
    assert_eq!(task.await.unwrap().len(), 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), destination.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn local_vision_never_returns_partial_oversize_or_identity_changed_output() {
    let mut partial = answer();
    partial["done_reason"] = json!("length");
    let mut huge = answer();
    huge["message"]["content"] = json!("PRIVATE".repeat(1000));
    let mut text_limit = answer();
    text_limit["message"]["content"] = json!("x".repeat(257));
    let mut changed = tags();
    changed["models"][0]["digest"] = json!("b".repeat(64));
    for replies in [
        vec![tags(), partial],
        vec![tags(), huge],
        vec![tags(), text_limit],
        vec![tags(), answer(), changed],
    ] {
        let (url, task) = provider(replies.into_iter().map(response).collect()).await;
        let owner = engine(url);
        let binding = owner.bind_local_vision(&selection()).unwrap();
        let error = binding
            .analyze(
                &image(),
                "Analyze",
                &limits(),
                &granted(),
                Instant::now() + Duration::from_secs(2),
                || Ok(()),
            )
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("PRIVATE"));
        assert!(!error.to_string().contains("movement"));
        let requests = task.await.unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.0.starts_with("POST ")).count(),
            1
        );
    }
}

#[tokio::test]
async fn local_vision_rejects_expired_invalid_and_revoked_calls_without_network_effects() {
    let owner = engine("http://127.0.0.1:1".into());
    let binding = owner.bind_local_vision(&selection()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    for bad in [true, false] {
        let mut bounds = limits();
        if bad {
            bounds.timeout_ms = u64::MAX;
        }
        let counter = calls.clone();
        let result = binding
            .analyze(
                &image(),
                "Analyze",
                &bounds,
                &granted(),
                Instant::now(),
                move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await;
        assert!(result.is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        binding
            .analyze(
                &image(),
                "Analyze",
                &limits(),
                &granted(),
                Instant::now() + Duration::from_secs(2),
                || anyhow::bail!("PRIVATE-denied")
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("outcome unconfirmed")
    );
}
