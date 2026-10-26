//! Test-only command-lock holder; never included in installed bundles.
use anyhow::{Context, Result, bail};
use std::io::{BufRead, Write};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let config = PathBuf::from(
        args.next()
            .context("existing fixture configuration required")?,
    );
    if args.next().is_some() || !config.is_absolute() || !config.is_file() {
        bail!("one absolute existing fixture configuration required")
    }
    let _guard = eefn::setup::instance_lock(&config)?.context("command lock already held")?;
    println!("command lock held");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim_end() != "release" {
        bail!("explicit fixture release required")
    }
    Ok(())
}
