//! Bounded, checksummed append records. Checksums are not authorization.
use super::{Change, FileId, JOURNAL_NAME, Stage, backup_prefix, file_id};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

const MAX_RECORD: usize = 256 * 1024;
const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
const MAX_CHANGES: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Header {
    version: u32,
    transaction_id: String,
    pub root: PathBuf,
    role: String,
    external: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Record {
    Header { header: Header },
    Staging { index: usize, stage: Stage },
    Prepared { index: usize, change: Change },
    Applied { index: usize },
    Undone { index: usize },
    Committed,
    Cleaned { index: usize },
}

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

struct Parsed {
    header: Header,
    changes: Vec<Change>,
    committed: bool,
    valid_bytes: u64,
    stages: Vec<Stage>,
    identity: FileId,
    length: u64,
}

/// Resolve the parent, including an existing ancestor of an uncreated path.
/// A planned shortcut may have an invalid file as its parent; writing it still
/// fails at the actual setup step, without pre-creating any directories here.
pub(super) fn location(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute(),
        "Setup recovery requires an absolute path"
    );
    let name = path.file_name().context("Missing setup filename")?;
    let mut parent = path.parent().context("Missing setup parent")?;
    let mut missing = Vec::new();
    loop {
        match parent.canonicalize() {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved.join(name));
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                missing.push(
                    parent
                        .file_name()
                        .context("Missing existing setup ancestor")?
                        .to_os_string(),
                );
                parent = parent.parent().context("Missing existing setup ancestor")?;
            }
            Err(error) => return Err(error).context("Resolve setup recovery location"),
        }
    }
}

fn regular(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Setup journal must be a regular file"
    );
    Ok(())
}

pub(super) fn probe_identity(parent: &Path) -> Result<()> {
    let source = tempfile::Builder::new()
        .prefix(".eef-setup-probe-")
        .tempfile_in(parent)?;
    let before = file_id(source.as_file())?;
    let target = tempfile::Builder::new()
        .prefix(".eef-setup-probe-")
        .tempfile_in(parent)?
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

fn encode(record: &Record) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(record)?;
    ensure!(
        body.len() <= MAX_RECORD,
        "Setup journal record is too large"
    );
    let mut frame = Vec::with_capacity(body.len() + 36);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    frame.extend_from_slice(&Sha256::digest(&body));
    Ok(frame)
}

impl Header {
    fn check_base(&self, root: &Path) -> Result<()> {
        ensure!(self.version == 1, "Unsupported setup journal version");
        ensure!(
            matches!(self.role.as_str(), "eef" | "eefn"),
            "Unknown setup journal role"
        );
        ensure!(
            self.transaction_id.len() == 32
                && self
                    .transaction_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "Invalid setup transaction identity"
        );
        ensure!(
            self.root == root.canonicalize()?,
            "Setup journal belongs to another installation folder"
        );
        ensure!(
            self.external.len() <= 2,
            "Too many external setup locations"
        );
        Ok(())
    }

    fn check_path(&self, path: &Path) -> Result<()> {
        ensure!(
            !path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir)),
            "Relative components in setup journal"
        );
        ensure!(
            location(path)? == path,
            "A setup recovery parent was redirected"
        );
        let in_root = path.strip_prefix(&self.root).ok().is_some_and(|relative| {
            !relative.as_os_str().is_empty()
                && !relative.components().next().is_some_and(|part| {
                    part.as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(JOURNAL_NAME)
                })
        });
        ensure!(
            in_root || self.external.iter().any(|expected| expected == path),
            "Setup recovery path is outside this installation and its shortcuts"
        );
        Ok(())
    }

    fn check_stage(&self, stage: &Stage) -> Result<()> {
        ensure!(
            location(&stage.path)? == stage.path,
            "A staging parent was redirected"
        );
        let name = stage
            .path
            .file_name()
            .context("Missing staging filename")?
            .to_string_lossy();
        ensure!(
            name.starts_with(".eef-setup-stage-") && name.len() > 17,
            "Invalid setup staging filename"
        );
        let in_root = stage.path.starts_with(&self.root) && stage.path != self.root;
        ensure!(
            in_root
                || self
                    .external
                    .iter()
                    .any(|path| path.parent() == stage.path.parent()),
            "Staging file is outside this installation and its shortcut directories"
        );
        Ok(())
    }

    fn check_change(&self, change: &Change, stages: &[Stage]) -> Result<()> {
        self.check_path(&change.path)?;
        ensure!(
            change.after.is_some() == change.after_identity.is_some(),
            "Missing staged file identity"
        );
        if let Some(index) = change.stage {
            let stage = stages.get(index).context("Unknown setup staging record")?;
            ensure!(
                change.after_identity == Some(stage.identity)
                    && stage.path.parent() == change.path.parent(),
                "Setup staging identity or directory differs"
            );
        } else {
            ensure!(
                change.after.is_none(),
                "Published file has no staging record"
            );
        }
        if let Some(original) = &change.original {
            ensure!(
                location(&original.backup)? == original.backup
                    && original.backup.parent() == change.path.parent(),
                "Setup backup is outside its original directory"
            );
            let prefix = backup_prefix(&change.path)?;
            let name = original
                .backup
                .file_name()
                .context("Missing backup filename")?
                .to_string_lossy();
            ensure!(
                name.starts_with(&prefix)
                    && name.ends_with(".backup")
                    && name.len() > prefix.len() + 7,
                "Invalid setup backup name"
            );
        } else {
            ensure!(
                change.after.is_some(),
                "A setup deletion has no original file"
            );
        }
        Ok(())
    }
}

