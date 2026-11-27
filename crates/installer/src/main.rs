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
#[allow(dead_code)] // Other consumers use startup helpers; setup uses the pure byte planner.
mod coordinator_config;
#[path = "../../../shared/installer_payload.rs"]
mod installer_payload;
#[path = "../../../shared/package_use.rs"]
#[allow(dead_code)] // Applications hold shared guards; setup holds exclusive guards.
mod package_use;
#[path = "../../../shared/setup_journal_format.rs"]
#[allow(dead_code)] // The installer writes; applications only inspect the same format.
mod setup_journal_format;
#[path = "../../../shared/setup_state.rs"]
#[allow(dead_code)] // Package clients use the guard; setup owns recovery itself.
mod setup_state;
mod transaction;
#[cfg(windows)]
mod wizard;
type Progress = Option<std::sync::Arc<dyn Fn(u32, &str) + Send + Sync>>;
const INSTALL_LOCK: &str = ".update.lock";

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
    install(options, None).map(|_| ())
}

fn install(options: Options, progress: Progress) -> Result<Option<String>> {
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
    validate_install_destination(&executable, &install_dir, &manifest.name)?;
    let startup_path = startup_path(product)?;
    let startup = options.startup.unwrap_or_else(|| {
        if options.quiet {
            startup_path.is_file()
        } else {
            ask_yes_no(
                "Startup application",
                &format!("Start {} automatically when you sign in?", product.display),
            )
        }
    });
    let shortcuts = Shortcuts {
        startup: startup_path,
        menu: app_shortcut_path(product)?,
    };
    let (_installation_lock, cleanup_warnings) = install_files(
        &payload,
        &install_dir,
        &manifest.name,
        startup,
        &shortcuts,
        &progress,
    )?;
    let warning = (!cleanup_warnings.is_empty()).then(|| {
        format!(
            "Installation completed. Some previous-file backups could not be removed:\n{}",
            cleanup_warnings.join("\n")
        )
    });
    if let Some(warning) = &warning {
        eprintln!("{warning}");
    }

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
    Ok(warning)
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

fn payload_path(name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    let mut relative = PathBuf::new();
    for component in Path::new(&normalized).components() {
        let value = match component {
            std::path::Component::CurDir => continue,
            std::path::Component::Normal(value) => {
                value.to_str().context("Invalid installer filename")?
            }
            _ => bail!("unsafe path in installer payload"),
        };
        anyhow::ensure!(
            !value.ends_with(['.', ' '])
                && !value
                    .chars()
                    .any(|ch| ch.is_control()
                        || matches!(ch, ':' | '<' | '>' | '"' | '|' | '?' | '*')),
            "Unsupported Windows filename in installer payload"
        );
        // Windows device names remain reserved even with filename extensions.
        // https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
        let stem = value.split('.').next().unwrap().trim_end_matches(' ');
        let serial = stem.get(..3).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
        }) && matches!(
            stem.get(3..),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
        );
        anyhow::ensure!(
            !serial
                && !["CON", "PRN", "AUX", "NUL"]
                    .iter()
                    .any(|reserved| stem.eq_ignore_ascii_case(reserved)),
            "Reserved Windows device name in installer payload"
        );
        relative.push(value);
    }
    anyhow::ensure!(
        !relative.components().next().is_some_and(|part| {
            let name = part.as_os_str().to_string_lossy();
            name.eq_ignore_ascii_case(INSTALL_LOCK)
                || name.eq_ignore_ascii_case(package_use::LOCK_NAME)
                || name.eq_ignore_ascii_case(transaction::JOURNAL_NAME)
        }),
        "installer payload uses a setup metadata filename"
    );
    Ok(relative)
}

