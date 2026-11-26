//! Roll back ordinary setup errors without copying installed runtimes.
//! Journaled setup recovery. Directory durability after power loss is separate.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
#[path = "setup_journal.rs"]
mod journal;
pub const JOURNAL_NAME: &str = crate::setup_state::JOURNAL_NAME;

pub fn pending_role(root: &Path) -> Result<Option<String>> {
    journal::pending_role(root)
}

pub fn recover_pending(root: &Path, role: &str, external: &[&Path]) -> Result<bool> {
    let Some(loaded) = journal::Journal::load(root, role, external)? else {
        return Ok(false);
    };
    let mut transaction = Transaction {
        changes: loaded.changes,
        stages: loaded.stages,
        finished: loaded.committed,
        journal: Some(loaded.journal),
        verified_volumes: Vec::new(),
    };
    if loaded.committed {
        let warnings = transaction.cleanup_committed();
        ensure!(
            warnings.is_empty(),
            "Previous setup completed, but recovery backup cleanup needs attention:\n{}",
            warnings.join("\n")
        );
    } else {
        transaction.rollback()?;
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileId {
    volume: u64,
    identifier: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    digest: [u8; 32],
    identity: FileId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    path: PathBuf,
    identity: FileId,
}

#[cfg(windows)]
fn file_id(file: &File) -> Result<FileId> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } != 0,
        "Cannot read a stable setup file identity: {}",
        io::Error::last_os_error()
    );
    ensure!(
        info.FileId.Identifier.iter().any(|byte| *byte != 0),
        "The filesystem did not supply a setup file identity"
    );
    Ok(FileId {
        volume: info.VolumeSerialNumber,
        identifier: info.FileId.Identifier,
    })
}

#[cfg(unix)]
fn file_id(file: &File) -> Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    let mut identifier = [0_u8; 16];
    identifier[..8].copy_from_slice(&metadata.ino().to_le_bytes());
    Ok(FileId {
        volume: metadata.dev(),
        identifier,
    })
}

#[cfg(not(any(windows, unix)))]
fn file_id(_file: &File) -> Result<FileId> {
    bail!("This platform does not supply setup file identities")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {
    backup: PathBuf,
    digest: [u8; 32],
    moved: bool,
    identity: FileId,
    reserved_identity: FileId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    path: PathBuf,
    original: Option<Original>,
    after: Option<[u8; 32]>,
    applied: bool,
    after_identity: Option<FileId>,
    reverted: bool,
    cleaned: bool,
    #[serde(skip)]
    logged: bool,
    stage: Option<usize>,
}

#[derive(Debug)]
pub struct Transaction {
    changes: Vec<Change>,
    finished: bool,
    journal: Option<journal::Journal>,
    stages: Vec<Stage>,
    verified_volumes: Vec<u64>,
}

fn fingerprint(path: &Path) -> Result<Fingerprint> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_file(),
        "Setup requires a regular file: {}",
        path.display()
    );
    let mut input = File::open(path)?;
    let identity = file_id(&input)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(Fingerprint {
        digest: hash.finalize().into(),
        identity,
    })
}

fn digest(path: &Path) -> Result<[u8; 32]> {
    Ok(fingerprint(path)?.digest)
}

