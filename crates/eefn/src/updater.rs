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

#[path = "../../../shared/installer_payload.rs"]
mod installer_payload;

// Installers contain runtimes, never model weights. Bound both declared and
// streamed sizes, including servers that omit Content-Length.
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_UPDATE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub prerelease_blocked: bool,
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

/// Read a bounded, validated installed selector; corruption is not "no update".
pub fn installed_version(install_dir: &Path) -> Result<Option<String>> {
    let file = match File::open(install_dir.join("current.txt")) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        bail!("installed version selector is too large")
    }
    let value = String::from_utf8(bytes)?;
    let version = value.trim();
    if version.is_empty() {
        return Ok(None);
    }
    validate_version(version)?;
    Ok(Some(version.into()))
}

/// One-shot owner selection, never persisted as an automatic-update preference.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitUpdate {
    pub schema_version: u32,
    pub expected_node_id: String,
    pub request_id: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub allow_prerelease: bool,
}

impl ExplicitUpdate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported explicit update schema")
        }
        crate::network::validate_node_id(&self.expected_node_id)?;
        if uuid::Uuid::parse_str(&self.request_id)?.to_string() != self.request_id {
            bail!("update request_id must be a canonical UUID")
        }
        validate_version(&self.version)?;
        if self.version.contains('-') && !self.allow_prerelease {
            bail!("explicit prerelease update requires allow_prerelease")
        }
        validate_https_artifact(&self.url)?;
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("explicit update requires an exact SHA-256")
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_UPDATE_BYTES as u64 {
            bail!("explicit update size must be positive and at most 512 MiB")
        }
        Ok(())
    }
}

fn validate_https_artifact(url: &str) -> Result<()> {
    if url.len() > 4096 || url.chars().any(char::is_control) {
        bail!("invalid update URL")
    }
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        bail!("explicit update requires HTTPS without credentials or a fragment")
    }
    Ok(())
}

pub async fn apply_explicit_for<G>(
    request: &ExplicitUpdate,
    install_dir: impl AsRef<Path>,
    current_version: &str,
    timeout: Duration,
    program: &str,
    authorize: impl Fn() -> Result<G>,
) -> Result<UpdateApplied> {
    request.validate()?;
    if !is_newer(&request.version, current_version) {
        bail!("explicit update must be newer than the running version")
    }
    drop(authorize()?);
    let manifest = UpdateManifest {
        version: request.version.clone(),
        url: request.url.clone(),
        sha256: request.sha256.clone(),
        size_bytes: Some(request.size_bytes),
    };
    install_manifest(
        &manifest,
        install_dir.as_ref(),
        timeout,
        program,
        Some(current_version),
        true,
        authorize,
    )
    .await
}

pub async fn load_manifest(url: &str, timeout: Duration) -> Result<UpdateManifest> {
    let bytes = fetch(url, timeout, MAX_MANIFEST_BYTES).await?;
    let manifest: UpdateManifest =
        serde_json::from_slice(&bytes).context("manifest is not valid JSON")?;
    if manifest.version.trim().is_empty()
        || manifest.url.trim().is_empty()
        || manifest.sha256.len() != 64
        || !manifest.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("manifest must contain version, url, and a 64-character sha256");
    }
    validate_version(&manifest.version)?;
    if manifest
        .size_bytes
        .is_some_and(|size| size == 0 || size > MAX_UPDATE_BYTES as u64)
    {
        bail!("Update size must be positive and no larger than 512 MiB")
    }
    Ok(manifest)
}

