#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use zip::ZipArchive;
#[cfg(windows)]
mod wizard;
type Progress = Option<std::sync::Arc<dyn Fn(u32, &str) + Send + Sync>>;

const FOOTER_MAGIC: &[u8; 8] = b"EEFINST1";

#[derive(Debug, Deserialize)]
struct BundleManifest {
    name: String,
    version: String,
}

#[derive(Default)]
struct Options {
    install_dir: Option<PathBuf>,
    startup: Option<bool>,
    launch: Option<bool>,
    quiet: bool,
}

struct Product {
    display: &'static str,
    directory: &'static str,
    executable: &'static str,
    config: &'static str,
    startup_name: &'static str,
}

fn main() {
    if let Err(error) = run() {
        let message = format!("EEF installation failed:\n\n{error:#}");
        if let Some(path) = std::env::var_os("EEF_INSTALLER_ERROR_LOG") {
            let _ = fs::write(path, &message);
        }
        if !std::env::args_os().any(|argument| argument == "--quiet") {
            show_error(&message);
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let options = parse_options()?;
    #[cfg(windows)]
    if !options.quiet {
        return wizard::run(options);
    }
    install(options, None)
}

fn install(options: Options, progress: Progress) -> Result<()> {
    if let Some(report) = &progress {
        report(2, "Checking the installer and destination…");
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let payload = read_payload(&executable)?;
    let manifest = read_manifest(&payload)?;
    let product = product(&manifest.name)?;
    let install_dir = match options.install_dir {
        Some(path) => absolute(path)?,
        None => {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable")?)
                .join(product.directory)
        }
    };
    reject_download_directory(&executable, &install_dir)?;
    check_install_space(&install_dir, unpacked_size(&payload)?)?;
    extract_payload(&payload, &install_dir, &manifest.name, &progress)?;
    if manifest.name == "eef" {
        ensure_coordinator_secret(&install_dir.join(product.config))?;
    }

    let startup = options.startup.unwrap_or_else(|| {
        if options.quiet {
            startup_path(product).is_ok_and(|path| path.is_file())
        } else {
            ask_yes_no(
                "Startup application",
                &format!("Start {} automatically when you sign in?", product.display),
            )
        }
    });
    set_startup(&install_dir, product, startup)?;
    if let Some(report) = &progress {
        report(95, "Creating your Start menu entry…");
    }
    create_app_shortcut(&install_dir, product)?;

    let installed_executable = install_dir.join(product.executable);
    let launch = options.launch.unwrap_or_else(|| {
        !options.quiet
            && ask_yes_no(
                "Launch application",
                &format!(
                    "Installation finished. Start {} and open its dashboard now?",
                    product.display
                ),
            )
    });
    if launch {
        launch_installed(&installed_executable, &install_dir.join(product.config))?;
    }
    if !options.quiet {
        show_info(&format!(
            "{} {} was installed in:\n{}\n\nStartup: {}\nOpen it again from the EEF folder in your Start menu.",
            product.display,
            manifest.version,
            install_dir.display(),
            if startup { "enabled" } else { "disabled" },
        ));
    }
    if let Some(report) = &progress {
        report(100, "Installation complete");
    }
    Ok(())
}

fn parse_options() -> Result<Options> {
    let mut options = Options::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--install-dir" => {
                options.install_dir = Some(PathBuf::from(
                    args.next().context("--install-dir requires a path")?,
                ));
            }
            "--startup" => options.startup = Some(true),
            "--no-startup" => options.startup = Some(false),
            "--launch" => options.launch = Some(true),
            "--no-launch" => options.launch = Some(false),
            "--quiet" => options.quiet = true,
            "--help" | "-h" => {
                show_info(
                    "Options:\n  --install-dir PATH\n  --startup | --no-startup\n  --launch | --no-launch\n  --quiet (does not launch; preserves startup unless explicitly changed)",
                );
                std::process::exit(0);
            }
            other => bail!("unknown installer option '{other}'"),
        }
    }
    Ok(options)
}

