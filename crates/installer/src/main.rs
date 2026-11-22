#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
#[cfg(test)]
use serde_json::Value;
use zip::ZipArchive;
#[path = "../../../shared/atomic_file.rs"]
pub mod atomic_file;
#[path = "../../../shared/coordinator_config.rs"]
mod coordinator_config;
#[path = "../../../shared/installer_payload.rs"]
mod installer_payload;
#[cfg(windows)]
mod wizard;
type Progress = Option<std::sync::Arc<dyn Fn(u32, &str) + Send + Sync>>;
const INSTALL_LOCK: &str = ".eef-install.lock";

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
        let message = format!("eef installation failed:\n\n{error:#}");
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
    reject_cmd_text(&install_dir.join(product.executable).to_string_lossy())?;
    reject_cmd_text(&install_dir.join(product.config).to_string_lossy())?;
    let _installation_lock = extract_payload(&payload, &install_dir, &manifest.name, &progress)?;
    if manifest.name == "eef" {
        ensure_coordinator_secret(&install_dir.join(product.config))?;
    }
    select_manual_install(&install_dir)?;

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
            "{} {} was installed in:\n{}\n\nStartup: {}\nOpen it again from the eef folder in your Start menu.",
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
    let range = installer_payload::payload_range(&mut file)?;
    file.seek(SeekFrom::Start(range.start))?;
    let mut payload = vec![0_u8; usize::try_from(range.end - range.start)?];
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

fn verified_unpacked_size(payload: &[u8]) -> Result<u64> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("unsafe path in installer payload")?;
        anyhow::ensure!(
            !relative.components().next().is_some_and(|part| part
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(INSTALL_LOCK)),
            "installer payload uses the setup lock filename"
        );
        let expected = entry.size();
        let actual = std::io::copy(&mut entry, &mut std::io::sink())
            .context("verify installer payload before changing installed files")?;
        anyhow::ensure!(
            actual == expected,
            "installer entry size differs from its declaration"
        );
        total = total
            .checked_add(actual)
            .context("installer payload is too large")?;
    }
    Ok(total)
}

fn product(name: &str) -> Result<&'static Product> {
    static COORDINATOR_PRODUCT: Product = Product {
        display: "eef coordinator",
        directory: "eef",
        executable: "eef.exe",
        config: "config/default_identity.yaml",
        startup_name: "eef Coordinator",
    };
    static NODE_PRODUCT: Product = Product {
        display: "eef node",
        directory: "eefn",
        executable: "eefn.exe",
        config: "config.json",
        startup_name: "eef Node",
    };
    match name {
        "eef" => Ok(&COORDINATOR_PRODUCT),
        "eefn" => Ok(&NODE_PRODUCT),
        _ => bail!("unknown eef product '{name}'"),
    }
}

fn extract_payload(
    payload: &[u8],
    destination: &Path,
    product: &str,
    progress: &Progress,
) -> Result<File> {
    let required = verified_unpacked_size(payload)?;
    check_install_space(destination, required)?;
    fs::create_dir_all(destination)?;
    let mut lock_options = fs::OpenOptions::new();
    lock_options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        lock_options.share_mode(3);
    }
    let installation_lock = lock_options.open(destination.join(INSTALL_LOCK))?;
    fs2::FileExt::try_lock_exclusive(&installation_lock)
        .context("Another setup is using this folder. Wait for it to finish before retrying")?;
    validate_installation_role(destination, product)?;
    let _replacement_guards = replacement_guards(payload, destination, product)?;
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
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        let write = |file: &mut File| std::io::copy(&mut entry, file).map(|_| ());
        if should_preserve_config(product, &relative) {
            if output.exists() {
                continue;
            }
            match atomic_file::create_with(&output, write) {
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                result => {
                    result.with_context(|| format!("create settings: {}", output.display()))?
                }
            }
        } else {
            atomic_file::replace_with(&output, write)
                .with_context(|| format!("install application file: {}", output.display()))?;
        }
    }
    Ok(installation_lock)
}

fn validate_installation_role(destination: &Path, expected: &str) -> Result<()> {
    let bytes = match fs::read(destination.join("bundle.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).context("Read the installed package identity before upgrading");
        }
    };
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    let installed: BundleManifest = serde_json::from_slice(bytes).context(
        "Cannot read this folder's package identity. Choose another installation folder",
    )?;
    anyhow::ensure!(
        installed.name == expected,
        "This folder contains {}. Choose a separate folder for {}",
        product(&installed.name)?.display,
        product(expected)?.display
    );
    Ok(())
}