fn verified_unpacked_size(payload: &[u8]) -> Result<u64> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        entry
            .enclosed_name()
            .context("unsafe path in installer payload")?;
        let relative = payload_path(entry.name())?;
        anyhow::ensure!(
            !relative.as_os_str().is_empty() || entry.is_dir(),
            "Empty installer file path"
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

fn prepare_extraction(payload: &[u8], destination: &Path) -> Result<(File, u64)> {
    let required = verified_unpacked_size(payload)?;
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
        .context("Another setup, update or repair is using this folder. Wait for it to finish before retrying")?;
    Ok((installation_lock, required))
}

struct Shortcuts {
    startup: PathBuf,
    menu: PathBuf,
}

fn install_files(
    payload: &[u8],
    destination: &Path,
    name: &str,
    startup: bool,
    shortcuts: &Shortcuts,
    progress: &Progress,
) -> Result<(File, Vec<String>)> {
    let (lock, required) = prepare_extraction(payload, destination)?;
    let _package_use = package_use::for_setup(destination)?;
    let external = [shortcuts.startup.as_path(), shortcuts.menu.as_path()];
    if transaction::recover_pending(destination, name, &external)? {
        if let Some(report) = progress {
            report(4, "Recovered the previous setup operation…");
        }
    }
    check_install_space(destination, required)?;
    let mut transaction = transaction::Transaction::persistent(destination, name, &external)?;
    let result = (|| {
        apply_payload(payload, destination, name, progress, &mut transaction)?;
        let product = product(name)?;
        if name == "eef" {
            transaction
                .edit(
                    &destination.join(product.config),
                    coordinator_config::installer_bytes,
                )
                .context("Prepare coordinator settings")?;
        }
        select_manual_install(destination, &mut transaction)?;
        set_startup(
            destination,
            product,
            startup,
            &shortcuts.startup,
            &mut transaction,
        )?;
        if let Some(report) = progress {
            report(95, "Creating your Start menu entry…");
        }
        create_app_shortcut(destination, product, &shortcuts.menu, &mut transaction)
            .with_context(|| format!("Create Start menu entry: {}", shortcuts.menu.display()))
    })();
    let warnings = transaction.finish(result)?;
    Ok((lock, warnings))
}

fn apply_payload(
    payload: &[u8],
    destination: &Path,
    product: &str,
    progress: &Progress,
    transaction: &mut transaction::Transaction,
) -> Result<()> {
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
        let relative = payload_path(entry.name())?;
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
            match transaction.create_with(&output, write) {
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
                {
                    continue;
                }
                result => {
                    result.with_context(|| format!("create settings: {}", output.display()))?
                }
            }
        } else {
            transaction
                .replace_with(&output, write)
                .with_context(|| format!("install application file: {}", output.display()))?;
        }
    }
    Ok(())
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

/// Read-only checks shared by the wizard and unattended setup. Extraction still
/// rechecks the installed role while holding the setup/update lock.
fn validate_install_destination(executable: &Path, destination: &Path, name: &str) -> Result<()> {
    let destination = normalize(destination)?;
    anyhow::ensure!(
        destination.file_name().is_some(),
        "Choose a dedicated installation folder, rather than the root of a drive"
    );
    reject_download_directory(executable, &destination)?;
    let product = product(name)?;
    reject_cmd_text(&destination.join(product.executable).to_string_lossy())?;
    reject_cmd_text(&destination.join(product.config).to_string_lossy())?;
    let mut ancestor = destination.as_path();
    loop {
        match fs::metadata(ancestor) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir(),
                    "This location is a file. Choose an installation folder"
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .context("This folder has no existing parent")?;
            }
            Err(error) => return Err(error).context("Cannot inspect the installation folder"),
        }
    }
    if let Some(pending) = transaction::pending_role(&destination)? {
        anyhow::ensure!(
            pending == name,
            "Use the matching role's installer to recover this setup"
        );
        Ok(())
    } else {
        validate_installation_role(&destination, name)
    }
}

fn replacement_guards(payload: &[u8], destination: &Path, product: &str) -> Result<Vec<File>> {
    let mut archive = ZipArchive::new(Cursor::new(payload))?;
    let mut guards = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let relative = payload_path(entry.name())?;
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
    (product == "eef"
        && normalized
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("config/")))
        || (product == "eefn" && normalized.eq_ignore_ascii_case("config.json"))
}

fn select_manual_install(
    destination: &Path,
    transaction: &mut transaction::Transaction,
) -> Result<()> {
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
    transaction.preserve_selection(&selected, &backup).context(
        "Preserve the previous update selection before launching the manual installation",
    )?;
    Ok(())
}

#[cfg(test)]
fn ensure_coordinator_secret(path: &Path) -> Result<()> {
    coordinator_config::initialize(path)?;
    coordinator_config::ensure_secret_file(path)
}

