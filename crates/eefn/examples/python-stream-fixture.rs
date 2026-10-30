//! Synthetic native streaming-bridge probe. Never installed or work authority.
use anyhow::{Context, Result, bail, ensure};
use eefn::python::{
    BridgeFailure, BridgeLimits, PythonPlugin,
    stream::{PinnedStreamSource, StreamBounds},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    python: PathBuf,
    plugin: PathBuf,
    stream_id: String,
    params: Value,
    bounds: StreamBounds,
    limits: BridgeLimits,
    execution_ms: u64,
    cancel_marker: Option<PathBuf>,
    observer_rejection_marker: Option<PathBuf>,
    pinned_source: Option<PinnedInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedInput {
    source_utf8: String,
    source_sha256: String,
}

async fn marker(path: PathBuf, expected: &'static [u8]) -> Result<()> {
    ensure!(path.is_absolute(), "Absolute owned marker required");
    loop {
        match tokio::fs::read(&path).await {
            Ok(bytes) if bytes == expected => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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
        (1..=10000).contains(&input.execution_ms),
        "Fixture deadline limit"
    );
    let runtime = eefn::PythonRuntime::discover(Some(input.python))
        .context("Explicit fixture interpreter missing")?
        .with_limits(input.limits)?;
    let plugin = PythonPlugin {
        path: input.plugin,
        capability: "stream.observe".into(),
        actions: vec!["observe".into()],
        params: json!({}),
    };
    let started = std::time::Instant::now();
    // Actual future is scoped here so cancellation retires it before reporting.
    let result = {
        let reject = input.observer_rejection_marker;
        let observe = move |observation| {
            let reject = reject.clone();
            async move {
                {
                    let mut out = std::io::stdout().lock();
                    writeln!(
                        out,
                        "{}",
                        json!({"event_type":"observation","observation":observation})
                    )?;
                    out.flush()?;
                }
                if let Some(path) = reject {
                    marker(path, b"reject").await?;
                    bail!("PRIVATE_OBSERVER_FAILURE");
                }
                Ok(())
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_millis(input.execution_ms);
        let operation = async {
            if let Some(source) = input.pinned_source {
                let pinned =
                    PinnedStreamSource::new(source.source_utf8.as_bytes(), &source.source_sha256)?;
                runtime
                    .observe_pinned_stream(
                        &pinned,
                        "observe",
                        input.params,
                        &input.stream_id,
                        input.bounds,
                        deadline,
                        observe,
                    )
                    .await
            } else {
                runtime
                    .observe_stream(
                        &plugin,
                        "observe",
                        input.params,
                        &input.stream_id,
                        input.bounds,
                        deadline,
                        observe,
                    )
                    .await
            }
        };
        tokio::pin!(operation);
        if let Some(path) = input.cancel_marker {
            tokio::select! {
                result=&mut operation=>Some(result),
                value=marker(path,b"ready")=>{value?;None},
            }
        } else {
            Some(operation.await)
        }
    };
    let result = match result {
        Some(Ok(summary)) => json!({"success":true,"summary":summary}),
        Some(Err(error)) => {
            json!({"success":false,"failure":error.downcast_ref::<BridgeFailure>().map(|value|format!("{value:?}")).unwrap_or_else(||"Other".into())})
        }
        None => json!({"success":false,"failure":"Cancelled"}),
    };
    println!(
        "{}",
        json!({"event_type":"result","result":result,"elapsed_ms":started.elapsed().as_millis(),"physical_capture":false,"workload_authorized":false,"external_effects_stopped":false})
    );
    Ok(())
}