fn replacement_guards(payload: &[u8], destination: &Path, product: &str) -> Result<Vec<File>> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut guards = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("unsafe path in installer payload")?;
        if entry.is_dir() || should_preserve_config(product, &relative) {
            continue;
        }
        let output = destination.join(relative);
        let mut options = fs::OpenOptions::new();
        options.write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_WRITE_DATA};
            options.access_mode(DELETE | FILE_WRITE_DATA);
        }
        match options.open(&output) {
            Ok(file) => guards.push(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Cannot replace {}. Close the app and check the folder's write permissions before retrying", output.display())
                });
            }
        }
    }
    Ok(guards)
}

fn should_preserve_config(product: &str, relative: &Path) -> bool {
    let normalized = relative.to_string_lossy().replace('\\', "/");
    (product == "eef" && normalized.starts_with("config/"))
        || (product == "eefn" && normalized.eq_ignore_ascii_case("config.json"))
}

fn select_manual_install(destination: &Path) -> Result<()> {
    // A manual installer writes the new root executable. An old updater's
    // current.txt would otherwise hand off to an older versions/ executable.
    // Preserve the selection for recovery; never remove prior version folders.
    let selected = destination.join("current.txt");
    match fs::symlink_metadata(&selected) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.file_type().is_file() => {
            bail!("current.txt must be a regular update-selection file")
        }
        Ok(_) => {}
    }
    let backup = destination.join(format!(
        "current.before-manual-install-{}.txt",
        hex::encode(rand::random::<[u8; 16]>())
    ));
    fs::rename(&selected, backup).context(
        "Preserve the previous update selection before launching the manual installation",
    )?;
    Ok(())
}

fn ensure_coordinator_secret(path: &Path) -> Result<()> {
    coordinator_config::initialize(path)?;
    coordinator_config::ensure_secret_file(path)
}

