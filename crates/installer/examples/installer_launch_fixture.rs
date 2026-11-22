//! Isolated wizard regression executable. Never included in release payloads.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::io::Result<()> {
    let path = std::env::current_exe()?.with_file_name("fixture-launch-attempts.txt");
    let attempts = match std::fs::read_to_string(&path) {
        Ok(value) => value.trim().parse::<u32>().expect("fixture attempt count"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    } + 1;
    std::fs::write(path, attempts.to_string())?;
    if attempts <= 2 {
        std::process::exit(13);
    }
    Ok(())
}