fn set_startup(
    install_dir: &Path,
    product: &Product,
    enabled: bool,
    startup: &Path,
    transaction: &mut transaction::Transaction,
) -> Result<()> {
    if enabled {
        fs::create_dir_all(startup.parent().expect("startup parent"))?;
        let executable = install_dir.join(product.executable);
        let config = install_dir.join(product.config);
        reject_cmd_text(&executable.to_string_lossy())?;
        reject_cmd_text(&config.to_string_lossy())?;
        transaction.write(
            startup,
            format!(
                "@echo off\r\nstart \"\" /min \"{}\" --config \"{}\"\r\n",
                executable.display(),
                config.display()
            )
            .as_bytes(),
        )?;
    } else {
        transaction.remove(startup)?;
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
    let source = resolve_install_location(source)?;
    let destination = resolve_install_location(destination)?;
    if source == destination || source.starts_with(&destination) || destination.starts_with(&source)
    {
        bail!("choose an installation directory separate from the downloaded installer")
    }
    Ok(())
}

fn resolve_install_location(path: &Path) -> Result<PathBuf> {
    let normalized = normalize(path)?;
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .context("This folder has no existing parent")?
                        .to_os_string(),
                );
                ancestor = ancestor
                    .parent()
                    .context("This folder has no existing parent")?;
            }
            Err(error) => return Err(error).context("Cannot inspect the installation folder"),
        }
    }
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

fn app_shortcut_path(product: &Product) -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("APPDATA").context("APPDATA is unavailable")?)
            .join("Microsoft/Windows/Start Menu/Programs/eef")
            .join(format!("{}.cmd", product.display)),
    )
}