fn set_startup(install_dir: &Path, product: &Product, enabled: bool) -> Result<()> {
    let startup = startup_path(product)?;
    if enabled {
        fs::create_dir_all(startup.parent().expect("startup parent"))?;
        let executable = install_dir.join(product.executable);
        let config = install_dir.join(product.config);
        reject_cmd_text(&executable.to_string_lossy())?;
        reject_cmd_text(&config.to_string_lossy())?;
        atomic_file::write(
            &startup,
            format!(
                "@echo off\r\nstart \"\" /min \"{}\" --config \"{}\"\r\n",
                executable.display(),
                config.display()
            )
            .as_bytes(),
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
    let mut child = command.spawn().context("launch installed application")?;
    std::thread::sleep(std::time::Duration::from_millis(700));
    if let Some(status) = child
        .try_wait()
        .context("check installed application startup")?
    {
        anyhow::ensure!(
            status.success(),
            "the installed application exited during startup ({status})"
        );
    }
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
        .join("Microsoft/Windows/Start Menu/Programs/eef");
    fs::create_dir_all(&menu)?;
    let executable = directory.join(product.executable);
    let config = directory.join(product.config);
    reject_cmd_text(&executable.to_string_lossy())?;
    reject_cmd_text(&config.to_string_lossy())?;
    atomic_file::write(
        &menu.join(format!("{}.cmd", product.display)),
        format!(
            "@echo off\r\nstart \"\" /min \"{}\" --config \"{}\" --open-dashboard\r\n",
            executable.display(),
            config.display()
        )
        .as_bytes(),
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
            wide("eef installer").as_ptr(),
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
            wide("eef installer").as_ptr(),
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
    use std::io::Write;

    fn zip_payload(name: &str, data: &[u8]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file(name, options).unwrap();
        writer.write_all(data).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn corrupt_later_entry_is_refused_before_any_installation_write() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("eefn.exe"), b"previous application").unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file("eefn.exe", options).unwrap();
        writer.write_all(b"updated application").unwrap();
        writer.start_file("later-runtime.dll", options).unwrap();
        let tail = b"later payload entry with invalid checksum";
        writer.write_all(tail).unwrap();
        let mut payload = writer.finish().unwrap().into_inner();
        let offset = payload
            .windows(tail.len())
            .position(|bytes| bytes == tail)
            .unwrap();
        payload[offset] ^= 1;

        assert!(extract_payload(&payload, &existing, "eefn", &None).is_err());
        assert_eq!(
            fs::read(existing.join("eefn.exe")).unwrap(),
            b"previous application"
        );
        assert_eq!(fs::read_dir(&existing).unwrap().count(), 1);
        let fresh = dir.path().join("fresh");
        assert!(extract_payload(&payload, &fresh, "eefn", &None).is_err());
        assert!(!fresh.exists());
    }

    #[test]
    fn failed_extraction_preserves_the_previous_file_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eefn.exe");
        let old_stage = dir.path().join("eefn.exe.installing");
        fs::write(&path, b"installed before the update").unwrap();
        fs::write(&old_stage, b"existing unrelated file").unwrap();
        let contents = b"complete replacement for the installer fixture";
        let valid = zip_payload("eefn.exe", contents);
        let mut corrupt = valid.clone();
        let offset = corrupt
            .windows(contents.len())
            .position(|data| data == contents)
            .unwrap();
        corrupt[offset] ^= 1;

        assert!(extract_payload(&corrupt, dir.path(), "eefn", &None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"installed before the update");
        assert_eq!(fs::read(&old_stage).unwrap(), b"existing unrelated file");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);

        extract_payload(&valid, dir.path(), "eefn", &None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), contents);
        assert_eq!(fs::read(&old_stage).unwrap(), b"existing unrelated file");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[cfg(windows)]
    #[test]
    fn a_later_locked_file_prevents_replacement_of_earlier_files() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("eefn.exe");
        let later = dir.path().join("runtime.dll");
        fs::write(&first, b"previous application").unwrap();
        fs::write(&later, b"previous runtime").unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&later)
            .unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("eefn.exe", options).unwrap();
        writer.write_all(b"updated application").unwrap();
        writer.start_file("runtime.dll", options).unwrap();
        writer.write_all(b"updated runtime").unwrap();
        let payload = writer.finish().unwrap().into_inner();

        let error = extract_payload(&payload, dir.path(), "eefn", &None).unwrap_err();
        assert!(error.to_string().contains("Close the app"));
        assert_eq!(fs::read(&first).unwrap(), b"previous application");
        assert_eq!(fs::read(&later).unwrap(), b"previous runtime");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        drop(lock);

        extract_payload(&payload, dir.path(), "eefn", &None).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"updated application");
        assert_eq!(fs::read(&later).unwrap(), b"updated runtime");
    }

    #[cfg(windows)]
    #[test]
    fn delete_locked_install_file_survives_a_refused_replacement() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eef.exe");
        fs::write(&path, b"previous application").unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let payload = zip_payload("eef.exe", b"updated application");

        assert!(extract_payload(&payload, dir.path(), "eef", &None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous application");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        drop(guard);

        extract_payload(&payload, dir.path(), "eef", &None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"updated application");
    }

    #[test]
    fn setup_lock_excludes_the_same_destination_until_its_owner_exits() {
        let dir = tempfile::tempdir().unwrap();
        let first = zip_payload("eefn.exe", b"first application");
        let second = zip_payload("eefn.exe", b"second application");
        let lock = extract_payload(&first, dir.path(), "eefn", &None).unwrap();
        let error = extract_payload(&second, dir.path(), "eefn", &None).unwrap_err();
        assert!(error.to_string().contains("Another setup"));
        assert_eq!(
            fs::read(dir.path().join("eefn.exe")).unwrap(),
            b"first application"
        );
        let other = tempfile::tempdir().unwrap();
        extract_payload(&second, other.path(), "eefn", &None).unwrap();
        drop(lock);
        extract_payload(&second, dir.path(), "eefn", &None).unwrap();
        assert_eq!(
            fs::read(dir.path().join("eefn.exe")).unwrap(),
            b"second application"
        );
    }

    #[test]
    fn payload_cannot_replace_the_installer_lock() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("fresh");
        for name in [INSTALL_LOCK, ".eef-install.lock/child"] {
            let payload = zip_payload(name, b"invalid payload entry");
            assert!(extract_payload(&payload, &destination, "eefn", &None).is_err());
            assert!(!destination.exists());
        }
    }

    #[test]
    fn role_mismatch_refuses_extraction_and_preserves_existing_package_files() {
        for (installed, incoming) in [("eef", "eefn"), ("eefn", "eef")] {
            let dir = tempfile::tempdir().unwrap();
            let manifest =
                serde_json::to_vec(&serde_json::json!({"name":installed,"version":"0.3.2"}))
                    .unwrap();
            fs::write(dir.path().join("bundle.json"), &manifest).unwrap();
            fs::write(dir.path().join("owner-settings"), b"owner data").unwrap();
            let incoming_executable = format!("{incoming}.exe");
            let payload = zip_payload(&incoming_executable, b"incoming application");

            let error = extract_payload(&payload, dir.path(), incoming, &None).unwrap_err();
            assert!(error.to_string().contains("Choose a separate folder"));
            assert!(!dir.path().join(incoming_executable).exists());
            assert_eq!(fs::read(dir.path().join("bundle.json")).unwrap(), manifest);
            assert_eq!(
                fs::read(dir.path().join("owner-settings")).unwrap(),
                b"owner data"
            );
        }
    }

    #[test]
    fn matching_legacy_package_identity_accepts_bom_and_extra_manifest_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        fs::write(
            &path,
            b"\xef\xbb\xbf{\"name\":\"eefn\",\"version\":\"0.3.2\",\"runtime\":\"rust\"}",
        )
        .unwrap();
        validate_installation_role(dir.path(), "eefn").unwrap();
        let payload = zip_payload("eefn.exe", b"updated node");
        extract_payload(&payload, dir.path(), "eefn", &None).unwrap();
        assert_eq!(
            fs::read(dir.path().join("eefn.exe")).unwrap(),
            b"updated node"
        );
        fs::write(&path, b"{broken identity").unwrap();
        assert!(validate_installation_role(dir.path(), "eefn").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn atomic_settings_installer_secret_creation_refuses_delete_locked_configuration() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eef.yaml");
        let before =
            b"identity:\n  name: Owner settings\nnode:\n  psk: ''\nfuture:\n  keep: true\n";
        fs::write(&path, before).unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        assert!(ensure_coordinator_secret(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        drop(guard);
        ensure_coordinator_secret(&path).unwrap();
        let saved: Value = serde_yml::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["identity"]["name"], "Owner settings");
        assert_eq!(saved["future"]["keep"], true);
        assert_eq!(saved["node"]["psk"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn atomic_settings_installer_preserves_existing_secret_without_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eef.yaml");
        let before = b"# Owner formatting and existing secret must survive\nnode:\n  psk: existing-fixture-secret\n";
        fs::write(&path, before).unwrap();
        ensure_coordinator_secret(&path).unwrap();
        assert_eq!(fs::read(path).unwrap(), before);
    }

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
        assert!(reject_cmd_text("C:\\Program Files\\eef").is_ok());
        assert!(reject_cmd_text("C:\\bad&path").is_err());
        assert!(reject_cmd_text("C:\\%TEMP%\\eef").is_err());
        assert!(reject_cmd_text("C:\\!variable!\\eef").is_err());
    }

    #[test]
    fn launch_reports_a_child_that_exits_with_a_startup_error() {
        // The Rust test harness rejects the application's --config argument.
        // This exercises a real child exit without launching an installed app.
        let executable = std::env::current_exe().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let error = launch_installed(&executable, &dir.path().join("unused.json")).unwrap_err();
        assert!(error.to_string().contains("exited during startup"));
    }

    #[test]
    fn manual_install_preserves_old_selection_without_launching_it() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("versions/0.3.2");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("preserve.txt"), "old version").unwrap();
        select_manual_install(dir.path()).unwrap();
        fs::write(dir.path().join("current.txt"), "0.3.2\n").unwrap();
        select_manual_install(dir.path()).unwrap();
        assert!(!dir.path().join("current.txt").exists());
        let backups = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("current.before-manual-install-")
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path()).unwrap(), "0.3.2\n");
        assert_eq!(
            fs::read_to_string(old.join("preserve.txt")).unwrap(),
            "old version"
        );
        select_manual_install(dir.path()).unwrap();
        fs::create_dir(dir.path().join("current.txt")).unwrap();
        assert!(select_manual_install(dir.path()).is_err());
    }
}
