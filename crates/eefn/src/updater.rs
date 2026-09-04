//! Verified, versioned node updates.
//!
//! Windows cannot replace a running executable. Updates therefore install into
//! `versions/<version>/` and atomically switch `current.txt`; the next launch
//! uses the new version and the previous directory remains available for rollback.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub sha256: String,
    pub dist_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateApplied {
    pub new_version: String,
    pub previous_version: Option<String>,
    pub restart: bool,
    pub install_dir: PathBuf,
}

pub async fn load_manifest(url: &str, timeout: Duration) -> Result<UpdateManifest> {
    let bytes = fetch(url, timeout).await?;
    let manifest: UpdateManifest =
        serde_json::from_slice(&bytes).context("manifest is not valid JSON")?;
    if manifest.version.trim().is_empty()
        || manifest.url.trim().is_empty()
        || manifest.sha256.len() != 64
    {
        bail!("manifest must contain version, url, and a 64-character sha256");
    }
    Ok(manifest)
}

pub async fn check(url: &str, current_version: &str, timeout: Duration) -> Result<UpdateCheck> {
    let manifest = load_manifest(url, timeout).await?;
    Ok(UpdateCheck {
        current_version: current_version.into(),
        latest_version: manifest.version.clone(),
        update_available: is_newer(&manifest.version, current_version),
        sha256: manifest.sha256,
        dist_url: manifest.url,
    })
}

pub async fn apply(
    url: &str,
    install_dir: impl AsRef<Path>,
    timeout: Duration,
) -> Result<UpdateApplied> {
    apply_for(url, install_dir, timeout, "eefn").await
}

pub async fn apply_for(
    url: &str,
    install_dir: impl AsRef<Path>,
    timeout: Duration,
    program: &str,
) -> Result<UpdateApplied> {
    validate_program(program)?;
    let install = install_dir
        .as_ref()
        .canonicalize()
        .context("install directory does not exist")?;
    if !install.is_dir() {
        bail!("install directory is not a directory")
    }
    let manifest = load_manifest(url, timeout.min(Duration::from_secs(30))).await?;
    validate_version(&manifest.version)?;
    let bytes = fetch(&manifest.url, timeout).await?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        bail!("dist zip sha256 mismatch; refusing update");
    }

    let versions = install.join("versions");
    fs::create_dir_all(&versions)?;
    let destination = versions.join(&manifest.version);
    if destination.exists() {
        let resolved = destination.canonicalize()?;
        if !resolved.starts_with(&versions) {
            bail!("unsafe update destination")
        }
        fs::remove_dir_all(&resolved)?;
    }
    fs::create_dir_all(&destination)?;
    extract_zip(&bytes, &destination)?;
    if !contains_binary(&destination, program) {
        fs::remove_dir_all(&destination)?;
        bail!("update archive does not contain the {program} executable");
    }

    let current_path = install.join("current.txt");
    let previous = fs::read_to_string(&current_path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let next_path = install.join("current.txt.next");
    fs::write(&next_path, format!("{}\n", manifest.version))?;
    fs::rename(&next_path, &current_path).or_else(|_| {
        fs::copy(&next_path, &current_path)?;
        fs::remove_file(&next_path)
    })?;
    fs::write(
        install.join("previous.txt"),
        previous.clone().unwrap_or_default(),
    )?;
    Ok(UpdateApplied {
        new_version: manifest.version,
        previous_version: previous,
        restart: true,
        install_dir: install,
    })
}

pub fn rollback(install_dir: impl AsRef<Path>) -> Result<String> {
    rollback_for(install_dir, "eefn")
}

pub fn rollback_for(install_dir: impl AsRef<Path>, program: &str) -> Result<String> {
    validate_program(program)?;
    let install = install_dir.as_ref().canonicalize()?;
    let previous = fs::read_to_string(install.join("previous.txt"))?
        .trim()
        .to_owned();
    if previous.is_empty() {
        bail!("no previous version is available")
    }
    validate_version(&previous)?;
    let target = install.join("versions").join(&previous);
    if !target.is_dir() || !contains_binary(&target, program) {
        bail!("previous version is incomplete")
    }
    fs::write(install.join("current.txt"), format!("{previous}\n"))?;
    Ok(previous)
}

pub fn installation_root(executable: impl AsRef<Path>) -> Result<PathBuf> {
    let executable = executable.as_ref().canonicalize()?;
    let parent = executable
        .parent()
        .context("executable has no parent directory")?;
    if parent
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("versions")
    {
        return parent
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .context("versioned executable has no installation root");
    }
    Ok(parent.to_path_buf())
}

pub fn handoff_if_selected(
    install_dir: impl AsRef<Path>,
    current_version: &str,
    program: &str,
) -> Result<bool> {
    validate_program(program)?;
    let install = install_dir.as_ref();
    let selected = match fs::read_to_string(install.join("current.txt")) {
        Ok(value) => value.trim().to_owned(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if selected.is_empty() || selected == current_version {
        return Ok(false);
    }
    validate_version(&selected)?;
    let version_dir = install.join("versions").join(&selected);
    let executable = find_binary(&version_dir, program)
        .with_context(|| format!("selected {program} version '{selected}' is incomplete"))?;
    std::process::Command::new(executable)
        .args(std::env::args_os().skip(1))
        .current_dir(std::env::current_dir()?)
        .spawn()
        .context("launch selected update")?;
    Ok(true)
}

pub fn sha256_file(path: impl AsRef<Path>) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex::encode(hash.finalize()))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    version_parts(latest) > version_parts(current)
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .replace('-', ".")
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn validate_version(version: &str) -> Result<()> {
    if version.is_empty()
        || !version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("unsafe update version")
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<()> {
    let cursor = io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("unsafe path in update archive")?
            .to_path_buf();
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?
            }
            let mut file = File::create(&output)?;
            io::copy(&mut entry, &mut file)?;
        }
    }
    Ok(())
}

fn validate_program(program: &str) -> Result<()> {
    if program.is_empty()
        || !program
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("unsafe program name")
    }
    Ok(())
}

fn find_binary(root: &Path, program: &str) -> Option<PathBuf> {
    let names = [format!("{program}.exe"), program.to_owned()];
    for name in &names {
        let candidate = root.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for entry in fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
    {
        for name in &names {
            let candidate = entry.path().join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn contains_binary(root: &Path, program: &str) -> bool {
    find_binary(root, program).is_some()
}

async fn fetch(url: &str, timeout: Duration) -> Result<Vec<u8>> {
    if let Some(path) = url.strip_prefix("file://") {
        return Ok(tokio::fs::read(PathBuf::from(path)).await?);
    }
    if Path::new(url).is_file() {
        return Ok(tokio::fs::read(url).await?);
    }
    let response = reqwest::Client::new()
        .get(url)
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
    }
}
