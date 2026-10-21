//! Same-version package selection. Semantic versions remain in current.txt;
//! repair generations live beside, never over, a running bundle.
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const ROOT_SLOT: &str = "$root";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleSelection {
    pub schema_version: u32,
    pub version: String,
    pub slot: String,
    pub previous_slot: Option<String>,
}

impl BundleSelection {
    fn validate(&self, version: &str) -> Result<()> {
        crate::updater::validate_version(version)?;
        if self.schema_version != 1 || self.version != version {
            bail!("invalid bundle selection schema/version")
        }
        validate_slot(&self.slot, version)?;
        if let Some(previous) = &self.previous_slot {
            validate_slot(previous, version)?;
            if previous == &self.slot {
                bail!("bundle selection cannot refer to itself as previous")
            }
        }
        Ok(())
    }
}

pub fn repair_slot(version: &str, request_id: &str) -> Result<String> {
    crate::updater::validate_version(version)?;
    if uuid::Uuid::parse_str(request_id)?.to_string() != request_id {
        bail!("repair request ID must be a canonical UUID")
    }
    Ok(format!("{version}.repair-{request_id}"))
}

fn validate_slot(slot: &str, version: &str) -> Result<()> {
    if slot == ROOT_SLOT || slot == version {
        return Ok(());
    }
    let request_id = slot
        .strip_prefix(&format!("{version}.repair-"))
        .context("invalid repair bundle slot")?;
    if repair_slot(version, request_id)? != slot {
        bail!("invalid repair bundle slot")
    }
    Ok(())
}

fn selector_path(install: &Path, version: &str) -> Result<PathBuf> {
    crate::updater::validate_version(version)?;
    Ok(install
        .join("versions")
        .join(format!("{version}.bundle.json")))
}

pub fn read(install: &Path, version: &str) -> Result<Option<BundleSelection>> {
    let file = match File::open(selector_path(install, version)?) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        bail!("bundle selection is too large")
    }
    let selection: BundleSelection = serde_json::from_slice(&bytes)?;
    selection.validate(version)?;
    Ok(Some(selection))
}

pub fn write(install: &Path, selection: &BundleSelection) -> Result<()> {
    selection.validate(&selection.version)?;
    crate::atomic_file::write(
        &selector_path(install, &selection.version)?,
        &serde_json::to_vec(selection)?,
    )?;
    Ok(())
}

pub fn directory(install: &Path, version: &str, slot: &str) -> Result<PathBuf> {
    validate_slot(slot, version)?;
    Ok(if slot == ROOT_SLOT {
        install.to_path_buf()
    } else {
        install.join("versions").join(slot)
    })
}

/// Identify only a direct flat/versioned bundle, not an arbitrary executable.
pub fn running_slot(install: &Path, version: &str, executable: &Path) -> Result<String> {
    let install = install.canonicalize()?;
    let executable = executable.canonicalize()?;
    let parent = executable
        .parent()
        .context("missing executable directory")?;
    if parent == install {
        return Ok(ROOT_SLOT.into());
    }
    if parent.parent() != Some(install.join("versions").as_path()) {
        bail!("running executable is not in a direct installation bundle")
    }
    let slot = parent
        .file_name()
        .and_then(|s| s.to_str())
        .context("invalid bundle name")?;
    validate_slot(slot, version)?;
    Ok(slot.into())
}

pub fn selected_directory(install: &Path, version: &str) -> Result<PathBuf> {
    match read(install, version)? {
        Some(selection) => directory(install, version, &selection.slot),
        None => Ok(install.join("versions").join(version)),
    }
}

/// Old same-version selectors retain their no-handoff behavior until an explicit
/// generation exists. A corrupt generation is never silently ignored.
pub fn pending_executable(
    install: &Path,
    current_version: &str,
    program: &str,
    running_executable: &Path,
) -> Result<Option<PathBuf>> {
    crate::updater::validate_program(program)?;
    let Some(version) = crate::updater::installed_version(install)? else {
        return Ok(None);
    };
    let selection = read(install, &version)?;
    if version == current_version && selection.is_none() {
        return Ok(None);
    }
    let directory = match selection {
        Some(selection) => directory(install, &version, &selection.slot)?,
        None => install.join("versions").join(&version),
    };
    let executable = crate::updater::find_binary(&directory, program)
        .with_context(|| format!("selected {program} bundle '{version}' is incomplete"))?
        .canonicalize()?;
    let install = install.canonicalize()?;
    if !executable.starts_with(&install) {
        bail!("selected bundle escapes the installation directory")
    }
    if executable == running_executable.canonicalize()? {
        if version != current_version {
            bail!("selected version disagrees with the running executable")
        }
        return Ok(None);
    }
    Ok(Some(executable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn bundle_selection_same_version_handoff_and_rollback_do_not_loop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let version = "0.4.0-alpha.6";
        let root_exe = root.join("eefn.exe");
        fs::write(&root_exe, b"old").unwrap();
        fs::write(root.join("current.txt"), version).unwrap();
        assert!(
            pending_executable(root, version, "eefn", &root_exe)
                .unwrap()
                .is_none()
        );
        let slot = repair_slot(version, &uuid::Uuid::new_v4().to_string()).unwrap();
        let bundle = directory(root, version, &slot).unwrap();
        fs::create_dir_all(&bundle).unwrap();
        let repaired = bundle.join("eefn.exe");
        fs::write(&repaired, b"same").unwrap();
        let mut selection = BundleSelection {
            schema_version: 1,
            version: version.into(),
            slot,
            previous_slot: Some(ROOT_SLOT.into()),
        };
        write(root, &selection).unwrap();
        assert_eq!(
            pending_executable(root, version, "eefn", &root_exe).unwrap(),
            Some(repaired.canonicalize().unwrap())
        );
        assert!(
            pending_executable(root, version, "eefn", &repaired)
                .unwrap()
                .is_none()
        );
        assert_eq!(running_slot(root, version, &root_exe).unwrap(), ROOT_SLOT);
        assert_eq!(
            running_slot(root, version, &repaired).unwrap(),
            selection.slot
        );
        selection.slot = selection.previous_slot.take().unwrap();
        write(root, &selection).unwrap();
        assert_eq!(
            pending_executable(root, version, "eefn", &repaired).unwrap(),
            Some(root_exe.canonicalize().unwrap())
        );
        assert!(
            pending_executable(root, version, "eefn", &root_exe)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bundle_selection_invalid_generation_fails_closed_without_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let version = "0.4.0";
        fs::create_dir_all(dir.path().join("versions")).unwrap();
        let path = selector_path(dir.path(), version).unwrap();
        for bytes in [
            vec![b'x'; 4097],
            b"{}".to_vec(),
            serde_json::to_vec(&BundleSelection {
                schema_version: 1,
                version: version.into(),
                slot: "../escape".into(),
                previous_slot: None,
            })
            .unwrap(),
            serde_json::to_vec(&BundleSelection {
                schema_version: 1,
                version: "0.5.0".into(),
                slot: version.into(),
                previous_slot: None,
            })
            .unwrap(),
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(read(dir.path(), version).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert!(repair_slot(version, "../bad").is_err());
        assert!(selector_path(dir.path(), "../../bad").is_err());
    }
}
