//! Cooperating setup/service ownership, not authorization or execution isolation.
use anyhow::{Context, Result, bail, ensure};
use std::{fs::File, path::Path};

pub const LOCK_NAME: &str = ".package-use.lock";

/// A persistent lock file is never removed on release. In particular, services
/// using separate configurations still share the same installation ownership.
pub struct Guard {
    _file: File,
}

/// Keep outside the service runtime, including its shutdown and destruction.
/// Inspect before creating even the ownership file, then recheck under the lock.
pub fn for_service(installation: &Path, role: &str) -> Result<Guard> {
    let state = crate::setup_state::startup_state(installation, role).context(
        "Setup state could not be verified. Run the matching installer in this folder before starting the app",
    )?;
    if state == crate::setup_state::StartupState::Uncommitted {
        bail!(
            "Setup did not finish. Run the matching installer in this folder before starting the app"
        )
    }
    let file = open(installation, false)?;
    fs2::FileExt::try_lock_shared(&file).context(
        "Cannot acquire installation access. Wait for setup to finish and check folder permissions before starting the app",
    )?;
    verify_identity(installation, &file)?;
    crate::setup_state::require_startup_ready(installation, role)?;
    Ok(Guard { _file: file })
}

/// Setup owns the update lock first. Release this guard after transaction finish
/// and before launching the installed app; the update lock may remain held.
pub fn for_setup(installation: &Path) -> Result<Guard> {
    let file = open(installation, true)?;
    fs2::FileExt::try_lock_exclusive(&file).context(
        "Cannot acquire exclusive installation access. Close running apps and check folder permissions before installing or recovering this folder",
    )?;
    verify_identity(installation, &file)?;
    Ok(Guard { _file: file })
}

fn options(write: bool) -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(write);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        // Readers and writers may open the stable file, but nobody may replace
        // or remove its pathname while any cooperating process owns a handle.
        options
            .share_mode(3)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options
}

fn open(installation: &Path, write: bool) -> Result<File> {
    let root = installation
        .canonicalize()
        .context("Installation folder is unavailable")?;
    ensure!(root.is_dir(), "Installation root is not a folder");
    let path = root.join(LOCK_NAME);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => regular(&path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match options(true).create_new(true).open(&path) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    regular(&path)?;
                }
                Err(error) => return Err(error).context(
                    "Cannot create installation ownership metadata; use a writable dedicated folder or run the matching installer",
                ),
            }
        }
        Err(error) => return Err(error).context("Cannot inspect installation ownership metadata"),
    }
    // A service can use an existing guard in a read-only installation. Setup
    // requires write access; neither path truncates existing control bytes.
    options(write)
        .open(&path)
        .context("Cannot open installation ownership metadata")
}

fn verify_identity(installation: &Path, file: &File) -> Result<()> {
    let path = installation.canonicalize()?.join(LOCK_NAME);
    regular(&path)?;
    let current = options(false).open(&path)?;
    ensure!(
        crate::setup_journal_format::file_id(file)?
            == crate::setup_journal_format::file_id(&current)?,
        "Installation ownership metadata changed during admission"
    );
    Ok(())
}

fn regular(path: &Path) -> Result<()> {
    ensure!(
        std::fs::symlink_metadata(path)?.file_type().is_file(),
        "Installation ownership metadata must be a regular file"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_share_installation_until_the_last_service_exits() {
        let root = tempfile::tempdir().unwrap();
        let first = for_service(root.path(), "eef").unwrap();
        let second = for_service(root.path(), "eefn").unwrap();
        assert!(for_setup(root.path()).is_err());
        drop(first);
        assert!(for_setup(root.path()).is_err());
        drop(second);
        assert!(for_setup(root.path()).is_ok());
        assert!(root.path().join(LOCK_NAME).is_file());
    }

    #[test]
    fn setup_refuses_startup_and_does_not_use_the_update_lock_as_a_lifetime_lock() {
        let root = tempfile::tempdir().unwrap();
        let updater = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.path().join(".update.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&updater).unwrap();
        let setup = for_setup(root.path()).unwrap();
        assert!(for_service(root.path(), "eef").is_err());
        drop(setup);
        // The normal installer can retain its update lock while launching.
        assert!(for_service(root.path(), "eef").is_ok());
    }

    #[test]
    fn unfinished_setup_is_rejected_without_creating_a_guard() {
        use crate::setup_journal_format::{Header, Record, encode};
        let root = tempfile::tempdir().unwrap();
        let record = Record::Header {
            header: Header {
                version: 1,
                transaction_id: "0123456789abcdef0123456789abcdef".into(),
                root: root.path().canonicalize().unwrap(),
                role: "eefn".into(),
                external: vec![],
            },
        };
        let journal = root.path().join(crate::setup_state::JOURNAL_NAME);
        let bytes = encode(&record).unwrap();
        std::fs::write(&journal, &bytes).unwrap();
        assert!(for_service(root.path(), "eefn").is_err());
        assert_eq!(std::fs::read(journal).unwrap(), bytes);
        assert!(!root.path().join(LOCK_NAME).exists());
    }

    #[test]
    fn existing_control_bytes_and_read_only_service_access_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(LOCK_NAME);
        std::fs::write(&path, b"existing control bytes").unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut read_only = original.clone();
        read_only.set_readonly(true);
        std::fs::set_permissions(&path, read_only).unwrap();
        let service = for_service(root.path(), "eefn").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"existing control bytes");
        drop(service);
        std::fs::set_permissions(path, original).unwrap();
    }

    #[test]
    fn recovery_cannot_replace_ownership_or_update_control_paths() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let header = crate::setup_journal_format::Header {
            version: 1,
            transaction_id: "0123456789abcdef0123456789abcdef".into(),
            root: root.clone(),
            role: "eefn".into(),
            // Even a malformed external allowlist cannot override the control.
            external: vec![root.join(LOCK_NAME)],
        };
        for name in [
            LOCK_NAME,
            ".PACKAGE-USE.LOCK",
            ".package-use.lock/child",
            ".update.lock",
        ] {
            assert!(header.check_path(&root.join(name)).is_err());
        }
        assert!(header.check_path(&root.join("eefn.exe")).is_ok());
    }

    #[test]
    fn invalid_guard_and_unknown_role_are_not_silent_unprotected_startup() {
        let root = tempfile::tempdir().unwrap();
        assert!(for_service(root.path(), "unknown").is_err());
        assert!(!root.path().join(LOCK_NAME).exists());
        std::fs::create_dir(root.path().join(LOCK_NAME)).unwrap();
        assert!(for_service(root.path(), "eef").is_err());
        assert!(for_setup(root.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn active_guard_cannot_be_replaced_or_deleted_on_windows() {
        let root = tempfile::tempdir().unwrap();
        let guard = for_service(root.path(), "eefn").unwrap();
        let path = root.path().join(LOCK_NAME);
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&path, root.path().join("other")).is_err());
        drop(guard);
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_guard_is_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("owner-file");
        std::fs::write(&target, b"owner data").unwrap();
        std::os::unix::fs::symlink(&target, root.path().join(LOCK_NAME)).unwrap();
        assert!(for_service(root.path(), "eefn").is_err());
        assert!(for_setup(root.path()).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"owner data");
    }
}
