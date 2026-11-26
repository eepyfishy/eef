//! Bounded, checksummed append records. Checksums are not authorization.
use super::{Change, FileId, JOURNAL_NAME, Stage, file_id};
use crate::setup_journal_format::{
    Header, MAX_CHANGES, MAX_JOURNAL, MAX_RECORD, encode, parse, regular,
};
pub(super) use crate::setup_journal_format::{Record, location};
use anyhow::{Context, Result, ensure};
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) struct Journal {
    path: PathBuf,
    file: File,
    identity: FileId,
    pub header: Header,
    poisoned: bool,
}

pub(super) struct Loaded {
    pub journal: Journal,
    pub changes: Vec<Change>,
    pub committed: bool,
    pub stages: Vec<Stage>,
}

pub(super) fn probe_identity(parent: &Path) -> Result<()> {
    let parent = parent
        .canonicalize()
        .context("Resolve setup identity probe folder")?;
    let source = tempfile::Builder::new()
        .prefix(".eef-setup-probe-")
        .tempfile_in(&parent)?;
    let before = file_id(source.as_file())?;
    let target = tempfile::Builder::new()
        .prefix(".eef-setup-probe-")
        .tempfile_in(&parent)?
        .into_temp_path();
    fs::remove_file(&target)?;
    let moved = source
        .persist_noclobber(&target)
        .map_err(|error| error.error)?;
    ensure!(
        file_id(&moved)? == before,
        "This filesystem changes file identities on rename; choose another installation location"
    );
    Ok(())
}

pub(super) fn pending_role(root: &Path) -> Result<Option<String>> {
    let path = root.join(JOURNAL_NAME);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => (),
    }
    let parsed = parse(&path)?;
    parsed.header.check_base(root)?;
    Ok(Some(parsed.header.role))
}

