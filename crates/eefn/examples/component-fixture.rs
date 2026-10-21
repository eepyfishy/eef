//! Test-only sentinel replacing an unavailable Python/model executable. It
//! records invocation inside a caller-owned fixture and always refuses to run.
fn main() -> std::io::Result<std::process::ExitCode> {
    let executable = std::env::current_exe()?;
    let python = executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("python.exe"));
    let marker = if python {
        std::path::PathBuf::from(std::env::var_os("EEF_TEST_PYTHON_MARKER").ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Expected owned fixture marker",
            )
        })?)
    } else {
        executable
            .parent()
            .ok_or_else(|| std::io::Error::other("Missing fixture directory"))?
            .join("fixture-started.marker")
    };
    std::fs::write(
        marker,
        b"Test-only executable invoked; no model/Python work performed",
    )?;
    Ok(std::process::ExitCode::FAILURE)
}
