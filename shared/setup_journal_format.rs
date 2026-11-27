//! Shared v1 setup journal data and read-only decoding. Checksums are not authorization.
use crate::setup_state::JOURNAL_NAME;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub(crate) const MAX_RECORD: usize = 256 * 1024;
pub(crate) const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_CHANGES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileId {
    pub(crate) volume: u64,
    pub(crate) identifier: [u8; 16],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stage {
    pub(crate) path: PathBuf,
    pub(crate) identity: FileId,
}

#[cfg(windows)]
pub(crate) fn file_id(file: &File) -> Result<FileId> {
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
pub(crate) fn file_id(file: &File) -> Result<FileId> {
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
pub(crate) fn file_id(_file: &File) -> Result<FileId> {
    anyhow::bail!("This platform does not supply setup file identities")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Original {
    pub(crate) backup: PathBuf,
    pub(crate) digest: [u8; 32],
    pub(crate) moved: bool,
    pub(crate) identity: FileId,
    pub(crate) reserved_identity: FileId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    pub(crate) path: PathBuf,
    pub(crate) original: Option<Original>,
    pub(crate) after: Option<[u8; 32]>,
    pub(crate) applied: bool,
    pub(crate) after_identity: Option<FileId>,
    pub(crate) reverted: bool,
    pub(crate) cleaned: bool,
    #[serde(skip)]
    pub(crate) logged: bool,
    pub(crate) stage: Option<usize>,
}

pub(crate) fn backup_prefix(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .context("Missing setup filename")?
        .to_string_lossy();
    let hash = hex::encode(Sha256::digest(name.as_bytes()));
    Ok(format!(".eef-setup-{}-", &hash[..24]))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Header {
    pub(crate) version: u32,
    pub(crate) transaction_id: String,
    pub(crate) root: PathBuf,
    pub(crate) role: String,
    pub(crate) external: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record {
    Header { header: Header },
    Staging { index: usize, stage: Stage },
    Prepared { index: usize, change: Change },
    Applied { index: usize },
    Undone { index: usize },
    Committed,
    Cleaned { index: usize },
}

pub(crate) struct Parsed {
    pub(crate) header: Header,
    pub(crate) changes: Vec<Change>,
    pub(crate) committed: bool,
    pub(crate) valid_bytes: u64,
    pub(crate) stages: Vec<Stage>,
    pub(crate) identity: FileId,
    pub(crate) length: u64,
}

/// Resolve the parent, including an existing ancestor of an uncreated path.
/// A planned shortcut may have an invalid file as its parent; writing it still
/// fails at the actual setup step, without pre-creating any directories here.
pub(crate) fn location(path: &Path) -> Result<PathBuf> {
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

pub(crate) fn regular(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "Setup journal must be a regular file"
    );
    Ok(())
}

pub(crate) fn encode(record: &Record) -> Result<Vec<u8>> {
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
    pub(crate) fn check_base(&self, root: &Path) -> Result<()> {
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

    pub(crate) fn check_path(&self, path: &Path) -> Result<()> {
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
        let in_root = path
            .strip_prefix(&self.root)
            .ok()
            .is_some_and(|relative| !relative.as_os_str().is_empty());
        ensure!(
            !path.strip_prefix(&self.root).ok().is_some_and(|relative| {
                relative.components().next().is_some_and(|part| {
                    let name = part.as_os_str().to_string_lossy();
                    [JOURNAL_NAME, ".update.lock", crate::package_use::LOCK_NAME]
                        .iter()
                        .any(|reserved| name.eq_ignore_ascii_case(reserved))
                })
            }),
            "Setup recovery cannot replace internal control metadata"
        );
        ensure!(
            in_root || self.external.iter().any(|expected| expected == path),
            "Setup recovery path is outside this installation and its shortcuts"
        );
        Ok(())
    }

    pub(crate) fn check_stage(&self, stage: &Stage) -> Result<()> {
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

    pub(crate) fn check_change(&self, change: &Change, stages: &[Stage]) -> Result<()> {
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

pub(crate) fn parse(path: &Path) -> Result<Parsed> {
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
                    header.is_some()
                        && !committed
                        && changes
                            .iter()
                            .all(|change| change.applied && !change.reverted),
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