fn read_payload(executable: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(executable)?;
    let length = file.metadata()?.len();
    if length < 16 {
        bail!("installer payload footer is missing")
    }
    file.seek(SeekFrom::End(-16))?;
    let mut footer = [0_u8; 16];
    file.read_exact(&mut footer)?;
    if &footer[8..] != FOOTER_MAGIC {
        bail!("installer payload signature is invalid")
    }
    let payload_length = u64::from_le_bytes(footer[..8].try_into().expect("footer length"));
    if payload_length == 0 || payload_length > length - 16 {
        bail!("installer payload length is invalid")
    }
    file.seek(SeekFrom::Start(length - 16 - payload_length))?;
    let mut payload = vec![0_u8; usize::try_from(payload_length)?];
    file.read_exact(&mut payload)?;
    if !payload.starts_with(b"PK\x03\x04") {
        bail!("installer payload is not a ZIP archive")
    }
    Ok(payload)
}

fn read_manifest(payload: &[u8]) -> Result<BundleManifest> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry
            .enclosed_name()
            .and_then(|path| path.file_name().map(ToOwned::to_owned))
            .is_some_and(|name| name.eq_ignore_ascii_case("bundle.json"))
        {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
            return serde_json::from_slice(bytes).context("bundle.json is invalid");
        }
    }
    bail!("installer payload has no bundle.json")
}

fn unpacked_size(payload: &[u8]) -> Result<u64> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        total = total
            .checked_add(archive.by_index(index)?.size())
            .context("installer payload is too large")?;
    }
    Ok(total)
}

fn product(name: &str) -> Result<&'static Product> {
    static EEF: Product = Product {
        display: "EEF coordinator",
        directory: "EEF",
        executable: "eef.exe",
        config: "config/default_identity.yaml",
        startup_name: "EEF Coordinator",
    };
    static EEFN: Product = Product {
        display: "EEFN node",
        directory: "EEFN",
        executable: "eefn.exe",
        config: "config.json",
        startup_name: "EEF Node",
    };
    match name {
        "eef" => Ok(&EEF),
        "eefn" => Ok(&EEFN),
        _ => bail!("unknown EEF product '{name}'"),
    }
}

fn extract_payload(
    payload: &[u8],
    destination: &Path,
    product: &str,
    progress: &Progress,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let entries = archive.len();
    for index in 0..entries {
        if let Some(report) = progress {
            report(
                10 + (80 * index / entries.max(1)) as u32,
                "Installing application files…",
            );
        }
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("unsafe path in installer payload")?
            .to_path_buf();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(&relative);
        if should_preserve_config(product, &relative) && output.exists() {
            continue;
        }
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let next = output.with_extension(format!(
            "{}installing",
            output
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        {
            let mut file = File::create(&next)?;
            std::io::copy(&mut entry, &mut file)?;
            file.sync_all()?;
        }
        if output.exists() {
            fs::remove_file(&output).with_context(|| {
                format!(
                    "close the running application and retry: {}",
                    output.display()
                )
            })?;
        }
        fs::rename(&next, &output)?;
    }
    Ok(())
}

fn should_preserve_config(product: &str, relative: &Path) -> bool {
    let normalized = relative.to_string_lossy().replace('\\', "/");
    (product == "eef" && normalized.starts_with("config/"))
        || (product == "eefn" && normalized.eq_ignore_ascii_case("config.json"))
}

fn ensure_coordinator_secret(path: &Path) -> Result<()> {
    let mut value: Value = serde_yml::from_slice(&fs::read(path)?)?;
    let current = value
        .pointer("/node/psk")
        .and_then(Value::as_str)
        .unwrap_or("");
    if current.len() >= 12 && !current.contains("CHANGE_ME") {
        return Ok(());
    }
    let object = value
        .as_object_mut()
        .context("EEF config is not a mapping")?;
    let node = object.entry("node").or_insert_with(|| json!({}));
    node.as_object_mut()
        .context("node config is not a mapping")?
        .insert(
            "psk".into(),
            Value::String(hex::encode(rand::random::<[u8; 32]>())),
        );
    fs::write(path, serde_yml::to_string(&value)?)?;
    Ok(())
}

fn set_startup(install_dir: &Path, product: &Product, enabled: bool) -> Result<()> {
    let startup = startup_path(product)?;
    if enabled {
        fs::create_dir_all(startup.parent().expect("startup parent"))?;
        let executable = install_dir.join(product.executable);
        let config = install_dir.join(product.config);
        reject_cmd_text(&executable.to_string_lossy())?;
        reject_cmd_text(&config.to_string_lossy())?;
        fs::write(
            &startup,
            format!(
                "@echo off\r\nstart \"\" /min \"{}\" --config \"{}\"\r\n",
                executable.display(),
                config.display()
            ),
        )?;
    } else if startup.is_file() {
        fs::remove_file(startup)?;
    }
    Ok(())
}

fn startup_path(product: &Product) -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("APPDATA").context("APPDATA is unavailable")?)
            .join("Microsoft/Windows/Start Menu/Programs/Startup")
            .join(format!("{}.cmd", product.startup_name)),
    )
}

