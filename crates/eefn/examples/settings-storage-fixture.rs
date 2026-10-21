//! Native test-only read handle denying FILE_SHARE_DELETE. Never shipped in apps.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    let path = std::env::args_os()
        .nth(1)
        .context("Expected an owned fixture file")?;
    let guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(path)?;
    println!("{{\"locked\":true}}");
    std::io::stdout().flush()?;
    let mut release = String::new();
    std::io::stdin().read_line(&mut release)?;
    drop(guard);
    Ok(())
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("This fixture requires Windows file-sharing semantics")
}
