//! Synthetic consent/revocation fixture. No microphone or workload admission.
use anyhow::{Context, Result, ensure};
use eefn::listening::{Command, Request};
use serde::Deserialize;
use serde_json::json;
use std::{io::Read, path::PathBuf, sync::Arc, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    root: PathBuf,
    python: PathBuf,
    plugin: PathBuf,
}
async fn marker(path: PathBuf) -> Result<()> {
    loop {
        match tokio::fs::read(&path).await {
            Ok(bytes) if bytes == b"ready" => return Ok(()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "Fixture input limit");
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(
        input.root.is_absolute() && input.root.is_dir(),
        "Owned fixture directory required"
    );
    let config = input.root.join("node.json");
    eefn::setup::write_json(
        &config,
        &json!({"node_id":"listening-fixture","psk":"fixture-only","endpoints":[],"permissions":{"media":{"microphone":true}},"dashboard":{"host":"127.0.0.1","port":1,"enabled":true}}),
    )?;
    let _lock = eefn::setup::instance_lock(&config)?.context("Fixture already running")?;
    let service = eefn::NodeService::new(config, "listening-fixture".into());
    let runtime_id = uuid::Uuid::new_v4().to_string();
    let mut policy = eefn::NodePolicy::default();
    policy.media.microphone = true;
    let engine =
        Arc::new(eefn::NodeEngine::new(false, vec![], input.root.clone())?.with_policy(policy)?);
    let _binding = service.attach_preview_runtime(&engine, runtime_id.clone());
    service.live.lock().unwrap()["runtime_id"] = json!(runtime_id);
    let api = service.start_api("127.0.0.1", 0, false).await?;
    // Wait for the real CLI to enable consent; no source exists before begin.
    tokio::fs::write(input.root.join("ready"), b"ready").await?;
    tokio::time::timeout(Duration::from_secs(30), marker(input.root.join("begin"))).await??;
    let runtime = eefn::PythonRuntime::discover(Some(input.python))
        .context("Explicit interpreter required")?
        .with_limits(eefn::python::BridgeLimits {
            timeout_seconds: 30,
            max_request_bytes: 65536,
            max_stdout_bytes: 32768,
            max_stderr_bytes: 1024,
            max_job_memory_mib: Some(128),
        })?;
    let plugin = eefn::PythonPlugin {
        path: input.plugin,
        capability: "fixture.observe".into(),
        actions: vec!["observe".into()],
        params: json!({}),
    };
    let stream_id = uuid::Uuid::new_v4().to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let first = input.root.join("observation");
    let operation = runtime.observe_stream(
        &plugin,
        "observe",
        json!({"root":input.root}),
        &stream_id,
        eefn::python::stream::StreamBounds {
            max_frame_bytes: 1024,
            max_stdout_bytes: 32768,
            max_observations: 16,
            max_source_frames: 10000,
        },
        deadline,
        move |value| {
            let path = first.clone();
            async move {
                tokio::fs::write(path, serde_json::to_vec(&value)?).await?;
                Ok(())
            }
        },
    );
    let started = std::time::Instant::now();
    let result = service
        .supervise_listening(&runtime_id, deadline, operation)
        .await;
    ensure!(
        result.is_err(),
        "Synthetic source must be revoked, not completed"
    );
    let report = service.listening_command(
        Request {
            schema_version: 1,
            expected_node_id: "listening-fixture".into(),
            expected_runtime_id: runtime_id.clone(),
            command: Command::Show {},
        },
        false,
    )?;
    tokio::fs::write(input.root.join("supervision-result.json"),serde_json::to_vec(&json!({"revoked":true,"elapsed_ms":started.elapsed().as_millis(),"runtime_id":runtime_id,"consent_report":report,"physical_capture":false,"workload_authorized":false,"external_effects_stopped":false}))?).await?;
    tokio::time::timeout(Duration::from_secs(30), marker(input.root.join("finish"))).await??;
    api.abort();
    Ok(())
}
