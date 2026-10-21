//! Isolated native receipt/crash helper, not shipped in application packages.
//! Calls the real public HTTPS repair entrypoint. The harness supplies a local
//! TLS gate that never serves an artifact; no validation bypass or model work.
use anyhow::{Context, Result, bail};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || !matches!(args[0].as_str(), "eef" | "eefn") {
        bail!("usage: repair-receipt-fixture PRODUCT OPERATION_UUID HTTPS_URL")
    }
    let executable = std::env::current_exe()?;
    let install = executable.parent().context("missing fixture directory")?;
    let request = eefn::package_repair::RepairRequest {
        schema_version: 1,
        request_id: args[1].clone(),
        version: eefn::VERSION.into(),
        url: args[2].clone(),
        sha256: "a".repeat(64),
        size_bytes: 1,
        allow_prerelease: true,
    };
    let applied = eefn::package_repair::apply_for(
        &request,
        install,
        eefn::VERSION,
        &executable,
        &args[0],
        Duration::from_secs(120),
        || Ok(()),
    )
    .await?;
    println!("{}", serde_json::to_string(&applied)?);
    Ok(())
}
