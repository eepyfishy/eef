//! Synthetic panic observer. Never included in application bundles.
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let report = PathBuf::from(args.next().context("report path required")?);
    if !report.is_absolute() || args.next().is_some() {
        bail!("One absolute new report path required")
    }
    eefn::panic_report::install(&report, "eefn")?;
    eefn::panic_report::phase("synthetic_failure");
    panic!("PRIVATE_SYNTHETIC_REQUEST_PAYLOAD must never enter the report");
}
