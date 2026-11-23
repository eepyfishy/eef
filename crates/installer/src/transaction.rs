//! Roll back ordinary setup errors without copying installed runtimes.
//! This is not automatic recovery after process termination or power loss.
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Original {
    backup: PathBuf,
    digest: [u8; 32],
    moved: bool,
}

#[derive(Debug)]
struct Change {
    path: PathBuf,
    original: Option<Original>,
    after: Option<[u8; 32]>,
    applied: bool,
}

#[derive(Debug)]
pub struct Transaction {
    changes: Vec<Change>,
    finished: bool,
}

fn digest(path: &Path) -> Result<[u8; 32]> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_file(),
        "Setup requires a regular file: {}",
        path.display()
    );
    let mut input = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().into())
}

fn observed(path: &Path) -> Result<Option<[u8; 32]>> {
    match digest(path) {
        Ok(digest) => Ok(Some(digest)),
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            finished: false,
        }
    }

    fn prepare_change(
        &mut self,
        path: &Path,
        before: Option<[u8; 32]>,
        after: Option<[u8; 32]>,
    ) -> Result<usize> {
        let current = observed(path)?;
        if before.is_none() && current.is_some() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists).into());
        }
        ensure!(
            current == before,
            "This file changed during setup; its current contents were kept: {}",
            path.display()
        );
        let original = if let Some(digest) = before {
            ensure!(
                !fs::metadata(path)?.permissions().readonly(),
                "This file is read-only: {}",
                path.display()
            );
            // Reserve an owned, unique same-directory name before moving the old
            // file. Keeping the TempPath prevents Drop deleting recovery bytes.
            let prefix = format!(
                ".eef-setup-{}-",
                path.file_name()
                    .context("Missing setup filename")?
                    .to_string_lossy()
            );
            let pending = tempfile::Builder::new()
                .prefix(&prefix)
                .suffix(".backup")
                .tempfile_in(path.parent().context("Missing setup parent")?)?;
            let (file, backup) = pending.into_parts();
            drop(file);
            let backup = backup.keep().map_err(|error| error.error)?;
            Some(Original {
                backup,
                digest,
                moved: false,
            })
        } else {
            None
        };
        let index = self.changes.len();
        self.changes.push(Change {
            path: path.to_path_buf(),
            original,
            after,
            applied: false,
        });
        if let Some(original) = &mut self.changes[index].original {
            fs::rename(path, &original.backup)
                .with_context(|| format!("Preserve previous file: {}", path.display()))?;
            original.moved = true;
        }
        Ok(index)
    }

    fn publish(
        &mut self,
        path: &Path,
        pending: tempfile::TempPath,
        before: Option<[u8; 32]>,
    ) -> Result<()> {
        let after = digest(&pending)?;
        if before == Some(after) {
            ensure!(
                observed(path)? == before,
                "This file changed during setup; its current contents were kept: {}",
                path.display()
            );
            return Ok(());
        }
        let index = self.prepare_change(path, before, Some(after))?;
        // Never replace a file created by somebody else after the old file was
        // moved. A failed publication is recovered with the rest of setup.
        pending
            .persist_noclobber(path)
            .map_err(|failure| failure.error)?;
        self.changes[index].applied = true;
        Ok(())
    }

    fn write_with(
        &mut self,
        path: &Path,
        create_only: bool,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<()> {
        let before = observed(path)?;
        if create_only && before.is_some() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists).into());
        }
        fs::create_dir_all(path.parent().context("Missing setup parent")?)?;
        let mut pending = tempfile::Builder::new()
            .prefix(".eef-setup-stage-")
            .tempfile_in(path.parent().unwrap())?;
        write(pending.as_file_mut())?;
        pending.as_file().sync_all()?;
        self.publish(path, pending.into_temp_path(), before)
    }

    pub fn replace_with(
        &mut self,
        path: &Path,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<()> {
        self.write_with(path, false, write)
    }

    pub fn create_with(
        &mut self,
        path: &Path,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<()> {
        self.write_with(path, true, write)
    }

    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        self.replace_with(path, |file| file.write_all(bytes))
    }

    pub fn edit(&mut self, path: &Path, prepare: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        let before = observed(path)?;
        fs::create_dir_all(path.parent().context("Missing settings parent")?)?;
        let pending = tempfile::Builder::new()
            .prefix(".eef-setup-stage-")
            .tempfile_in(path.parent().unwrap())?
            .into_temp_path();
        if before.is_some() {
            let mut source = File::open(path)?;
            let mut stage = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&pending)?;
            io::copy(&mut source, &mut stage)?;
        } else {
            fs::remove_file(&pending)?;
        }
        prepare(&pending)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&pending)?
            .sync_all()?;
        self.publish(path, pending, before)
    }

    pub fn remove(&mut self, path: &Path) -> Result<()> {
        let Some(before) = observed(path)? else {
            return Ok(());
        };
        let index = self.prepare_change(path, Some(before), None)?;
        self.changes[index].applied = true;
        Ok(())
    }

    pub fn preserve_selection(&mut self, from: &Path, to: &Path) -> Result<()> {
        let before = digest(from)?;
        let mut source = File::open(from)?;
        self.create_with(to, |file| io::copy(&mut source, file).map(|_| ()))?;
        ensure!(
            digest(to)? == before,
            "The update selection changed while it was being preserved"
        );
        let index = self.prepare_change(from, Some(before), None)?;
        self.changes[index].applied = true;
        Ok(())
    }

    fn undo(change: &Change) -> Result<()> {
        let original = change.original.as_ref().filter(|original| original.moved);
        if !change.applied && original.is_none() {
            return Ok(());
        }
        let expected = if change.applied { change.after } else { None };
        ensure!(
            observed(&change.path)? == expected,
            "A file changed after setup wrote it; current contents were kept: {}",
            change.path.display()
        );
        if let Some(original) = original {
            ensure!(
                digest(&original.backup)? == original.digest,
                "A setup backup changed; it was kept for recovery: {}",
                original.backup.display()
            );
            fs::rename(&original.backup, &change.path).with_context(|| {
                format!(
                    "Restore {} from {}",
                    change.path.display(),
                    original.backup.display()
                )
            })?;
        } else if change.applied && change.after.is_some() {
            fs::remove_file(&change.path)?;
        }
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        for change in self.changes.iter().rev() {
            if let Err(error) = Self::undo(change) {
                let backup = change
                    .original
                    .as_ref()
                    .map(|original| original.backup.display().to_string())
                    .unwrap_or_else(|| "no prior file".into());
                failures.push(format!("{error:#}; recovery backup: {backup}"));
            } else if let Some(original) =
                change.original.as_ref().filter(|original| !original.moved)
            {
                // A failed move left only our reserved empty backup file.
                let remove = (|| -> Result<()> {
                    ensure!(
                        digest(&original.backup)? == <[u8; 32]>::from(Sha256::digest([])),
                        "Unused backup file changed; it was kept"
                    );
                    fs::remove_file(&original.backup)?;
                    Ok(())
                })();
                if let Err(error) = remove {
                    failures.push(format!(
                        "Remove unused backup {}: {error}",
                        original.backup.display()
                    ));
                }
            }
        }
        self.finished = true;
        if !failures.is_empty() {
            bail!(
                "Setup rollback needs attention. Recovery backups were kept.\n{}",
                failures.join("\n")
            );
        }
        Ok(())
    }

    pub fn commit(&mut self) -> Vec<String> {
        self.finished = true;
        let mut warnings = Vec::new();
        for original in self
            .changes
            .iter()
            .filter_map(|change| change.original.as_ref())
        {
            let remove = (|| -> Result<()> {
                ensure!(
                    digest(&original.backup)? == original.digest,
                    "Previous-file backup changed; it was kept"
                );
                fs::remove_file(&original.backup)?;
                Ok(())
            })();
            if let Err(error) = remove {
                warnings.push(format!("{}: {error}", original.backup.display()));
            }
        }
        warnings
    }

    pub fn finish(&mut self, result: Result<()>) -> Result<Vec<String>> {
        match result {
            Ok(()) => Ok(self.commit()),
            Err(error) if self.changes.is_empty() => {
                self.finished = true;
                Err(error)
            }
            Err(error) => match self.rollback() {
                Ok(()) => {
                    Err(error.context("Setup did not finish; its file changes were rolled back"))
                }
                Err(rollback) => Err(error.context(format!("{rollback:#}"))),
            },
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            // Best effort during unwinding. Backups are ordinary kept paths,
            // never TempPaths whose Drop could erase the only recovery copy.
            let _ = self.rollback();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn late_failure_restores_multiple_edits_deletions_and_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        let settings = dir.path().join("settings");
        let startup = dir.path().join("startup");
        let added = dir.path().join("added");
        for (path, contents) in [
            (&app, b"old app".as_slice()),
            (&settings, b"owner settings"),
            (&startup, b"old startup"),
        ] {
            fs::write(path, contents).unwrap();
        }
        let mut transaction = Transaction::new();
        transaction.write(&app, b"new app").unwrap();
        transaction.write(&settings, b"new defaults").unwrap();
        transaction.write(&settings, b"generated secret").unwrap();
        transaction.remove(&startup).unwrap();
        transaction.write(&added, b"new runtime").unwrap();
        let error = transaction
            .finish(Err(anyhow::anyhow!("later shortcut failure")))
            .unwrap_err();
        assert!(format!("{error:#}").contains("rolled back"));
        assert_eq!(fs::read(app).unwrap(), b"old app");
        assert_eq!(fs::read(settings).unwrap(), b"owner settings");
        assert_eq!(fs::read(startup).unwrap(), b"old startup");
        assert!(!added.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn partial_staging_failure_rolls_back_earlier_published_files() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::write(&first, b"old first").unwrap();
        fs::write(&second, b"old second").unwrap();
        let mut transaction = Transaction::new();
        transaction.write(&first, b"new first").unwrap();
        let result = transaction.replace_with(&second, |file| {
            file.write_all(b"partial second")?;
            Err(io::Error::from(io::ErrorKind::WriteZero))
        });
        assert!(transaction.finish(result).is_err());
        assert_eq!(fs::read(first).unwrap(), b"old first");
        assert_eq!(fs::read(second).unwrap(), b"old second");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn concurrently_created_settings_are_kept_and_do_not_become_setup_changes() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings");
        let other = dir.path().join("other");
        fs::write(&other, b"old other").unwrap();
        let mut transaction = Transaction::new();
        transaction.write(&other, b"new other").unwrap();
        let error = transaction
            .create_with(&settings, |file| {
                fs::write(&settings, b"owner creation")?;
                file.write_all(b"setup defaults")
            })
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(
            transaction
                .finish(Err(anyhow::anyhow!("later failure")))
                .is_err()
        );
        assert_eq!(fs::read(settings).unwrap(), b"owner creation");
        assert_eq!(fs::read(other).unwrap(), b"old other");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn observed_owner_changes_block_restore_and_keep_the_original_backup() {
        let dir = tempfile::tempdir().unwrap();
        let changed = dir.path().join("changed");
        let other = dir.path().join("other");
        fs::write(&changed, b"old changed").unwrap();
        fs::write(&other, b"old other").unwrap();
        let backup;
        {
            let mut transaction = Transaction::new();
            transaction.write(&changed, b"setup changed").unwrap();
            backup = transaction.changes[0]
                .original
                .as_ref()
                .unwrap()
                .backup
                .clone();
            transaction.write(&other, b"setup other").unwrap();
            fs::write(&changed, b"later owner edit").unwrap();
            let error = transaction
                .finish(Err(anyhow::anyhow!("late failure")))
                .unwrap_err();
            assert!(format!("{error:#}").contains("Recovery backups were kept"));
            assert!(format!("{error:#}").contains(&backup.display().to_string()));
        }
        assert_eq!(fs::read(changed).unwrap(), b"later owner edit");
        assert_eq!(fs::read(other).unwrap(), b"old other");
        assert_eq!(fs::read(backup).unwrap(), b"old changed");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn commit_preserves_a_backup_changed_through_an_earlier_open_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings");
        fs::write(&path, b"old settings").unwrap();
        let mut earlier_writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
        let mut transaction = Transaction::new();
        transaction.write(&path, b"setup settings").unwrap();
        let backup = transaction.changes[0]
            .original
            .as_ref()
            .unwrap()
            .backup
            .clone();
        earlier_writer.write_all(b"owner update").unwrap();
        earlier_writer.sync_all().unwrap();
        let warnings = transaction.finish(Ok(())).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("backup changed"));
        assert!(warnings[0].contains(&backup.display().to_string()));
        assert_eq!(fs::read(path).unwrap(), b"setup settings");
        assert_eq!(fs::read(backup).unwrap(), b"owner update");
    }

    #[test]
    fn settings_are_prepared_privately_and_concurrent_edits_are_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings");
        fs::write(&path, b"owner settings").unwrap();
        let mut transaction = Transaction::new();
        let result = transaction.edit(&path, |stage| {
            assert_eq!(fs::read(&path)?, b"owner settings");
            fs::write(stage, b"prepared settings")?;
            fs::write(&path, b"concurrent settings")?;
            Ok(())
        });
        assert!(transaction.finish(result).is_err());
        assert_eq!(fs::read(path).unwrap(), b"concurrent settings");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn unwinding_restores_changes_and_commit_keeps_them_without_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app");
        fs::write(&path, b"old app").unwrap();
        assert!(
            std::panic::catch_unwind(|| {
                let mut transaction = Transaction::new();
                transaction.write(&path, b"abandoned app").unwrap();
                panic!("synthetic setup unwind");
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"old app");
        let mut transaction = Transaction::new();
        transaction.write(&path, b"committed app").unwrap();
        assert!(transaction.finish(Ok(())).unwrap().is_empty());
        drop(transaction);
        assert_eq!(fs::read(path).unwrap(), b"committed app");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn refused_rollback_keeps_both_the_new_file_and_recovery_backup() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app");
        fs::write(&path, b"old app").unwrap();
        let mut transaction = Transaction::new();
        transaction.write(&path, b"new app").unwrap();
        let backup = transaction.changes[0]
            .original
            .as_ref()
            .unwrap()
            .backup
            .clone();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let error = transaction
            .finish(Err(anyhow::anyhow!("later failure")))
            .unwrap_err();
        assert!(format!("{error:#}").contains("Recovery backups were kept"));
        assert_eq!(fs::read(&path).unwrap(), b"new app");
        assert_eq!(fs::read(&backup).unwrap(), b"old app");
        drop(guard);
    }

    #[cfg(windows)]
    #[test]
    fn backup_cleanup_failure_is_a_committed_install_warning() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app");
        fs::write(&path, b"old app").unwrap();
        let mut transaction = Transaction::new();
        transaction.write(&path, b"new app").unwrap();
        let backup = transaction.changes[0]
            .original
            .as_ref()
            .unwrap()
            .backup
            .clone();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&backup)
            .unwrap();
        let warnings = transaction.finish(Ok(())).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains(&backup.display().to_string()));
        drop(transaction);
        assert_eq!(fs::read(&path).unwrap(), b"new app");
        assert_eq!(fs::read(&backup).unwrap(), b"old app");
        drop(guard);
    }
}