pub async fn check(url: &str, current_version: &str, timeout: Duration) -> Result<UpdateCheck> {
    let manifest = load_manifest(url, timeout).await?;
    let prerelease_blocked = manifest.version.contains('-');
    Ok(UpdateCheck {
        current_version: current_version.into(),
        latest_version: manifest.version.clone(),
        update_available: !prerelease_blocked && is_newer(&manifest.version, current_version),
        prerelease_blocked,
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
    let manifest = load_manifest(url, timeout.min(Duration::from_secs(30))).await?;
    validate_version(&manifest.version)?;
    // Recheck the actual manifest being installed, not a previous check result.
    // Automatic and legacy feed callers can never opt into prereleases.
    if manifest.version.contains('-') {
        bail!(
            "prerelease updates are blocked on feeds; use an explicit update command or installer"
        )
    }
    install_manifest(
        &manifest,
        install_dir.as_ref(),
        timeout,
        program,
        None,
        false,
        || Ok(()),
    )
    .await
}

async fn install_manifest<G>(
    manifest: &UpdateManifest,
    install_dir: &Path,
    timeout: Duration,
    program: &str,
    current_version: Option<&str>,
    https_only: bool,
    authorize: impl Fn() -> Result<G>,
) -> Result<UpdateApplied> {
    validate_program(program)?;
    let install = install_dir
        .canonicalize()
        .context("install directory does not exist")?;
    if !install.is_dir() {
        bail!("install directory is not a directory")
    }
    // Serialize explicit and automatic installers, including separate processes.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(install.join(".update.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another update is in progress")?;
    let selected = installed_version(&install)?;
    if let Some(current) = current_version {
        if selected
            .as_deref()
            .is_some_and(|selected| selected != current)
        {
            bail!("an installed update is pending; restart and inspect before updating again")
        }
    }
    let versions = install.join("versions");
    fs::create_dir_all(&versions)?;
    let destination = versions.join(&manifest.version);
    if destination.exists() {
        bail!("this update version is already present; existing installed files were preserved")
    }
    if let Some(selected) = selected.as_deref() {
        if !is_newer(&manifest.version, selected) {
            bail!("update cannot replace an equal or newer installed selection")
        }
    }
    let limit = manifest
        .size_bytes
        .map(|size| size as usize)
        .unwrap_or(MAX_UPDATE_BYTES);
    let bytes = fetch_with_policy(&manifest.url, timeout, limit, https_only).await?;
    if manifest
        .size_bytes
        .is_some_and(|size| size != bytes.len() as u64)
    {
        bail!("Update download did not match its declared size")
    }
    let actual = hex::encode(Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        bail!("dist zip sha256 mismatch; refusing update");
    }
    let staging = tempfile::Builder::new()
        .prefix(".incoming-")
        .tempdir_in(&versions)?;
    extract_zip(&bytes, staging.path())?;
    if !contains_binary(staging.path(), program) {
        bail!("update archive does not contain the {program} executable");
    }
    if current_version.is_some() {
        let binary = find_binary(staging.path(), program).context("missing update executable")?;
        let metadata_path = binary
            .parent()
            .context("missing update directory")?
            .join("bundle.json");
        let mut metadata = Vec::new();
        File::open(metadata_path)?
            .take(65537)
            .read_to_end(&mut metadata)?;
        if metadata.len() > 65536 {
            bail!("update bundle metadata is too large")
        }
        let metadata: serde_json::Value = serde_json::from_slice(&metadata)?;
        if metadata["name"] != program || metadata["version"] != manifest.version {
            bail!("update bundle product/version does not match the explicit selection")
        }
    }
    // A revoked grant while downloading must not select the staged update.
    let _authorization_guard = authorize()?;
    fs::rename(staging.path(), &destination)?;

    let current_path = install.join("current.txt");
    let previous = selected;
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
        new_version: manifest.version.clone(),
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
    if validate_version(latest).is_err() || validate_version(current).is_err() {
        return false;
    }
    let (latest_core, latest_pre) = latest
        .split_once('-')
        .map_or((latest, None), |(core, pre)| (core, Some(pre)));
    let (current_core, current_pre) = current
        .split_once('-')
        .map_or((current, None), |(core, pre)| (core, Some(pre)));
    match version_parts(latest_core).cmp(&version_parts(current_core)) {
        std::cmp::Ordering::Greater => return true,
        std::cmp::Ordering::Less => return false,
        std::cmp::Ordering::Equal => {}
    }
    match (latest_pre, current_pre) {
        (None, Some(_)) => true,
        (Some(_), None) | (None, None) => false,
        (Some(latest), Some(current)) => {
            let left: Vec<_> = latest.split('.').collect();
            let right: Vec<_> = current.split('.').collect();
            for (a, b) in left.iter().zip(&right) {
                let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    _ => a.cmp(b),
                };
                if order != std::cmp::Ordering::Equal {
                    return order == std::cmp::Ordering::Greater;
                }
            }
            left.len() > right.len()
        }
    }
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .replace('-', ".")
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn validate_version(version: &str) -> Result<()> {
    let base = version.split('-').next().unwrap_or("");
    let parts: Vec<_> = base.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
        || version.len() > 100
        || !version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("unsafe update version")
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], destination: &Path) -> Result<()> {
    let cursor = io::Cursor::new(update_payload(bytes)?);
    let mut archive = zip::ZipArchive::new(cursor)?;
    let mut required = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        entry
            .enclosed_name()
            .context("unsafe path in update archive")?;
        required = required
            .checked_add(entry.size())
            .context("update archive size overflow")?;
    }
    if fs2::available_space(destination)? < required {
        bail!(
            "Not enough disk space for this update. Free space or choose another installation drive."
        )
    }
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
            let expected = entry.size();
            let copied = io::copy(
                &mut entry.by_ref().take(expected.saturating_add(1)),
                &mut file,
            )?;
            if copied != expected {
                bail!("update entry exceeded or did not match its declared size")
            }
        }
    }
    Ok(())
}