fn create_app_shortcut(
    directory: &Path,
    product: &Product,
    shortcut: &Path,
    transaction: &mut transaction::Transaction,
) -> Result<()> {
    let executable = directory.join(product.executable);
    let config = directory.join(product.config);
    reject_cmd_text(&executable.to_string_lossy())?;
    reject_cmd_text(&config.to_string_lossy())?;
    transaction.write(
        shortcut,
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

    fn extract_payload(
        payload: &[u8],
        destination: &Path,
        product: &str,
        progress: &Progress,
    ) -> Result<File> {
        let (lock, required) = prepare_extraction(payload, destination)?;
        check_install_space(destination, required)?;
        let mut transaction = transaction::Transaction::new();
        let result = apply_payload(payload, destination, product, progress, &mut transaction);
        assert!(transaction.finish(result)?.is_empty());
        Ok(lock)
    }

    fn select_manual_install(destination: &Path) -> Result<()> {
        let mut transaction = transaction::Transaction::new();
        let result = super::select_manual_install(destination, &mut transaction);
        assert!(transaction.finish(result)?.is_empty());
        Ok(())
    }

    fn zip_payload(name: &str, data: &[u8]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer.start_file(name, options).unwrap();
        writer.write_all(data).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn setup_fixture_payload(name: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let manifest =
            serde_json::to_vec(&serde_json::json!({"name":name,"version":"new"})).unwrap();
        let executable = format!("{name}.exe");
        let config = product(name).unwrap().config;
        for (path, contents) in [
            ("bundle.json", manifest.as_slice()),
            (&executable, b"new application"),
            (config, b"{}"),
            ("runtime/new.dll", b"new runtime"),
        ] {
            writer.start_file(path, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn setup_files_under(path: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                files.extend(setup_files_under(&entry.path()));
            } else {
                files.push(entry.path());
            }
        }
        files.sort();
        files
    }

    #[test]
    fn late_start_menu_failure_restores_upgraded_files_settings_selector_and_startup() {
        for name in ["eef", "eefn"] {
            for enable_startup in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().join("installation");
                fs::create_dir(&root).unwrap();
                let product = product(name).unwrap();
                let manifest =
                    serde_json::to_vec(&serde_json::json!({"name":name,"version":"old"})).unwrap();
                fs::write(root.join("bundle.json"), &manifest).unwrap();
                fs::write(root.join(product.executable), b"old application").unwrap();
                fs::create_dir_all(root.join(product.config).parent().unwrap()).unwrap();
                let settings: &[u8] = if name == "eef" {
                    b"# owner\nnode:\n  psk: ''\nfuture: keep\n"
                } else {
                    b"{\"owner\":true}"
                };
                fs::write(root.join(product.config), settings).unwrap();
                fs::write(root.join("current.txt"), b"old-version\n").unwrap();
                fs::write(root.join("owner.data"), b"owner data").unwrap();
                let startup = dir.path().join("startup.cmd");
                fs::write(&startup, b"old startup choice").unwrap();
                let blocked = dir.path().join("blocked-menu");
                fs::write(&blocked, b"owner file blocking the menu parent").unwrap();
                let before = setup_files_under(dir.path())
                    .into_iter()
                    .map(|path| {
                        let bytes = fs::read(&path).unwrap();
                        (path, bytes)
                    })
                    .collect::<Vec<_>>();
                let shortcuts = Shortcuts {
                    startup,
                    menu: blocked.join("entry.cmd"),
                };
                let reached_menu = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let reached = reached_menu.clone();
                let callback_root = root.clone();
                let callback_startup = shortcuts.startup.clone();
                let progress: Progress = Some(std::sync::Arc::new(move |percent, _| {
                    if percent == 95 {
                        assert_eq!(
                            fs::read(callback_root.join(product.executable)).unwrap(),
                            b"new application"
                        );
                        assert!(callback_root.join("runtime/new.dll").is_file());
                        assert!(!callback_root.join("current.txt").exists());
                        if enable_startup {
                            assert_ne!(fs::read(&callback_startup).unwrap(), b"old startup choice");
                        } else {
                            assert!(!callback_startup.exists());
                        }
                        if name == "eef" {
                            let settings: Value = serde_yml::from_slice(
                                &fs::read(callback_root.join(product.config)).unwrap(),
                            )
                            .unwrap();
                            assert_eq!(settings["node"]["psk"].as_str().unwrap().len(), 64);
                        }
                        reached.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }));
                let error = install_files(
                    &setup_fixture_payload(name),
                    &root,
                    name,
                    enable_startup,
                    &shortcuts,
                    &progress,
                )
                .unwrap_err();
                assert!(reached_menu.load(std::sync::atomic::Ordering::SeqCst));
                assert!(format!("{error:#}").contains("Create Start menu entry"));
                assert!(format!("{error:#}").contains("rolled back"));
                for (path, bytes) in &before {
                    assert_eq!(fs::read(path).unwrap(), *bytes, "{}", path.display());
                }
                let mut expected = before.into_iter().map(|(path, _)| path).collect::<Vec<_>>();
                expected.push(root.join(INSTALL_LOCK));
                expected.push(root.join(package_use::LOCK_NAME));
                expected.sort();
                assert_eq!(setup_files_under(dir.path()), expected);
            }
        }
    }

    #[test]
    fn setup_waits_for_every_package_user_before_recovery_or_mutation() {
        for name in ["eef", "eefn"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("installation");
            let payload = setup_fixture_payload(name);
            drop(extract_payload(&payload, &root, name, &None).unwrap());
            let executable = root.join(product(name).unwrap().executable);
            fs::write(&executable, b"running application bytes").unwrap();
            let shortcuts = Shortcuts {
                startup: dir.path().join("startup.cmd"),
                menu: dir.path().join("menu.cmd"),
            };
            let first = package_use::for_service(&root, name).unwrap();
            let second = package_use::for_service(&root, name).unwrap();
            let before = setup_files_under(dir.path());
            let contents = before
                .iter()
                .map(|path| (path.clone(), fs::read(path).unwrap()))
                .collect::<Vec<_>>();
            let error = install_files(&payload, &root, name, false, &shortcuts, &None)
                .err()
                .expect("An active package user must refuse setup");
            assert!(format!("{error:#}").contains("exclusive installation access"));
            assert_eq!(setup_files_under(dir.path()), before);
            for (path, bytes) in &contents {
                assert_eq!(fs::read(path).unwrap(), *bytes);
            }
            assert!(!root.join(transaction::JOURNAL_NAME).exists());
            assert!(!shortcuts.menu.exists());
            drop(first);
            assert!(install_files(&payload, &root, name, false, &shortcuts, &None).is_err());
            assert_eq!(fs::read(&executable).unwrap(), b"running application bytes");
            drop(second);
            let (update_lock, warnings) =
                install_files(&payload, &root, name, false, &shortcuts, &None).unwrap();
            assert!(warnings.is_empty());
            assert_eq!(fs::read(&executable).unwrap(), b"new application");
            // Setup releases package ownership before the wrapper launches, even
            // though that wrapper intentionally retains its update lock.
            let service = package_use::for_service(&root, name).unwrap();
            drop(service);
            drop(update_lock);
        }
    }

    #[test]
    fn failed_first_install_removes_its_created_files_and_releases_the_lock() {
        for name in ["eef", "eefn"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("installation");
            let blocked = dir.path().join("blocked-menu");
            fs::write(&blocked, b"owner file").unwrap();
            let shortcuts = Shortcuts {
                startup: dir.path().join("startup.cmd"),
                menu: blocked.join("entry.cmd"),
            };
            let payload = setup_fixture_payload(name);
            let error = install_files(&payload, &root, name, true, &shortcuts, &None).unwrap_err();
            assert!(format!("{error:#}").contains("Create Start menu entry"));
            assert_eq!(
                setup_files_under(&root),
                vec![root.join(package_use::LOCK_NAME), root.join(INSTALL_LOCK)]
            );
            assert!(!shortcuts.startup.exists());
            assert_eq!(fs::read(&blocked).unwrap(), b"owner file");
            let corrected = Shortcuts {
                menu: dir.path().join("menu/entry.cmd"),
                ..shortcuts
            };
            let (lock, warnings) =
                install_files(&payload, &root, name, true, &corrected, &None).unwrap();
            assert!(warnings.is_empty());
            assert_eq!(
                fs::read(root.join(product(name).unwrap().executable)).unwrap(),
                b"new application"
            );
            assert!(corrected.startup.is_file());
            assert!(corrected.menu.is_file());
            assert!(setup_files_under(dir.path()).iter().all(|path| {
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".eef-setup-")
            }));
            drop(lock);
        }
    }

    #[test]
    fn late_payload_failure_restores_an_earlier_replaced_executable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("installation");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("eefn.exe"), b"old app").unwrap();
        let callback_root = root.clone();
        let progress: Progress = Some(std::sync::Arc::new(move |percent, _| {
            // Guards already passed. Introduce an actual filesystem failure
            // only after the earlier archive entry was published.
            if percent > 10 && !callback_root.join("blocked").exists() {
                fs::write(callback_root.join("blocked"), b"concurrent owner file").unwrap();
            }
        }));
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("eefn.exe", options).unwrap();
        writer.write_all(b"new app").unwrap();
        writer.start_file("blocked/later.dll", options).unwrap();
        writer.write_all(b"new runtime").unwrap();
        let payload = writer.finish().unwrap().into_inner();
        assert!(extract_payload(&payload, &root, "eefn", &progress).is_err());
        assert_eq!(fs::read(root.join("eefn.exe")).unwrap(), b"old app");
        assert_eq!(
            fs::read(root.join("blocked")).unwrap(),
            b"concurrent owner file"
        );
        assert_eq!(setup_files_under(&root).len(), 3);
    }

    // This fixture is compiled only into the Rust test executable. Shipping
    // installers contain no environment-controlled halt or fault-injection hook.
    #[test]
    fn setup_process_crash_child() {
        if std::env::var("EEF_SETUP_TEST_CHILD").as_deref() != Ok("1") {
            return;
        }
        let root = PathBuf::from(std::env::var_os("EEF_SETUP_TEST_ROOT").unwrap());
        let name = std::env::var("EEF_SETUP_TEST_ROLE").unwrap();
        let menu = PathBuf::from(std::env::var_os("EEF_SETUP_TEST_MENU").unwrap());
        let shortcuts = Shortcuts {
            startup: root.parent().unwrap().join("startup.cmd"),
            menu,
        };
        let result = install_files(
            &setup_fixture_payload(&name),
            &root,
            &name,
            true,
            &shortcuts,
            &None,
        );
        panic!("Crash fixture did not reach the requested checkpoint: {result:?}");
    }

    #[test]
    fn killed_setup_recovers_original_files_or_keeps_an_already_committed_installation() {
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for name in ["eef", "eefn"] {
            for checkpoint in [
                "staging:1",
                "written:1",
                "prepared:1",
                "moved:1",
                "published:1",
                "committed:0",
                "cleaned:0",
                "undone:1",
            ] {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().join("installation");
                let product = product(name).unwrap();
                fs::create_dir_all(root.join(product.config).parent().unwrap()).unwrap();
                fs::write(root.join(product.executable), b"old application").unwrap();
                fs::write(
                    root.join("bundle.json"),
                    serde_json::to_vec(&serde_json::json!({"name":name,"version":"old"})).unwrap(),
                )
                .unwrap();
                let settings: &[u8] = if name == "eef" {
                    b"# owner\nnode:\n  psk: ''\nfuture: keep\n"
                } else {
                    b"{\"owner\":true}"
                };
                fs::write(root.join(product.config), settings).unwrap();
                fs::write(root.join("current.txt"), b"old-version\n").unwrap();
                fs::write(root.join("owner.data"), b"owner data").unwrap();
                let startup = dir.path().join("startup.cmd");
                fs::write(&startup, b"old startup").unwrap();
                let menu = if checkpoint == "undone:1" {
                    let blocker = dir.path().join("blocked-menu");
                    fs::write(&blocker, b"owner menu blocker").unwrap();
                    blocker.join("entry.cmd")
                } else {
                    dir.path().join("menu.cmd")
                };
                if checkpoint != "undone:1" {
                    fs::write(&menu, b"old menu").unwrap();
                }
                let before = setup_files_under(dir.path())
                    .into_iter()
                    .map(|path| {
                        let bytes = fs::read(&path).unwrap();
                        (path, bytes)
                    })
                    .collect::<Vec<_>>();
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args(["--exact", "tests::setup_process_crash_child", "--nocapture"])
                    .env("EEF_SETUP_TEST_CHILD", "1")
                    .env("EEF_SETUP_TEST_ROOT", &root)
                    .env("EEF_SETUP_TEST_ROLE", name)
                    .env("EEF_SETUP_TEST_MENU", &menu)
                    .env("EEF_SETUP_TEST_HALT", checkpoint)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x0800_0000);
                }
                let mut child = OwnedChild(command.spawn().unwrap());
                let marker = root.join(".setup-test-ready");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                while !marker.is_file() {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        let mut errors = String::new();
                        if let Some(mut stderr) = child.0.stderr.take() {
                            stderr.read_to_string(&mut errors).unwrap();
                        }
                        panic!("{name} {checkpoint} exited before checkpoint ({status}): {errors}");
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "{name} {checkpoint} did not reach its checkpoint"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                assert_eq!(fs::read_to_string(&marker).unwrap(), checkpoint);
                child.0.kill().unwrap();
                child.0.wait().unwrap();
                fs::remove_file(marker).unwrap();
                let downloads = dir.path().join("downloads");
                fs::create_dir(&downloads).unwrap();
                let journal_before = fs::read(root.join(transaction::JOURNAL_NAME)).unwrap();
                validate_install_destination(&downloads.join("setup.exe"), &root, name).unwrap();
                assert_eq!(
                    fs::read(root.join(transaction::JOURNAL_NAME)).unwrap(),
                    journal_before
                );
                if matches!(checkpoint, "moved:1" | "cleaned:0") {
                    let recovered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let observed = recovered.clone();
                    let callback_root = root.clone();
                    let progress: Progress = Some(std::sync::Arc::new(move |percent, _| {
                        if percent == 4 {
                            let expected: &[u8] = if checkpoint == "cleaned:0" {
                                b"new application"
                            } else {
                                b"old application"
                            };
                            assert_eq!(
                                fs::read(callback_root.join(product.executable)).unwrap(),
                                expected
                            );
                            assert!(!callback_root.join(transaction::JOURNAL_NAME).exists());
                            let competing = fs::OpenOptions::new()
                                .read(true)
                                .write(true)
                                .open(callback_root.join(INSTALL_LOCK))
                                .unwrap();
                            assert!(fs2::FileExt::try_lock_exclusive(&competing).is_err());
                            observed.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }));
                    let shortcuts = Shortcuts { startup, menu };
                    let (retry_lock, warnings) = install_files(
                        &setup_fixture_payload(name),
                        &root,
                        name,
                        true,
                        &shortcuts,
                        &progress,
                    )
                    .unwrap();
                    assert!(recovered.load(std::sync::atomic::Ordering::SeqCst));
                    assert!(warnings.is_empty());
                    assert_eq!(
                        fs::read(root.join(product.executable)).unwrap(),
                        b"new application"
                    );
                    assert_eq!(fs::read(root.join("owner.data")).unwrap(), b"owner data");
                    assert!(!root.join(transaction::JOURNAL_NAME).exists());
                    assert!(setup_files_under(dir.path()).iter().all(|path| {
                        !path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".eef-setup-")
                    }));
                    drop(retry_lock);
                    continue;
                }
                let lock = fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(root.join(INSTALL_LOCK))
                    .unwrap();
                fs2::FileExt::try_lock_exclusive(&lock).unwrap();
                assert!(
                    transaction::recover_pending(&root, name, &[&startup, &menu]).unwrap(),
                    "{name} {checkpoint}"
                );
                assert!(!root.join(transaction::JOURNAL_NAME).exists());
                if matches!(checkpoint, "committed:0" | "cleaned:0") {
                    assert_eq!(
                        fs::read(root.join(product.executable)).unwrap(),
                        b"new application"
                    );
                    assert!(root.join("runtime/new.dll").is_file());
                    assert!(!root.join("current.txt").exists());
                    assert_ne!(fs::read(&startup).unwrap(), b"old startup");
                    assert_ne!(fs::read(&menu).unwrap(), b"old menu");
                    assert_eq!(fs::read(root.join("owner.data")).unwrap(), b"owner data");
                    assert_eq!(
                        fs::read_dir(&root)
                            .unwrap()
                            .filter_map(|entry| entry.ok())
                            .filter(|entry| entry
                                .file_name()
                                .to_string_lossy()
                                .starts_with("current.before-manual-install-"))
                            .count(),
                        1
                    );
                } else {
                    for (path, bytes) in &before {
                        assert_eq!(
                            fs::read(path).unwrap(),
                            *bytes,
                            "{name} {checkpoint}: {}",
                            path.display()
                        );
                    }
                    let mut expected = before
                        .iter()
                        .map(|(path, _)| path.clone())
                        .collect::<Vec<_>>();
                    expected.push(root.join(INSTALL_LOCK));
                    expected.push(root.join(package_use::LOCK_NAME));
                    expected.sort();
                    assert_eq!(
                        setup_files_under(dir.path()),
                        expected,
                        "{name} {checkpoint}"
                    );
                }
                assert!(setup_files_under(dir.path()).iter().all(|path| {
                    !path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".eef-setup-")
                }));
                assert!(!transaction::recover_pending(&root, name, &[&startup, &menu]).unwrap());
                drop(lock);
                if checkpoint == "undone:1" {
                    let blocker = menu.parent().unwrap();
                    fs::rename(blocker, blocker.with_extension("owner-copy")).unwrap();
                }
                let shortcuts = Shortcuts { startup, menu };
                let (retry_lock, warnings) = install_files(
                    &setup_fixture_payload(name),
                    &root,
                    name,
                    true,
                    &shortcuts,
                    &None,
                )
                .unwrap();
                assert!(warnings.is_empty());
                assert_eq!(
                    fs::read(root.join(product.executable)).unwrap(),
                    b"new application"
                );
                assert!(!root.join(transaction::JOURNAL_NAME).exists());
                assert!(shortcuts.startup.is_file() && shortcuts.menu.is_file());
                drop(retry_lock);
            }
        }
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
        for name in [
            INSTALL_LOCK,
            package_use::LOCK_NAME,
            ".package-use.lock/child",
            ".PACKAGE-USE.LOCK",
            "./.package-use.lock",
            ".\\.PACKAGE-USE.LOCK",
            ".update.lock/child",
            transaction::JOURNAL_NAME,
            ".setup-transaction/child",
            ".SETUP-TRANSACTION",
            "./.update.lock",
            ".\\.update.lock",
            "./.setup-transaction",
            ".\\.SETUP-TRANSACTION",
        ] {
            let payload = zip_payload(name, b"invalid payload entry");
            assert!(extract_payload(&payload, &destination, "eefn", &None).is_err());
            assert!(!destination.exists());
        }
    }

    #[test]
    fn normalized_payload_names_preserve_owner_settings_for_both_roles() {
        for name in ["eef", "eefn"] {
            let dir = tempfile::tempdir().unwrap();
            let config = product(name).unwrap().config;
            let destination = dir.path().join(config);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::write(&destination, b"owner settings").unwrap();
            let payload = zip_payload(&format!("./{config}"), b"bundled defaults");
            extract_payload(&payload, dir.path(), name, &None).unwrap();
            assert_eq!(fs::read(&destination).unwrap(), b"owner settings");
            assert_eq!(setup_files_under(dir.path()).len(), 2);
        }
        assert_eq!(
            payload_path("./packages/เสียง.py").unwrap(),
            Path::new("packages/เสียง.py")
        );
    }

    #[cfg(windows)]
    #[test]
    fn case_variant_config_directory_never_replaces_owner_coordinator_settings() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config/default_identity.yaml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, b"owner settings with existing authority").unwrap();
        let payload = zip_payload("CONFIG/default_identity.yaml", b"bundled defaults");
        extract_payload(&payload, dir.path(), "eef", &None).unwrap();
        assert_eq!(
            fs::read(&config).unwrap(),
            b"owner settings with existing authority"
        );
        assert_eq!(setup_files_under(dir.path()).len(), 2);
    }

    #[test]
    fn unsafe_windows_payload_names_fail_before_installation_writes() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "../owner",
            "folder/../owner",
            "file:stream",
            "folder./file",
            "folder /file",
            "NUL",
            "NUL.txt",
            "nested/com1.dll",
            "nested/LPT².txt",
            "C:/owner",
            "\\\\server\\share\\owner",
            "file?name",
            ".update.lock.",
            ".setup-transaction ",
        ] {
            let destination = dir.path().join("fresh");
            let payload = zip_payload(name, b"invalid payload");
            assert!(
                extract_payload(&payload, &destination, "eefn", &None).is_err(),
                "{name}"
            );
            assert!(!destination.exists(), "{name}");
        }
    }

    #[test]
    fn setup_and_existing_update_lock_exclude_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let selector = dir.path().join("current.txt");
        fs::write(&selector, b"0.4.0-alpha.6\n").unwrap();
        let update = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join(".update.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&update).unwrap();
        let payload = zip_payload("eefn.exe", b"new application");

        let error = extract_payload(&payload, dir.path(), "eefn", &None).unwrap_err();
        assert!(error.to_string().contains("update or repair"));
        assert!(!dir.path().join("eefn.exe").exists());
        assert_eq!(fs::read(&selector).unwrap(), b"0.4.0-alpha.6\n");
        drop(update);

        let setup = extract_payload(&payload, dir.path(), "eefn", &None).unwrap();
        let update = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.path().join(".update.lock"))
            .unwrap();
        assert!(fs2::FileExt::try_lock_exclusive(&update).is_err());
        drop(setup);
        fs2::FileExt::try_lock_exclusive(&update).unwrap();
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
    fn folder_preflight_is_read_only_for_new_and_existing_installations() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let installer = downloads.join("setup.exe");
        for name in ["eef", "eefn"] {
            let destination = dir.path().join(name);
            validate_install_destination(&installer, &destination, name).unwrap();
            assert!(!destination.exists());
            fs::create_dir(&destination).unwrap();
            let manifest =
                serde_json::to_vec(&serde_json::json!({"name":name,"version":"old"})).unwrap();
            fs::write(destination.join("bundle.json"), &manifest).unwrap();
            validate_install_destination(&installer, &destination, name).unwrap();
            assert_eq!(fs::read(destination.join("bundle.json")).unwrap(), manifest);
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
        }
    }

    #[test]
    fn folder_preflight_rejects_invalid_locations_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let installer = downloads.join("setup.exe");
        let file = dir.path().join("owner-file");
        fs::write(&file, b"owner content").unwrap();
        for destination in [
            downloads.clone(),
            downloads.join("eef"),
            dir.path().to_path_buf(),
            dir.path().join("bad&folder"),
            file.clone(),
            file.join("eef"),
        ] {
            assert!(
                validate_install_destination(&installer, &destination, "eef").is_err(),
                "{}",
                destination.display()
            );
        }
        assert_eq!(fs::read(file).unwrap(), b"owner content");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(fs::read_dir(downloads).unwrap().count(), 0);
        let root = dir.path().ancestors().last().unwrap();
        assert!(
            validate_install_destination(&installer, root, "eef")
                .unwrap_err()
                .to_string()
                .contains("dedicated installation folder")
        );
    }

    #[test]
    fn folder_preflight_rejects_the_other_role_before_a_setup_lock_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let installer = downloads.join("setup.exe");
        for (installed, incoming) in [("eef", "eefn"), ("eefn", "eef")] {
            let destination = dir.path().join(installed);
            fs::create_dir(&destination).unwrap();
            let manifest =
                serde_json::to_vec(&serde_json::json!({"name":installed,"version":"old"})).unwrap();
            fs::write(destination.join("bundle.json"), &manifest).unwrap();
            let error =
                validate_install_destination(&installer, &destination, incoming).unwrap_err();
            assert!(error.to_string().contains("Choose a separate folder"));
            assert_eq!(fs::read(destination.join("bundle.json")).unwrap(), manifest);
            assert_eq!(fs::read_dir(destination).unwrap().count(), 1);
        }
    }

    #[cfg(windows)]
    #[test]
    fn download_folder_check_handles_canonical_windows_prefixes_and_case() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).unwrap();
        let installer = downloads.canonicalize().unwrap().join("setup.exe");
        assert!(reject_download_directory(&installer, &downloads).is_err());
        assert!(reject_download_directory(&installer, &dir.path().join("downloads/eef")).is_err());
        assert!(reject_download_directory(&installer, &dir.path().join("eef")).is_ok());
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