impl Journal {
    pub fn create(root: &Path, role: &str, external: &[&Path]) -> Result<Self> {
        probe_identity(root)?;
        let header = Header {
            version: 1,
            transaction_id: hex::encode(rand::random::<[u8; 16]>()),
            root: root.canonicalize()?,
            role: role.to_string(),
            external: external
                .iter()
                .map(|path| location(path))
                .collect::<Result<_>>()?,
        };
        header.check_base(root)?;
        let path = header.root.join(JOURNAL_NAME);
        crate::atomic_file::create(
            &path,
            &encode(&Record::Header {
                header: header.clone(),
            })?,
        )
        .context("Create setup recovery journal")?;
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3);
        }
        let file = options.open(&path)?;
        let identity = file_id(&file)?;
        Ok(Self {
            path,
            file,
            identity,
            header,
            poisoned: false,
        })
    }

    pub fn load(root: &Path, role: &str, external: &[&Path]) -> Result<Option<Loaded>> {
        let path = root.join(JOURNAL_NAME);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => (),
        }
        let parsed = parse(&path)?;
        parsed.header.check_base(root)?;
        ensure!(
            parsed.header.role == role,
            "Use the matching role's installer to recover this setup"
        );
        let expected = external
            .iter()
            .map(|path| location(path))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            parsed.header.external == expected,
            "Setup recovery shortcuts belong to another location or Windows account"
        );
        for stage in &parsed.stages {
            parsed.header.check_stage(stage)?;
        }
        for change in &parsed.changes {
            parsed.header.check_change(change, &parsed.stages)?;
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3);
        }
        let file = options.open(&path)?;
        let identity = file_id(&file)?;
        ensure!(
            identity == parsed.identity && file.metadata()?.len() == parsed.length,
            "Setup journal changed during validation"
        );
        // A partial final append cannot authorize a mutation. Discard only that
        // tail, after the complete journal and all recovery scopes validated.
        file.set_len(parsed.valid_bytes)?;
        file.sync_all()?;
        Ok(Some(Loaded {
            journal: Self {
                path,
                file,
                identity,
                header: parsed.header,
                poisoned: false,
            },
            changes: parsed.changes,
            stages: parsed.stages,
            committed: parsed.committed,
        }))
    }

    pub fn check_path(&self, path: &Path) -> Result<()> {
        self.header.check_path(path)
    }
    pub fn check_stage(&self, stage: &Stage) -> Result<()> {
        self.header.check_stage(stage)
    }

    pub fn append(&mut self, record: &Record) -> Result<()> {
        if let Record::Prepared { index, .. } | Record::Staging { index, .. } = record {
            ensure!(
                *index < MAX_CHANGES,
                "Setup transaction exceeds its entry limit"
            );
        }
        ensure!(
            !self.poisoned,
            "Setup journal needs recovery before more records can be written"
        );
        let frame = encode(record)?;
        let start = self.file.seek(SeekFrom::End(0))?;
        ensure!(
            start + frame.len() as u64 <= MAX_JOURNAL,
            "Setup journal exceeds its size limit"
        );
        let result = self
            .file
            .write_all(&frame)
            .and_then(|_| self.file.sync_all());
        if let Err(error) = result {
            if self
                .file
                .set_len(start)
                .and_then(|_| self.file.sync_all())
                .is_err()
            {
                self.poisoned = true;
            }
            return Err(error).context("Persist setup recovery record before continuing");
        }
        Ok(())
    }

    pub fn remove(self) -> Result<()> {
        let Self {
            path,
            file,
            identity,
            ..
        } = self;
        drop(file);
        regular(&path)?;
        ensure!(
            file_id(&File::open(&path)?)? == identity,
            "Setup journal changed; it was kept"
        );
        fs::remove_file(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Transaction, fingerprint, recover_pending};
    use super::*;
    use sha2::{Digest, Sha256};

    fn abandon(root: &Path) -> (PathBuf, PathBuf) {
        let app = root.join("app");
        fs::write(&app, b"old app").unwrap();
        let mut transaction = Transaction::persistent(root, "eefn", &[]).unwrap();
        transaction.write(&app, b"new app").unwrap();
        let backup = transaction.changes[0]
            .original
            .as_ref()
            .unwrap()
            .backup
            .clone();
        transaction.finished = true; // Test-only simulated termination; no Drop rollback.
        drop(transaction);
        (app, backup)
    }

    #[test]
    fn incomplete_final_append_is_read_only_in_preflight_and_recoverable_afterward() {
        for tail in [vec![0_u8, 1], vec![100_u8, 0, 0, 0, b'{', b'x']] {
            let root = tempfile::tempdir().unwrap();
            let (app, backup) = abandon(root.path());
            let path = root.path().join(JOURNAL_NAME);
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(&tail).unwrap();
            file.sync_all().unwrap();
            drop(file);
            let before = fs::read(&path).unwrap();
            assert_eq!(pending_role(root.path()).unwrap().as_deref(), Some("eefn"));
            assert_eq!(fs::read(&path).unwrap(), before);
            assert!(recover_pending(root.path(), "eefn", &[]).unwrap());
            assert_eq!(fs::read(app).unwrap(), b"old app");
            assert!(!backup.exists());
            assert!(!path.exists());
        }
    }

    #[test]
    fn corruption_or_unknown_record_blocks_recovery_before_any_file_is_changed() {
        for unknown in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (app, backup) = abandon(root.path());
            let path = root.path().join(JOURNAL_NAME);
            if unknown {
                let body = br#"{"kind":"future_authority_override"}"#;
                let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
                file.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
                file.write_all(body).unwrap();
                file.write_all(&Sha256::digest(body)).unwrap();
                file.sync_all().unwrap();
            } else {
                let mut bytes = fs::read(&path).unwrap();
                let end = bytes.len() - 1;
                bytes[end] ^= 1;
                fs::write(&path, bytes).unwrap();
            }
            let before = fs::read(&path).unwrap();
            assert!(recover_pending(root.path(), "eefn", &[]).is_err());
            assert_eq!(fs::read(&app).unwrap(), b"new app");
            assert_eq!(fs::read(backup).unwrap(), b"old app");
            assert_eq!(fs::read(path).unwrap(), before);
        }
    }

    #[test]
    fn later_validly_checksummed_out_of_scope_record_cannot_trigger_earlier_undo() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        fs::write(&app, b"old app").unwrap();
        let owner = outside.path().join("owner.data");
        fs::write(&owner, b"outside owner data").unwrap();
        let mut transaction = Transaction::persistent(root.path(), "eefn", &[]).unwrap();
        transaction.write(&app, b"new app").unwrap();
        let mut forged = transaction.changes[0].clone();
        forged.path = location(&owner).unwrap();
        forged.applied = false;
        forged.original.as_mut().unwrap().moved = false;
        transaction
            .journal
            .as_mut()
            .unwrap()
            .append(&Record::Prepared {
                index: 1,
                change: forged,
            })
            .unwrap();
        transaction.finished = true;
        drop(transaction);
        let path = root.path().join(JOURNAL_NAME);
        let before = fs::read(&path).unwrap();
        let error = recover_pending(root.path(), "eefn", &[]).unwrap_err();
        assert!(error.to_string().contains("outside this installation"));
        assert_eq!(fs::read(&app).unwrap(), b"new app");
        assert_eq!(fs::read(&owner).unwrap(), b"outside owner data");
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn role_root_and_shortcut_bindings_are_checked_before_restore() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let (app, _) = abandon(root.path());
        assert!(recover_pending(root.path(), "eef", &[]).is_err());
        assert!(
            recover_pending(root.path(), "eefn", &[&other.path().join("startup.cmd")]).is_err()
        );
        fs::copy(
            root.path().join(JOURNAL_NAME),
            other.path().join(JOURNAL_NAME),
        )
        .unwrap();
        fs::write(other.path().join("owner"), b"other installation").unwrap();
        assert!(recover_pending(other.path(), "eefn", &[]).is_err());
        assert_eq!(
            fs::read(other.path().join("owner")).unwrap(),
            b"other installation"
        );
        assert_eq!(fs::read(&app).unwrap(), b"new app");
        assert!(recover_pending(root.path(), "eefn", &[]).unwrap());
        assert_eq!(fs::read(app).unwrap(), b"old app");
    }

    #[test]
    fn identical_bytes_in_a_different_owner_file_are_not_treated_as_setup_output() {
        let root = tempfile::tempdir().unwrap();
        let (app, backup) = abandon(root.path());
        let setup_identity = fingerprint(&app).unwrap().identity;
        let owner = root.path().join("owner-replacement");
        fs::write(&owner, b"new app").unwrap();
        let owner_identity = fingerprint(&owner).unwrap().identity;
        assert_ne!(setup_identity, owner_identity);
        fs::rename(&owner, &app).unwrap();
        assert!(recover_pending(root.path(), "eefn", &[]).is_err());
        assert_eq!(fingerprint(&app).unwrap().identity, owner_identity);
        assert_eq!(fs::read(&app).unwrap(), b"new app");
        assert_eq!(fs::read(backup).unwrap(), b"old app");
        assert!(root.path().join(JOURNAL_NAME).is_file());
    }

    #[test]
    fn oversized_or_invalid_frame_lengths_are_rejected_without_restore() {
        for length in [0_u32, 1, (MAX_RECORD + 1) as u32, u32::MAX] {
            let root = tempfile::tempdir().unwrap();
            let (app, backup) = abandon(root.path());
            let path = root.path().join(JOURNAL_NAME);
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(&length.to_le_bytes()).unwrap();
            file.sync_all().unwrap();
            drop(file);
            assert!(recover_pending(root.path(), "eefn", &[]).is_err());
            assert_eq!(fs::read(app).unwrap(), b"new app");
            assert_eq!(fs::read(backup).unwrap(), b"old app");
        }
    }

    #[test]
    fn writer_refuses_entry_overflow_before_persisting_an_unrecoverable_record() {
        let root = tempfile::tempdir().unwrap();
        let mut transaction = Transaction::persistent(root.path(), "eefn", &[]).unwrap();
        transaction
            .write(&root.path().join("new-file"), b"fixture")
            .unwrap();
        let journal = transaction.journal.as_mut().unwrap();
        let before = fs::read(&journal.path).unwrap();
        assert!(
            journal
                .append(&Record::Staging {
                    index: MAX_CHANGES,
                    stage: transaction.stages[0].clone()
                })
                .is_err()
        );
        assert_eq!(fs::read(&journal.path).unwrap(), before);
        assert!(
            transaction
                .finish(Err(anyhow::anyhow!("later failure")))
                .is_err()
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
