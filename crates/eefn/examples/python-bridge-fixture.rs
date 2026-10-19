//! Isolated verification adapter for the production Python bridge. Not packaged
//! or an execution authority; the caller supplies its own interpreter/plugin.
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    python: PathBuf,
    plugin: PathBuf,
    #[serde(default)]
    limits: eefn::python::BridgeLimits,
    action: String,
    #[serde(default)]
    params: Value,
    cancel_after_ms: Option<u64>,
    cancel_after_marker: Option<PathBuf>,
}

async fn wait_for_marker(path: PathBuf) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match tokio::fs::read(&path).await {
                Ok(bytes) if bytes == b"ready" => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("Fixture cancellation marker was not observed")?
}
#[tokio::main]
async fn main() -> Result<()> {
    let mut bytes = vec![];
    std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "Fixture input too large");
    let input: Input = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        input.cancel_after_ms.is_none() || input.cancel_after_marker.is_none(),
        "Choose one fixture cancellation trigger"
    );
    let runtime = eefn::PythonRuntime::discover(Some(input.python))
        .context("Fixture interpreter missing")?
        .with_limits(input.limits)?;
    let started = std::time::Instant::now();
    let result = async {
        let plugin = runtime.inspect(input.plugin).await?;
        runtime.invoke(&plugin, &input.action, input.params).await
    };
    let output = if let Some(path) = input.cancel_after_marker {
        tokio::pin!(result);
        tokio::select! {
            output = &mut result => Some(output),
            observed = wait_for_marker(path) => {
                observed?;
                None
            }
        }
    } else {
        match input.cancel_after_ms {
            Some(ms) => match tokio::time::timeout(Duration::from_millis(ms), result).await {
                Ok(result) => Some(result),
                Err(_) => None,
            },
            None => Some(result.await),
        }
    };
    let response = match output {
        Some(Ok(data)) => json!({"success":true,"data":data}),
        Some(Err(error)) => {
            json!({"success":false,"failure":error.downcast_ref::<eefn::python::BridgeFailure>().map(|e|format!("{e:?}")).unwrap_or_else(||"Other".into()),"error":format!("{error:#}")})
        }
        None => json!({"success":false,"failure":"Cancelled"}),
    };
    println!(
        "{}",
        json!({"elapsed_ms":started.elapsed().as_millis(),"result":response})
    );
    Ok(())
}