fn launch_installed(executable: &Path, config: &Path) -> Result<()> {
    let mut command = Command::new(executable);
    command
        .arg("--config")
        .arg(config)
        .arg("--open-dashboard")
        .current_dir(
            executable
                .parent()
                .context("installed executable has no directory")?,
        );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.spawn().context("launch installed application")?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    Ok(())
}

fn reject_download_directory(executable: &Path, destination: &Path) -> Result<()> {
    let source = executable.parent().context("installer has no directory")?;
    let source = normalize(source)?;
    let destination = normalize(destination)?;
    if source == destination || source.starts_with(&destination) || destination.starts_with(&source)
    {
        bail!("choose an installation directory separate from the downloaded installer")
    }
    Ok(())
}

fn check_install_space(destination: &Path, payload_bytes: u64) -> Result<()> {
    let mut probe = destination;
    while !probe.exists() {
        probe = probe
            .parent()
            .context("installation path has no existing parent")?;
    }
    let available = fs2::available_space(probe)?;
    if available < payload_bytes {
        bail!(
            "There is not enough free space for the installed files. Choose another drive or free some space."
        )
    }
    Ok(())
}

fn create_app_shortcut(directory: &Path, product: &Product) -> Result<()> {
    let menu = PathBuf::from(std::env::var_os("APPDATA").context("APPDATA is unavailable")?)
        .join("Microsoft/Windows/Start Menu/Programs/EEF");
    fs::create_dir_all(&menu)?;
    let executable = directory.join(product.executable);
    let config = directory.join(product.config);
    reject_cmd_text(&executable.to_string_lossy())?;
    reject_cmd_text(&config.to_string_lossy())?;
    fs::write(
        menu.join(format!("{}.cmd", product.display)),
        format!(
            "@echo off\r\nstart \"\" /min \"{}\" --config \"{}\" --open-dashboard\r\n",
            executable.display(),
            config.display()
        ),
    )?;
    Ok(())
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn normalize(path: &Path) -> Result<PathBuf> {
    let absolute = absolute(path.to_path_buf())?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
        }
    }
    Ok(normalized)
}

fn reject_cmd_text(value: &str) -> Result<()> {
    if value.contains(['\r', '\n', '"', '&', '|', '<', '>', '^', '%', '!']) {
        bail!("installation path contains unsupported command characters")
    }
    Ok(())
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
fn ask_yes_no(title: &str, message: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_YESNO, MessageBoxW,
    };
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide(title).as_ptr(),
            MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND,
        ) == IDYES
    }
}

#[cfg(not(windows))]
fn ask_yes_no(_: &str, _: &str) -> bool {
    false
}

#[cfg(windows)]
fn show_info(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MessageBoxW,
    };
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide("EEF installer").as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn show_info(message: &str) {
    println!("{message}");
}

#[cfg(windows)]
fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW,
    };
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(message).as_ptr(),
            wide("EEF installer").as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(windows))]
fn show_error(message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_preservation_is_product_specific() {
        assert!(should_preserve_config(
            "eef",
            Path::new("config/default_identity.yaml")
        ));
        assert!(should_preserve_config("eefn", Path::new("config.json")));
        assert!(!should_preserve_config("eefn", Path::new("eefn.exe")));
    }

    #[test]
    fn command_file_rejects_metacharacters() {
        assert!(reject_cmd_text("C:\\Program Files\\EEF").is_ok());
        assert!(reject_cmd_text("C:\\bad&path").is_err());
        assert!(reject_cmd_text("C:\\%TEMP%\\EEF").is_err());
        assert!(reject_cmd_text("C:\\!variable!\\EEF").is_err());
    }
}
