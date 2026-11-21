#[path = "../../shared/windows_version.rs"]
mod windows_version;

fn main() -> std::io::Result<()> {
    windows_version::embed("eef", "eef coordinator")
}