fn observed_file(path: &Path) -> Result<Option<Fingerprint>> {
    match fingerprint(path) {
        Ok(value) => Ok(Some(value)),
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

fn backup_prefix(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .context("Missing setup filename")?
        .to_string_lossy();
    let hash = hex::encode(Sha256::digest(name.as_bytes()));
    Ok(format!(".eef-setup-{}-", &hash[..24]))
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
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
            finished: false,
            journal: None,
            stages: Vec::new(),
            verified_volumes: Vec::new(),
        }
    }

    pub fn persistent(root: &Path, role: &str, external: &[&Path]) -> Result<Self> {
        Ok(Self {
            changes: Vec::new(),
            stages: Vec::new(),
            finished: false,
            journal: Some(journal::Journal::create(root, role, external)?),
            verified_volumes: Vec::new(),
        })
    }

    fn record(&mut self, record: journal::Record) -> Result<()> {
        if let Some(journal) = &mut self.journal {
            journal.append(&record)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn checkpoint(&self, phase: &str, index: usize) {
        if std::env::var("EEF_SETUP_TEST_CHILD").as_deref() != Ok("1")
            || std::env::var("EEF_SETUP_TEST_HALT").as_deref()
                != Ok(format!("{phase}:{index}").as_str())
        {
            return;
        }
        let journal = self
            .journal
            .as_ref()
            .expect("Crash fixture needs a persistent transaction");
        let marker = journal.header.root.join(".setup-test-ready");
        crate::atomic_file::write(&marker, format!("{phase}:{index}").as_bytes()).unwrap();
        loop {
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
    }

    fn prepare_change(
        &mut self,
        path: &Path,
        before: Option<[u8; 32]>,
        after: Option<[u8; 32]>,
        after_identity: Option<FileId>,
        stage: Option<usize>,
    ) -> Result<usize> {
        let canonical = journal::location(path)?;
        let path = canonical.as_path();
        if let Some(journal) = &self.journal {
            journal.check_path(path)?;
        }
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
            let original_file = fingerprint(path)?;
            ensure!(
                original_file.digest == digest,
                "The original file changed while setup checked it"
            );
            ensure!(
                !fs::metadata(path)?.permissions().readonly(),
                "This file is read-only: {}",
                path.display()
            );
            // Reserve an owned, unique same-directory name before moving the old
            // file. Keeping the TempPath prevents Drop deleting recovery bytes.
            let prefix = backup_prefix(path)?;
            let pending = tempfile::Builder::new()
                .prefix(&prefix)
                .suffix(".backup")
                .tempfile_in(path.parent().context("Missing setup parent")?)?;
            let (file, backup) = pending.into_parts();
            let reserved_identity = file_id(&file)?;
            drop(file);
            let backup = backup.keep().map_err(|error| error.error)?;
            Some(Original {
                backup,
                digest,
                moved: false,
                identity: original_file.identity,
                reserved_identity,
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
            after_identity,
            reverted: false,
            cleaned: false,
            logged: false,
            stage,
        });
        self.record(journal::Record::Prepared {
            index,
            change: self.changes[index].clone(),
        })?;
        self.changes[index].logged = true;
        #[cfg(test)]
        self.checkpoint("prepared", index);
        if let Some(original) = &mut self.changes[index].original {
            fs::rename(path, &original.backup)
                .with_context(|| format!("Preserve previous file: {}", path.display()))?;
            original.moved = true;
        }
        #[cfg(test)]
        self.checkpoint("moved", index);
        Ok(index)
    }

    fn publish(
        &mut self,
        path: &Path,
        pending: tempfile::TempPath,
        before: Option<[u8; 32]>,
        stage: usize,
    ) -> Result<()> {
        let staged = fingerprint(&pending)?;
        let after = staged.digest;
        if before == Some(after) {
            ensure!(
                observed(path)? == before,
                "This file changed during setup; its current contents were kept: {}",
                path.display()
            );
            return Ok(());
        }
        ensure!(
            self.stages[stage].identity == staged.identity,
            "Setup staging identity changed"
        );
        let index = self.prepare_change(
            path,
            before,
            Some(after),
            Some(staged.identity),
            Some(stage),
        )?;
        // Never replace a file created by somebody else after the old file was
        // moved. A failed publication is recovered with the rest of setup.
        pending
            .persist_noclobber(&self.changes[index].path)
            .map_err(|failure| failure.error)?;
        self.changes[index].applied = true;
        #[cfg(test)]
        self.checkpoint("published", index);
        self.record(journal::Record::Applied { index })?;
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
        self.write_observed(path, before, write)
    }

    fn write_observed(
        &mut self,
        path: &Path,
        before: Option<[u8; 32]>,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<()> {
        let parent = path.parent().context("Missing setup parent")?;
        fs::create_dir_all(parent)?;
        // tempfile's native Windows calls need the extended path prefix even
        // when only its scratch filename pushes the full path over MAX_PATH.
        let parent = parent
            .canonicalize()
            .context("Resolve setup staging folder")?;
        let mut pending = tempfile::Builder::new()
            .prefix(".eef-setup-stage-")
            .tempfile_in(&parent)?;
        let stage = Stage {
            path: journal::location(pending.path())?,
            identity: file_id(pending.as_file())?,
        };
        if self.journal.is_some() && !self.verified_volumes.contains(&stage.identity.volume) {
            journal::probe_identity(&parent)?;
            self.verified_volumes.push(stage.identity.volume);
        }
        if let Some(journal) = &self.journal {
            journal.check_stage(&stage)?;
        }
        let index = self.stages.len();
        self.record(journal::Record::Staging {
            index,
            stage: stage.clone(),
        })?;
        self.stages.push(stage);
        #[cfg(test)]
        self.checkpoint("staging", index);
        write(pending.as_file_mut())?;
        #[cfg(test)]
        self.checkpoint("written", index);
        pending.as_file().sync_all()?;
        self.publish(path, pending.into_temp_path(), before, index)
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

    pub fn edit(
        &mut self,
        path: &Path,
        prepare: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    ) -> Result<()> {
        let before = observed(path)?;
        let bytes = before.map(|_| fs::read(path)).transpose()?;
        if let Some(prepared) = prepare(bytes.as_deref())? {
            use std::io::Write;
            self.write_observed(path, before, |file| file.write_all(&prepared))?;
        } else {
            ensure!(
                observed(path)? == before,
                "Settings changed during setup; current contents were kept"
            );
        }
        Ok(())
    }

    pub fn remove(&mut self, path: &Path) -> Result<()> {
        let Some(before) = observed(path)? else {
            return Ok(());
        };
        let index = self.prepare_change(path, Some(before), None, None, None)?;
        self.changes[index].applied = true;
        self.record(journal::Record::Applied { index })?;
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
        let index = self.prepare_change(from, Some(before), None, None, None)?;
        self.changes[index].applied = true;
        self.record(journal::Record::Applied { index })?;
        Ok(())
    }

    fn undo(change: &Change) -> Result<()> {
        let current = observed_file(&change.path)?;
        let after = change
            .after
            .zip(change.after_identity)
            .map(|(digest, identity)| Fingerprint { digest, identity });
        if let Some(original) = &change.original {
            let before = Fingerprint {
                digest: original.digest,
                identity: original.identity,
            };
            let reserved = Fingerprint {
                digest: Sha256::digest([]).into(),
                identity: original.reserved_identity,
            };
            match observed_file(&original.backup)? {
                Some(backup) if backup == reserved => {
                    fs::remove_file(&original.backup)?;
                }
                Some(backup) if backup == before => {
                    ensure!(
                        current == after || current.is_none(),
                        "A file changed after setup wrote it; current contents were kept: {}",
                        change.path.display()
                    );
                    fs::rename(&original.backup, &change.path).with_context(|| {
                        format!(
                            "Restore {} from {}",
                            change.path.display(),
                            original.backup.display()
                        )
                    })?;
                }
                None if current == Some(before) => (), // Recovery already restored this file before its progress append.
                _ => bail!(
                    "A setup backup changed or is missing; recovery copies were kept: {}",
                    original.backup.display()
                ),
            }
        } else if current == after {
            fs::remove_file(&change.path)?;
        } else if current.is_some() {
            ensure!(
                !change.applied && current.map(|value| value.identity) != change.after_identity,
                "A file changed after setup wrote it; current contents were kept: {}",
                change.path.display()
            );
        }
        Ok(())
    }

    pub fn rollback(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        for index in (0..self.changes.len()).rev() {
            if self.changes[index].reverted {
                continue;
            }
            if let Err(error) = Self::undo(&self.changes[index]) {
                let change = &self.changes[index];
                let backup = change
                    .original
                    .as_ref()
                    .map(|original| original.backup.display().to_string())
                    .unwrap_or_else(|| "no prior file".into());
                failures.push(format!("{error:#}; recovery backup: {backup}"));
            } else {
                #[cfg(test)]
                self.checkpoint("undone", index);
                self.changes[index].reverted = true;
                if self.changes[index].logged {
                    if let Err(error) = self.record(journal::Record::Undone { index }) {
                        failures.push(format!("Record recovered file: {error:#}"));
                    }
                }
            }
        }
        self.finished = true;
        if failures.is_empty() {
            failures.extend(self.cleanup_stages());
        }
        if failures.is_empty() {
            if let Some(journal) = self.journal.take() {
                if let Err(error) = journal.remove() {
                    failures.push(format!("Remove completed recovery journal: {error:#}"));
                }
            }
        }
        if !failures.is_empty() {
            bail!(
                "Setup rollback needs attention. Recovery backups were kept.\n{}",
                failures.join("\n")
            );
        }
        Ok(())
    }

    pub fn commit(&mut self) -> Result<Vec<String>> {
        self.record(journal::Record::Committed)?;
        self.finished = true;
        #[cfg(test)]
        self.checkpoint("committed", 0);
        Ok(self.cleanup_committed())
    }

    fn cleanup_committed(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        for index in 0..self.changes.len() {
            if self.changes[index].cleaned {
                continue;
            }
            let Some(original) = &self.changes[index].original else {
                continue;
            };
            let remove = (|| -> Result<()> {
                if let Some(backup) = observed_file(&original.backup)? {
                    let before = Fingerprint {
                        digest: original.digest,
                        identity: original.identity,
                    };
                    let reserved = Fingerprint {
                        digest: Sha256::digest([]).into(),
                        identity: original.reserved_identity,
                    };
                    ensure!(
                        backup == before || backup == reserved,
                        "Previous-file backup changed; it was kept"
                    );
                    fs::remove_file(&original.backup)?;
                }
                Ok(())
            })();
            if let Err(error) = remove {
                warnings.push(format!("{}: {error}", original.backup.display()));
            } else {
                #[cfg(test)]
                self.checkpoint("cleaned", index);
                self.changes[index].cleaned = true;
                if self.changes[index].logged {
                    if let Err(error) = self.record(journal::Record::Cleaned { index }) {
                        warnings.push(format!("Record backup cleanup: {error:#}"));
                    }
                }
            }
        }
        warnings.extend(self.cleanup_stages());
        if warnings.is_empty() {
            if let Some(journal) = self.journal.take() {
                if let Err(error) = journal.remove() {
                    warnings.push(format!("Remove completed setup journal: {error:#}"));
                }
            }
        }
        warnings
    }

    fn cleanup_stages(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        for stage in &self.stages {
            let cleanup = (|| -> Result<()> {
                match fs::symlink_metadata(&stage.path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(error.into()),
                    Ok(metadata) => ensure!(
                        metadata.file_type().is_file(),
                        "Staging path is no longer a regular file"
                    ),
                }
                ensure!(
                    file_id(&File::open(&stage.path)?)? == stage.identity,
                    "A different file occupies the staging path; it was kept"
                );
                fs::remove_file(&stage.path)?;
                Ok(())
            })();
            if let Err(error) = cleanup {
                warnings.push(format!(
                    "Keep staging file {}: {error:#}",
                    stage.path.display()
                ));
            }
        }
        warnings
    }

    pub fn finish(&mut self, result: Result<()>) -> Result<Vec<String>> {
        match result {
            Ok(()) => match self.commit() {
                Ok(warnings) => Ok(warnings),
                Err(error) => match self.rollback() {
                    Ok(()) => Err(error
                        .context("Setup commit was not persisted; file changes were rolled back")),
                    Err(rollback) => Err(error.context(format!("{rollback:#}"))),
                },
            },
            Err(error) if self.changes.is_empty() && self.stages.is_empty() => {
                self.finished = true;
                match self
                    .journal
                    .take()
                    .map(|journal| journal.remove())
                    .transpose()
                {
                    Ok(_) => Err(error),
                    Err(recovery) => {
                        Err(error.context(format!("Keep setup journal for recovery: {recovery:#}")))
                    }
                }
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

    #[cfg(windows)]
    fn raw_windows_directory(root: &Path, units: usize) -> PathBuf {
        use std::os::windows::ffi::OsStrExt;
        assert!(matches!(
            root.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_))
        ));
        let mut path = root.to_path_buf();
        loop {
            let length = path.as_os_str().encode_wide().count();
            if length >= units {
                break;
            }
            path.push("d".repeat((units - length).saturating_sub(1).clamp(1, 32)));
        }
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_paths_stage_short_names_and_preserve_rollback_and_commit() {
        use std::os::windows::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let parent = raw_windows_directory(dir.path(), 246);
        let path = parent.join("COPYING");
        assert!(path.as_os_str().encode_wide().count() < 260);
        fs::write(&path, b"owner contents").unwrap();
        let original = fingerprint(&path).unwrap();
        let mut transaction = Transaction::persistent(dir.path(), "eefn", &[]).unwrap();
        transaction.write(&path, b"first replacement").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first replacement");
        transaction.rollback().unwrap();
        assert_eq!(fingerprint(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert!(!dir.path().join(JOURNAL_NAME).exists());
        let mut transaction = Transaction::persistent(dir.path(), "eefn", &[]).unwrap();
        transaction.write(&path, b"committed replacement").unwrap();
        assert!(transaction.commit().unwrap().is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"committed replacement");
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        assert!(!dir.path().join(JOURNAL_NAME).exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_paths_recover_uncommitted_publication_in_a_deep_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = raw_windows_directory(dir.path(), 280);
        let path = root.join("owner-file");
        fs::write(&path, b"owner contents").unwrap();
        let original = fingerprint(&path).unwrap();
        let mut transaction = Transaction::persistent(&root, "eefn", &[]).unwrap();
        transaction
            .write(&path, b"interrupted replacement")
            .unwrap();
        // Simulate a lost process without invoking ordinary Drop rollback.
        transaction.finished = true;
        drop(transaction);
        assert!(root.join(JOURNAL_NAME).is_file());
        assert!(recover_pending(&root, "eefn", &[]).unwrap());
        assert_eq!(fingerprint(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(!root.join(JOURNAL_NAME).exists());
    }

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
    fn maximum_length_file_names_use_bounded_backup_names_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join(format!("{}.dll", "x".repeat(251)));
        fs::write(&path, b"old long-name file").unwrap();
        let mut transaction = Transaction::persistent(&root, "eefn", &[]).unwrap();
        transaction.write(&path, b"new long-name file").unwrap();
        assert!(
            transaction.changes[0]
                .original
                .as_ref()
                .unwrap()
                .backup
                .file_name()
                .unwrap()
                .len()
                < 100
        );
        assert!(
            transaction
                .finish(Err(anyhow::anyhow!("late failure")))
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"old long-name file");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
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
        let result = transaction.edit(&path, |before| {
            assert_eq!(before.unwrap(), b"owner settings");
            assert_eq!(fs::read(&path)?, b"owner settings");
            fs::write(&path, b"concurrent settings")?;
            Ok(Some(b"prepared settings".to_vec()))
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
