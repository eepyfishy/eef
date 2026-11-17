//! Test-only fake llama-server. Never packaged; no model inference or downloads.
use axum::{
    Json, Router,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    anyhow::ensure!(
        std::env::var("EEF_MODEL_STARTUP_FIXTURE").as_deref() == Ok("1"),
        "test fixture only"
    );
    let args: Vec<String> = std::env::args().collect();
    let arg = |key: &str| -> anyhow::Result<String> {
        args.windows(2)
            .find(|v| v[0] == key)
            .map(|v| v[1].clone())
            .ok_or_else(|| anyhow::anyhow!("missing {key}"))
    };
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(arg("--model")?)?)?;
    // Optional nested finite process fixture inherits this model's owned job.
    // It is never a production llama argument or a packaged executable.
    #[cfg(windows)]
    let _descendants = if let Some(directory) = config["process_tree_fixture_dir"].as_str() {
        use std::os::windows::process::CommandExt;
        let directory = PathBuf::from(directory);
        anyhow::ensure!(
            directory.is_absolute() && directory.is_dir(),
            "isolated tree directory required"
        );
        let helper = std::env::current_exe()?.with_file_name("process-fixture.exe");
        Some(
            std::process::Command::new(helper)
                .arg("loop")
                .arg(directory)
                .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?,
        )
    } else {
        None
    };
    let ready = PathBuf::from(config["ready_file"].as_str().unwrap());
    let marker = PathBuf::from(config["pid_file"].as_str().unwrap());
    let infer_gate = config["infer_gate"].as_str().map(PathBuf::from);
    let infer_started = config["infer_started"].as_str().map(PathBuf::from);
    let vision = config.get("vision_fixture").cloned();
    if let Some(vision) = &vision {
        anyhow::ensure!(
            vision["model"].as_str() == Some(arg("--alias")?.as_str()),
            "synthetic vision alias mismatch"
        );
    }
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", arg("--port")?)).await?;
    std::fs::write(marker, std::process::id().to_string())?;
    axum::serve(
        listener,
        Router::new().route("/v1/chat/completions", post(move |Json(request): Json<Value>| {let gate=infer_gate.clone();let started=infer_started.clone();let vision=vision.clone();async move {
            if let Some(vision)=vision {
                let trace=PathBuf::from(vision["requests"].as_str().unwrap());
                if !trace.is_absolute() {return Json(json!({"error":"synthetic vision scope rejected"}));}
                let mut attempts=std::fs::OpenOptions::new().create(true).append(true).open(trace.with_extension("attempts")).unwrap();
                std::io::Write::write_all(&mut attempts,b"request\n").unwrap();
                let valid = request.get("model").is_none()
                    && request["messages"].as_array().is_some_and(|m|m.len()==1)
                    && request["messages"][0]["content"] == json!([
                        {"type":"text","text":"Describe movement"},
                        {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,/9gBAgME/9k="}}
                    ]) && request["max_tokens"] == 32;
                if !valid {return Json(json!({"error":"synthetic vision request rejected"}));}
                let started=PathBuf::from(vision["started"].as_str().unwrap());
                let release=PathBuf::from(vision["release"].as_str().unwrap());
                let raw=serde_json::to_vec(&request).unwrap();
                if !trace.is_absolute() || !started.is_absolute() || !release.is_absolute()
                    || raw.len()>65536 || trace.exists() {return Json(json!({"error":"synthetic vision scope or replay rejected"}));}
                std::fs::write(trace,raw).unwrap();
                std::fs::write(started,b"synthetic vision request active").unwrap();
                let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(15);
                while !release.is_file() {
                    if tokio::time::Instant::now()>=deadline {return Json(json!({"error":"synthetic vision gate expired"}));}
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                return Json(json!({"choices":[{"message":{"content":"movement detected"},"finish_reason":"stop"}]}));
            }
            let prompt = request["messages"].as_array().and_then(|v|v.last()).and_then(|v|v["content"].as_str());
            if prompt==Some("fixture-gate") {
                if let Some(file)=started {std::fs::write(file,b"synthetic request active").unwrap();}
                if let Some(file)=gate {while !file.is_file(){tokio::time::sleep(std::time::Duration::from_millis(10)).await;}}
            }
                    Json(match prompt {
                        Some("fixture-preview") => json!({"choices":[{"message":{"content":json!({
                            "schema_version":1,"intent":"information","goal":"Inspect node status",
                            "context_hints":[],"constraints":[],"suggested_capabilities":["system.info"],
                            "complexity":"simple","workload":"one_shot","needs_clarification":false,"clarification":null
                        }).to_string()},"finish_reason":"stop"}]}),
                        Some("fixture-missing") => json!({}),
                Some("fixture-length") => json!({"choices":[{"message":{"content":"fixture partial"},"finish_reason":"length"}]}),
                _ => json!({"choices":[{"message":{"content":"fixture only; no real inference"},"finish_reason":"stop"}]}),
            })
        }})).route(
            "/health",
            get(move || {
                let ready = ready.clone();
                async move {
                    if ready.is_file() {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }
            }),
        ),
    )
    .await?;
    Ok(())
}