fn parse(path: &Path) -> Result<Parsed> {
    regular(path)?;
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let identity = file_id(&file)?;
    ensure!(
        length <= MAX_JOURNAL,
        "Setup journal exceeds its size limit"
    );
    let mut header = None;
    let mut changes: Vec<Change> = Vec::new();
    let mut stages: Vec<Stage> = Vec::new();
    let mut committed = false;
    let mut valid_bytes = 0_u64;
    while length - valid_bytes >= 4 {
        let mut prefix = [0_u8; 4];
        file.read_exact(&mut prefix)?;
        let count = u32::from_le_bytes(prefix) as usize;
        ensure!(
            (2..=MAX_RECORD).contains(&count),
            "Invalid setup journal frame length"
        );
        let frame_size = count as u64 + 36;
        if length - valid_bytes < frame_size {
            break;
        } // Incomplete final append, no subsequent mutation was authorized.
        let mut body = vec![0_u8; count];
        let mut checksum = [0_u8; 32];
        file.read_exact(&mut body)?;
        file.read_exact(&mut checksum)?;
        ensure!(
            <[u8; 32]>::from(Sha256::digest(&body)) == checksum,
            "Setup journal checksum mismatch"
        );
        let record: Record =
            serde_json::from_slice(&body).context("Invalid setup journal record")?;
        match record {
            Record::Header { header: value } => {
                ensure!(
                    header.is_none() && valid_bytes == 0,
                    "Repeated setup journal header"
                );
                header = Some(value);
            }
            Record::Prepared { index, mut change } => {
                ensure!(
                    header.is_some() && !committed && index == changes.len() && index < MAX_CHANGES,
                    "Out-of-order setup journal preparation"
                );
                ensure!(
                    !change.applied
                        && !change.reverted
                        && !change.cleaned
                        && !change
                            .original
                            .as_ref()
                            .is_some_and(|original| original.moved),
                    "Invalid initial setup change state"
                );
                change.logged = true;
                changes.push(change);
            }
            Record::Staging { index, stage } => {
                ensure!(
                    header.is_some() && !committed && index == stages.len() && index < MAX_CHANGES,
                    "Out-of-order setup staging record"
                );
                stages.push(stage);
            }
            Record::Applied { index } => {
                let change = changes
                    .get_mut(index)
                    .context("Unknown applied setup change")?;
                ensure!(
                    !committed && !change.applied && !change.reverted,
                    "Out-of-order setup application"
                );
                change.applied = true;
                if let Some(original) = &mut change.original {
                    original.moved = true;
                }
            }
            Record::Undone { index } => {
                let change = changes
                    .get_mut(index)
                    .context("Unknown recovered setup change")?;
                ensure!(
                    !committed && !change.reverted,
                    "Out-of-order setup recovery"
                );
                change.reverted = true;
            }
            Record::Committed => {
                ensure!(
                    header.is_some() && !committed && !changes.iter().any(|change| change.reverted),
                    "Out-of-order setup commit"
                );
                committed = true;
            }
            Record::Cleaned { index } => {
                let change = changes
                    .get_mut(index)
                    .context("Unknown cleaned setup change")?;
                ensure!(
                    committed && !change.cleaned && change.original.is_some(),
                    "Out-of-order setup cleanup"
                );
                change.cleaned = true;
            }
        }
        valid_bytes += frame_size;
    }
    Ok(Parsed {
        header: header.context("Setup journal has no complete header")?,
        changes,
        stages,
        committed,
        valid_bytes,
        identity,
        length,
    })
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