fn update_payload(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok(bytes);
    }
    let range = installer_payload::payload_range(&mut io::Cursor::new(bytes))?;
    Ok(&bytes[usize::try_from(range.start)?..usize::try_from(range.end)?])
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

async fn fetch(url: &str, timeout: Duration, limit: usize) -> Result<Vec<u8>> {
    fetch_with_policy(url, timeout, limit, false).await
}

async fn fetch_with_policy(
    url: &str,
    timeout: Duration,
    limit: usize,
    https_only: bool,
) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    if https_only {
        validate_https_artifact(url)?;
    }
    if let Some(path) = url
        .strip_prefix("file://")
        .or_else(|| Path::new(url).is_file().then_some(url))
    {
        let file = tokio::fs::File::open(path).await?;
        if file.metadata().await?.len() > limit as u64 {
            bail!("Update response exceeds its size limit")
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes).await?;
        if bytes.len() > limit {
            bail!("Update response exceeds its size limit")
        }
        return Ok(bytes);
    }
    let client = if https_only {
        reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5
                    || validate_https_artifact(attempt.url().as_str()).is_err()
                {
                    attempt.error("unsafe update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?
    } else {
        reqwest::Client::new()
    };
    let mut response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        bail!("Update response exceeds its size limit")
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            bail!("Update response exceeds its size limit")
        }
        bytes
            .try_reserve(chunk.len())
            .context("Not enough memory to receive this update")?;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_selector_is_bounded_and_fails_closed_on_corruption() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed_version(dir.path()).unwrap(), None);
        let path = dir.path().join("current.txt");
        fs::write(&path, "0.4.0-alpha.5\n").unwrap();
        assert_eq!(
            installed_version(dir.path()).unwrap().as_deref(),
            Some("0.4.0-alpha.5")
        );
        for bytes in [b"../invalid".to_vec(), vec![b'a'; 1025], vec![255]] {
            fs::write(&path, &bytes).unwrap();
            assert!(installed_version(dir.path()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    fn explicit_request() -> ExplicitUpdate {
        ExplicitUpdate {
            schema_version: 1,
            expected_node_id: "fixture-node".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            version: "0.4.0-alpha.5".into(),
            url: "https://example.invalid/node.exe".into(),
            sha256: "a".repeat(64),
            size_bytes: 123,
            allow_prerelease: true,
        }
    }

    #[test]
    fn explicit_request_requires_exact_bounded_selection_and_one_shot_consent() {
        let request = explicit_request();
        request.validate().unwrap();
        let mut bad = request.clone();
        bad.allow_prerelease = false;
        assert!(bad.validate().is_err());
        bad.version = "0.4.0".into();
        bad.validate().unwrap();
        for url in [
            "http://example.org/a",
            "file:///a",
            "https://user:password@example.org/a",
            "https://example.org/a#fragment",
        ] {
            bad = request.clone();
            bad.url = url.into();
            assert!(bad.validate().is_err());
        }
        for bytes in [0, MAX_UPDATE_BYTES as u64 + 1] {
            bad = request.clone();
            bad.size_bytes = bytes;
            assert!(bad.validate().is_err());
        }
        bad = request.clone();
        bad.sha256 = "x".repeat(64);
        assert!(bad.validate().is_err());
        bad = request.clone();
        bad.schema_version = 2;
        assert!(bad.validate().is_err());
        bad = request.clone();
        bad.request_id = "not-a-uuid".into();
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(request).unwrap();
        value["automatic_policy"] = serde_json::json!("alpha");
        assert!(serde_json::from_value::<ExplicitUpdate>(value).is_err());
    }

    #[tokio::test]
    async fn explicit_update_rejects_no_consent_downgrade_and_denied_authority_before_io() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = explicit_request();
        for version in ["0.4.0-alpha.5", "0.4.0", "1.0.0"] {
            assert!(
                apply_explicit_for(
                    &request,
                    dir.path(),
                    version,
                    Duration::from_secs(1),
                    "eefn",
                    || Ok(())
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("newer")
            );
        }
        assert!(
            apply_explicit_for(
                &request,
                dir.path(),
                "0.4.0-alpha.4",
                Duration::from_secs(1),
                "eefn",
                || -> Result<()> { anyhow::bail!("revoked") }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("revoked")
        );
        request.allow_prerelease = false;
        assert!(
            apply_explicit_for(
                &request,
                dir.path(),
                "0.4.0-alpha.4",
                Duration::from_secs(1),
                "eefn",
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("allow_prerelease")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn explicit_install_core_checks_artifact_authority_lock_and_preserves_feed() {
        use std::io::Write;
        for program in ["eef", "eefn"] {
            let dir = tempfile::tempdir().unwrap();
            let version = "0.4.0-alpha.5";
            let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
            zip.start_file(
                format!("{program}.exe"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"fixture only, never executed").unwrap();
            zip.start_file("bundle.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                &serde_json::to_vec(&serde_json::json!({"name":program,"version":version}))
                    .unwrap(),
            )
            .unwrap();
            let bytes = zip.finish().unwrap().into_inner();
            let artifact = dir.path().join("artifact.zip");
            fs::write(&artifact, &bytes).unwrap();
            let manifest = UpdateManifest {
                version: version.into(),
                url: artifact.to_string_lossy().into(),
                sha256: hex::encode(Sha256::digest(&bytes)),
                size_bytes: Some(bytes.len() as u64),
            };
            let current = "0.4.0-alpha.4";
            fs::write(dir.path().join("current.txt"), current).unwrap();
            fs::write(
                dir.path().join("config.json"),
                b"owner stable-only feed fixture",
            )
            .unwrap();
            let assert_unchanged = || {
                assert_eq!(
                    fs::read_to_string(dir.path().join("current.txt")).unwrap(),
                    current
                );
                assert!(!dir.path().join("versions").join(version).exists());
            };
            let mut bad = manifest.clone();
            bad.sha256 = "0".repeat(64);
            assert!(
                install_manifest(
                    &bad,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || Ok(())
                )
                .await
                .is_err()
            );
            assert_unchanged();
            bad = manifest.clone();
            bad.size_bytes = Some(bytes.len() as u64 + 1);
            assert!(
                install_manifest(
                    &bad,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || Ok(())
                )
                .await
                .is_err()
            );
            bad = manifest.clone();
            bad.version = "0.4.0-alpha.6".into();
            assert!(
                install_manifest(
                    &bad,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || Ok(())
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("product/version")
            );
            assert!(
                install_manifest(
                    &manifest,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || -> Result<()> { bail!("revoked after download") }
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("revoked")
            );
            assert_unchanged();
            let lock = File::options()
                .read(true)
                .write(true)
                .open(dir.path().join(".update.lock"))
                .unwrap();
            fs2::FileExt::try_lock_exclusive(&lock).unwrap();
            assert!(
                install_manifest(
                    &manifest,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || Ok(())
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("in progress")
            );
            drop(lock);
            let applied = install_manifest(
                &manifest,
                dir.path(),
                Duration::from_secs(1),
                program,
                Some(current),
                false,
                || Ok(()),
            )
            .await
            .unwrap();
            assert_eq!(applied.new_version, version);
            assert_eq!(
                fs::read_to_string(dir.path().join("previous.txt")).unwrap(),
                current
            );
            assert_eq!(
                fs::read(dir.path().join("config.json")).unwrap(),
                b"owner stable-only feed fixture"
            );
            assert!(
                install_manifest(
                    &manifest,
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                    Some(current),
                    false,
                    || Ok(())
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("pending")
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("current.txt")).unwrap(),
                format!("{version}\n")
            );
        }
    }

    #[tokio::test]
    async fn fetch_limits_local_declared_and_chunked_content() {
        use axum::{Router, body::Body, routing::get};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture");
        std::fs::write(&path, [b'x'; 65]).unwrap();
        assert!(
            fetch(path.to_str().unwrap(), Duration::from_secs(2), 64)
                .await
                .is_err()
        );
        assert_eq!(
            fetch(path.to_str().unwrap(), Duration::from_secs(2), 65)
                .await
                .unwrap()
                .len(),
            65
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/length", get(|| async { "x".repeat(65) }))
            .route(
                "/chunked",
                get(|| async {
                    Body::from_stream(futures_util::stream::iter(
                        (0..5).map(|_| Ok::<_, std::io::Error>(vec![b'x'; 16])),
                    ))
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for path in ["length", "chunked"] {
            assert!(
                fetch(
                    &format!("http://{address}/{path}"),
                    Duration::from_secs(2),
                    64
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("size limit")
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn update_size_hash_and_existing_versions_preserve_installation() {
        use std::io::Write;
        for program in ["eef", "eefn"] {
            let dir = tempfile::tempdir().unwrap();
            let mut archive = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
            archive
                .start_file(
                    format!("{program}.exe"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(b"test fixture, never executed").unwrap();
            let bytes = archive.finish().unwrap().into_inner();
            let artifact = dir.path().join("fixture.zip");
            fs::write(&artifact, &bytes).unwrap();
            let feed = dir.path().join("feed.json");
            let manifest = serde_json::json!({"version":"0.4.0","url":artifact,"sha256":hex::encode(Sha256::digest(&bytes)),"size_bytes":bytes.len()});
            let save = |value: &serde_json::Value| {
                fs::write(&feed, serde_json::to_vec(value).unwrap()).unwrap()
            };
            fs::write(dir.path().join("current.txt"), "0.3.2\n").unwrap();
            let mut bad = manifest.clone();
            bad["sha256"] = serde_json::json!("0".repeat(64));
            save(&bad);
            assert!(
                apply_for(
                    feed.to_str().unwrap(),
                    dir.path(),
                    Duration::from_secs(2),
                    program
                )
                .await
                .is_err()
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("current.txt")).unwrap(),
                "0.3.2\n"
            );
            bad = manifest.clone();
            bad["size_bytes"] = serde_json::json!(bytes.len() + 1);
            save(&bad);
            assert!(
                apply_for(
                    feed.to_str().unwrap(),
                    dir.path(),
                    Duration::from_secs(2),
                    program
                )
                .await
                .is_err()
            );
            save(&manifest);
            apply_for(
                feed.to_str().unwrap(),
                dir.path(),
                Duration::from_secs(2),
                program,
            )
            .await
            .unwrap();
            assert_eq!(
                fs::read_to_string(dir.path().join("current.txt")).unwrap(),
                "0.4.0\n"
            );
            let installed = dir
                .path()
                .join("versions/0.4.0")
                .join(format!("{program}.exe"));
            assert_eq!(
                fs::read(&installed).unwrap(),
                b"test fixture, never executed"
            );
            bad = manifest.clone();
            bad["url"] = serde_json::json!("http://127.0.0.1:1/not-requested");
            save(&bad);
            assert!(
                apply_for(
                    feed.to_str().unwrap(),
                    dir.path(),
                    Duration::from_secs(2),
                    program
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("already present")
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("previous.txt")).unwrap(),
                "0.3.2"
            );
        }
    }

    #[tokio::test]
    async fn feed_updates_never_install_prereleases_even_after_a_stable_check() {
        let dir = tempfile::tempdir().unwrap();
        let feed = dir.path().join("feed.json");
        let save = |version: &str| {
            fs::write(&feed, serde_json::to_vec(&serde_json::json!({
            "version":version,"url":"http://127.0.0.1:1/must-not-download","sha256":"0".repeat(64),
            "allow_prerelease":true,"manual_only":true
        })).unwrap()).unwrap()
        };
        for program in ["eef", "eefn"] {
            save("0.4.0");
            assert!(
                check(
                    feed.to_str().unwrap(),
                    "0.4.0-alpha.2",
                    Duration::from_secs(1)
                )
                .await
                .unwrap()
                .update_available
            );
            for version in [
                "0.4.0-alpha.3",
                "99.0.0-beta.1",
                "99.0.0-rc.1",
                "99.0.0-dev.1",
            ] {
                save(version);
                let report = check(feed.to_str().unwrap(), "0.3.2", Duration::from_secs(1))
                    .await
                    .unwrap();
                assert!(!report.update_available);
                assert!(report.prerelease_blocked);
                let error = apply_for(
                    feed.to_str().unwrap(),
                    dir.path(),
                    Duration::from_secs(1),
                    program,
                )
                .await
                .unwrap_err();
                assert!(error.to_string().contains("prerelease updates are blocked"));
                assert!(!dir.path().join("versions").exists());
                assert!(!dir.path().join("current.txt").exists());
            }
        }
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(is_newer("0.4.0", "0.4.0-dev.1"));
        assert!(!is_newer("0.4.0-dev.1", "0.4.0"));
        assert!(is_newer("0.4.0-dev.10", "0.4.0-dev.2"));
        assert!(is_newer("0.4.0", "0.4.0-alpha.1"));
        assert!(!is_newer("0.3.2", "0.4.0-alpha.1"));
        assert!(!is_newer("0.4.0-alpha.1", "0.4.0"));
        assert!(is_newer("0.4.0-alpha.2", "0.4.0-alpha.1"));
        assert!(validate_version("0.4.0a").is_err()); // public tag, not internal version
        for bad in [".", "..", "../escape", "1..0", ""] {
            assert!(validate_version(bad).is_err());
        }
    }

    #[test]
    fn installer_update_payload_is_recovered() {
        let zip = b"PK\x03\x04payload";
        let mut installer = b"stub".to_vec();
        installer.extend_from_slice(zip);
        installer.extend_from_slice(&(zip.len() as u64).to_le_bytes());
        installer.extend_from_slice(b"EEFINST1");
        assert_eq!(update_payload(&installer).unwrap(), zip);
        assert_eq!(update_payload(zip).unwrap(), zip);
        assert!(update_payload(b"not an update").is_err());
    }
}
