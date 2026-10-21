//! Explicit verified same-version package repair. No settings/model download,
//! in-place replacement, automatic retry, inference or restart authority.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bundle_selection::{self, BundleSelection};
use crate::updater::{self, UpdateApplied, UpdateManifest};

#[derive(Clone, Debug, clap::Args)]
pub struct RepairArgs {
    /// Exact installer/ZIP for this running build. Never an automatic feed change.
    #[arg(
        long,
        required_unless_present_any = ["rollback", "status"],
        conflicts_with_all = ["rollback", "status"]
    )]
    pub url: Option<String>,
    #[arg(
        long,
        required_unless_present_any = ["rollback", "status"],
        conflicts_with_all = ["rollback", "status"]
    )]
    pub sha256: Option<String>,
    #[arg(
        long,
        required_unless_present_any = ["rollback", "status"],
        conflicts_with_all = ["rollback", "status"]
    )]
    pub size_bytes: Option<u64>,
    /// Allow this explicitly selected prerelease package only.
    #[arg(long)]
    pub allow_prerelease: bool,
    /// Select the previous retained bundle; restart separately. No download.
    #[arg(long, conflicts_with_all = ["allow_prerelease", "status"])]
    pub rollback: bool,
    /// Inspect package selection without a running service or configuration file.
    #[arg(long, conflicts_with = "allow_prerelease")]
    pub status: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub allow_prerelease: bool,
}

impl RepairRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported package repair schema")
        }
        if uuid::Uuid::parse_str(&self.request_id)?.to_string() != self.request_id {
            bail!("repair request ID must be a canonical UUID")
        }
        updater::validate_artifact(
            &self.version,
            &self.url,
            &self.sha256,
            self.size_bytes,
            self.allow_prerelease,
        )
    }
}

impl From<&updater::ExplicitUpdate> for RepairRequest {
    fn from(value: &updater::ExplicitUpdate) -> Self {
        Self {
            schema_version: value.schema_version,
            request_id: value.request_id.clone(),
            version: value.version.clone(),
            url: value.url.clone(),
            sha256: value.sha256.clone(),
            size_bytes: value.size_bytes,
            allow_prerelease: value.allow_prerelease,
        }
    }
}

pub async fn execute_local(args: &RepairArgs, program: &str) -> Result<serde_json::Value> {
    let executable = std::env::current_exe()?;
    let install = updater::installation_root(&executable)?;
    if args.status {
        let selected = updater::installed_version(&install)?;
        let generation = selected
            .as_deref()
            .map(|version| bundle_selection::read(&install, version))
            .transpose()?
            .flatten();
        return Ok(
            serde_json::json!({"schema_version":1,"report_type":"package_status","success":true,
            "current_version":crate::VERSION,"installed_version":selected,"bundle_selection":generation,
            "restart_required":updater::restart_required(&install, crate::VERSION, program)?,
            "note":"Read-only package selection. A selected package is not proof that its dependencies work. No download, repair or restart was requested."}),
        );
    }
    if args.rollback {
        let version = updater::rollback_for(&install, program)?;
        return Ok(
            serde_json::json!({"schema_version":1,"report_type":"package_rollback","success":true,
            "selected_version":version,"restart_required":true,
            "note":"Previous bundle selected, not started. Restart explicitly; all pending settings also apply. No files were deleted."}),
        );
    }
    let request = RepairRequest {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        version: crate::VERSION.into(),
        url: args.url.clone().context("repair requires --url")?,
        sha256: args.sha256.clone().context("repair requires --sha256")?,
        size_bytes: args.size_bytes.context("repair requires --size-bytes")?,
        allow_prerelease: args.allow_prerelease,
    };
    let applied = apply_for(
        &request,
        &install,
        crate::VERSION,
        &executable,
        program,
        Duration::from_secs(120),
        || Ok(()),
    )
    .await?;
    Ok(
        serde_json::json!({"schema_version":1,"report_type":"explicit_repair","success":true,
        "request_id":request.request_id,"applied":applied,"automatic_policy_changed":false,
        "note":"Verified package selected. Restart the application explicitly; ordinary restart also applies other pending settings. Old files were preserved. No automatic retry or restart."}),
    )
}

pub async fn apply_for<G>(
    request: &RepairRequest,
    install: &Path,
    current_version: &str,
    running_executable: &Path,
    program: &str,
    timeout: Duration,
    authorize: impl Fn() -> Result<G>,
) -> Result<UpdateApplied> {
    request.validate()?;
    if request.version != current_version {
        bail!("repair must match the running version exactly")
    }
    drop(authorize()?);
    if !cfg!(windows) {
        bail!("verified package repair currently supports Windows distribution packages only")
    }
    let manifest = UpdateManifest {
        version: request.version.clone(),
        url: request.url.clone(),
        sha256: request.sha256.clone(),
        size_bytes: Some(request.size_bytes),
    };
    install_verified(
        &manifest,
        &request.request_id,
        install,
        current_version,
        running_executable,
        program,
        timeout,
        true,
        authorize,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn install_verified<G>(
    manifest: &UpdateManifest,
    request_id: &str,
    install: &Path,
    current_version: &str,
    running_executable: &Path,
    program: &str,
    timeout: Duration,
    https_only: bool,
    authorize: impl Fn() -> Result<G>,
) -> Result<UpdateApplied> {
    updater::validate_program(program)?;
    if manifest.version != current_version {
        bail!("repair must match the running version exactly")
    }
    let install = install.canonicalize()?;
    let previous_slot =
        bundle_selection::running_slot(&install, current_version, running_executable)?;
    let slot = bundle_selection::repair_slot(current_version, request_id)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(install.join(".update.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another update or repair is in progress")?;
    let selected = updater::installed_version(&install)?;
    if selected.as_deref().is_some_and(|v| v != current_version)
        || bundle_selection::pending_executable(
            &install,
            current_version,
            program,
            running_executable,
        )?
        .is_some()
    {
        bail!("an installed package is pending; restart and inspect before repair")
    }
    let versions = install.join("versions");
    fs::create_dir_all(&versions)?;
    let destination = versions.join(&slot);
    if destination.exists() {
        bail!("repair request already staged; inspect selection before retrying")
    }
    let size = manifest
        .size_bytes
        .context("repair requires exact package size")?;
    let bytes =
        updater::fetch_with_policy(&manifest.url, timeout, usize::try_from(size)?, https_only)
            .await?;
    if bytes.len() as u64 != size
        || !hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(&manifest.sha256)
    {
        bail!("repair package size or SHA-256 mismatch")
    }
    let staging = tempfile::Builder::new()
        .prefix(".repair-")
        .tempdir_in(&versions)?;
    updater::extract_zip(&bytes, staging.path())?;
    let binary = updater::find_binary(staging.path(), program)
        .context("repair package executable is missing")?;
    let bundle = binary
        .parent()
        .context("repair package directory is missing")?;
    let metadata = read_bounded(&bundle.join("bundle.json"), 65536)?;
    let metadata: serde_json::Value = serde_json::from_slice(&metadata)?;
    if metadata["name"] != program || metadata["version"] != current_version {
        bail!("repair package product/version does not match the running application")
    }
    // A same-version label alone must not smuggle in different executable code.
    if updater::sha256_file(&binary)? != updater::sha256_file(running_executable)? {
        bail!(
            "repair executable differs from the running build; use an explicit application update instead"
        )
    }
    verify_inventory(bundle, program)?;
    let _authorization_guard = authorize()?;
    // Nothing in the running bundle, config or model storage is overwritten.
    fs::rename(bundle, &destination)?;
    let selection = BundleSelection {
        schema_version: 1,
        version: current_version.into(),
        slot,
        previous_slot: Some(previous_slot),
    };
    // On a flat first install, publish the generation before current.txt. If the
    // second write fails the old flat program still runs; the orphan is retained.
    bundle_selection::write(&install, &selection)?;
    if selected.is_none() {
        crate::atomic_file::write(
            &install.join("current.txt"),
            format!("{current_version}\n").as_bytes(),
        )?;
    }
    Ok(UpdateApplied {
        new_version: current_version.into(),
        previous_version: selected,
        restart: true,
        install_dir: install,
    })
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("repair metadata exceeds its size limit")
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

fn verify_inventory(bundle: &Path, program: &str) -> Result<()> {
    let entries: Vec<InventoryEntry> = serde_json::from_slice(&read_bounded(
        &bundle.join("files.sha256.json"),
        4 * 1024 * 1024,
    )?)?;
    if entries.is_empty() || entries.len() > 20000 {
        bail!("invalid repair inventory size")
    }
    let mut paths = BTreeSet::new();
    for entry in entries {
        let path = entry.path.replace('\\', "/");
        if path.contains(':')
            || path.is_empty()
            || Path::new(&path)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || !paths.insert(path.to_ascii_lowercase())
        {
            bail!("unsafe or duplicate repair inventory path")
        }
        let target = bundle.join(&path);
        if !target.is_file()
            || fs::metadata(&target)?.len() != entry.bytes
            || !updater::sha256_file(&target)?.eq_ignore_ascii_case(&entry.sha256)
        {
            bail!("repair inventory verification failed")
        }
    }
    let mut required = vec![
        format!("{program}.exe"),
        "bundle.json".into(),
        "python/python.exe".into(),
    ];
    if program == "eefn" {
        required.push("tools/llama-server.exe".into());
        for name in [
            "tts",
            "system_clock",
            "screen_capture",
            "microphone",
            "keyboard",
            "input_control",
            "camera",
            "audio_output",
        ] {
            required.push(format!("python/plugins/{name}.py"));
        }
    }
    for required in required {
        if !paths.contains(&required) {
            bail!("repair inventory is missing a required bundled component")
        }
    }
    let mut directories = vec![bundle.to_path_buf()];
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            if count > 20000 {
                bail!("repair bundle contains too many paths")
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!("repair bundle contains a symbolic link")
            }
            if kind.is_dir() {
                directories.push(entry.path());
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(bundle)?
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase();
            if relative != "files.sha256.json" && !paths.contains(&relative) {
                bail!("repair bundle contains an unlisted file")
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Write};

    const VERSION: &str = "0.4.0-alpha.6";
    const EXECUTABLE: &[u8] = b"same executable fixture; never executed";

    #[test]
    fn package_repair_cli_requires_explicit_apply_or_read_only_recovery_mode() {
        use clap::Parser;
        #[derive(clap::Parser)]
        struct Owner {
            #[command(flatten)]
            artifact: RepairArgs,
        }
        assert!(Owner::try_parse_from(["repair"]).is_err());
        assert!(
            Owner::try_parse_from(["repair", "--status"])
                .unwrap()
                .artifact
                .status
        );
        assert!(
            Owner::try_parse_from(["repair", "--rollback"])
                .unwrap()
                .artifact
                .rollback
        );
        for args in [
            vec!["repair", "--status", "--rollback"],
            vec!["repair", "--status", "--url", "https://example.org/a"],
            vec!["repair", "--rollback", "--allow-prerelease"],
            vec!["repair", "--url", "https://example.org/a"],
        ] {
            assert!(Owner::try_parse_from(args).is_err());
        }
    }

    fn package(program: &str, mutation: &str) -> Vec<u8> {
        let mut files = vec![
            (
                format!("{program}.exe"),
                if mutation == "executable" {
                    b"other build".to_vec()
                } else {
                    EXECUTABLE.to_vec()
                },
            ),
            (
                "bundle.json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "name":if mutation == "product" { "other" } else {program},
                    "version":if mutation == "version" {"0.5.0"} else {VERSION},
                }))
                .unwrap(),
            ),
            (
                "python/python.exe".into(),
                b"fake Python; never executed".to_vec(),
            ),
        ];
        if program == "eefn" {
            files.push((
                "tools/llama-server.exe".into(),
                b"not an actual model server".to_vec(),
            ));
            for name in [
                "tts",
                "system_clock",
                "screen_capture",
                "microphone",
                "keyboard",
                "input_control",
                "camera",
                "audio_output",
            ] {
                files.push((
                    format!("python/plugins/{name}.py"),
                    b"fixture metadata only".to_vec(),
                ));
            }
        }
        if mutation == "missing_python" {
            files.retain(|(p, _)| p != "python/python.exe");
        }
        let mut inventory: Vec<_> = files
            .iter()
            .map(|(path, bytes)| {
                serde_json::json!({
            "path":path,"bytes":bytes.len(),"sha256":hex::encode(Sha256::digest(bytes))})
            })
            .collect();
        match mutation {
            "inventory_digest" => inventory[0]["sha256"] = serde_json::json!("0".repeat(64)),
            "inventory_escape" => inventory[0]["path"] = serde_json::json!("../escape"),
            "inventory_duplicate" => inventory.push(inventory[0].clone()),
            "unlisted" => files.push(("unknown.dll".into(), b"unlisted fixture".to_vec())),
            _ => {}
        }
        if mutation != "missing_inventory" {
            files.push((
                "files.sha256.json".into(),
                serde_json::to_vec(&inventory).unwrap(),
            ));
        }
        let mut archive = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        for (path, bytes) in files {
            // Nested payloads are normalized to a direct bundle during install.
            archive
                .start_file(
                    format!("package/{path}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    fn fixture(
        root: &Path,
        program: &str,
        versioned: bool,
        mutation: &str,
    ) -> (std::path::PathBuf, UpdateManifest) {
        let bundle = if versioned {
            root.join("versions").join(VERSION)
        } else {
            root.to_path_buf()
        };
        fs::create_dir_all(&bundle).unwrap();
        let executable = bundle.join(format!("{program}.exe"));
        fs::write(&executable, EXECUTABLE).unwrap();
        fs::write(
            root.join("config.json"),
            b"owner configuration and stable-only policy",
        )
        .unwrap();
        fs::write(
            root.join("retained-model.gguf"),
            b"synthetic model; do not alter",
        )
        .unwrap();
        if versioned {
            fs::write(root.join("current.txt"), VERSION).unwrap();
        }
        let bytes = package(program, mutation);
        let path = root.join("fixture.zip");
        fs::write(&path, &bytes).unwrap();
        (
            executable,
            UpdateManifest {
                version: VERSION.into(),
                url: path.to_string_lossy().into(),
                sha256: hex::encode(Sha256::digest(&bytes)),
                size_bytes: Some(bytes.len() as u64),
            },
        )
    }

    #[tokio::test]
    async fn package_repair_stages_exact_flat_and_versioned_bundles_and_rolls_back() {
        for program in ["eef", "eefn"] {
            for versioned in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path();
                let (executable, manifest) = fixture(root, program, versioned, "valid");
                let request = uuid::Uuid::new_v4().to_string();
                let old_slot = bundle_selection::running_slot(root, VERSION, &executable).unwrap();
                let applied = install_verified(
                    &manifest,
                    &request,
                    root,
                    VERSION,
                    &executable,
                    program,
                    Duration::from_secs(1),
                    false,
                    || Ok(()),
                )
                .await
                .unwrap();
                assert!(applied.restart);
                assert_eq!(applied.new_version, VERSION);
                let selection = bundle_selection::read(root, VERSION).unwrap().unwrap();
                assert_eq!(selection.previous_slot.as_deref(), Some(old_slot.as_str()));
                let selected =
                    bundle_selection::pending_executable(root, VERSION, program, &executable)
                        .unwrap()
                        .unwrap();
                assert_eq!(fs::read(&selected).unwrap(), EXECUTABLE);
                assert_eq!(
                    updater::installation_root(&selected).unwrap(),
                    root.canonicalize().unwrap()
                );
                assert!(
                    bundle_selection::pending_executable(root, VERSION, program, &selected)
                        .unwrap()
                        .is_none()
                );
                assert!(
                    install_verified(
                        &manifest,
                        &request,
                        root,
                        VERSION,
                        &executable,
                        program,
                        Duration::from_secs(1),
                        false,
                        || Ok(())
                    )
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("pending")
                );
                assert_eq!(updater::rollback_for(root, program).unwrap(), VERSION);
                assert!(
                    bundle_selection::pending_executable(root, VERSION, program, &executable)
                        .unwrap()
                        .is_none()
                );
                assert!(selected.is_file(), "rollback retains the repaired package");
                assert!(
                    install_verified(
                        &manifest,
                        &request,
                        root,
                        VERSION,
                        &executable,
                        program,
                        Duration::from_secs(1),
                        false,
                        || Ok(())
                    )
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("already staged")
                );
                assert_eq!(fs::read(&executable).unwrap(), EXECUTABLE);
                assert_eq!(
                    fs::read(root.join("config.json")).unwrap(),
                    b"owner configuration and stable-only policy"
                );
                assert_eq!(
                    fs::read(root.join("retained-model.gguf")).unwrap(),
                    b"synthetic model; do not alter"
                );
            }
        }
    }

    #[tokio::test]
    async fn package_repair_rejects_mismatched_incomplete_or_unlisted_packages_before_selection() {
        for mutation in [
            "executable",
            "product",
            "version",
            "missing_python",
            "missing_inventory",
            "inventory_digest",
            "inventory_escape",
            "inventory_duplicate",
            "unlisted",
            "sha",
            "size",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let (executable, mut manifest) = fixture(root, "eefn", true, mutation);
            if mutation == "sha" {
                manifest.sha256 = "0".repeat(64);
            }
            if mutation == "size" {
                *manifest.size_bytes.as_mut().unwrap() += 1;
            }
            assert!(
                install_verified(
                    &manifest,
                    &uuid::Uuid::new_v4().to_string(),
                    root,
                    VERSION,
                    &executable,
                    "eefn",
                    Duration::from_secs(1),
                    false,
                    || Ok(())
                )
                .await
                .is_err(),
                "{mutation}"
            );
            assert!(
                bundle_selection::read(root, VERSION).unwrap().is_none(),
                "{mutation}"
            );
            assert_eq!(
                fs::read_to_string(root.join("current.txt")).unwrap(),
                VERSION
            );
            assert_eq!(fs::read(&executable).unwrap(), EXECUTABLE);
            assert_eq!(
                fs::read_dir(root.join("versions")).unwrap().count(),
                1,
                "owned staging cleaned on failure"
            );
        }
    }

    #[tokio::test]
    async fn package_repair_honors_update_lock_final_revocation_and_pending_selection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (executable, manifest) = fixture(root, "eefn", true, "valid");
        let request_id = uuid::Uuid::new_v4().to_string();
        assert!(
            install_verified(
                &manifest,
                &request_id,
                root,
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                false,
                || -> Result<()> { bail!("grant revoked") }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("revoked")
        );
        assert!(bundle_selection::read(root, VERSION).unwrap().is_none());
        let lock = File::options()
            .read(true)
            .write(true)
            .open(root.join(".update.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        assert!(
            install_verified(
                &manifest,
                &request_id,
                root,
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                false,
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("in progress")
        );
        drop(lock);
        fs::write(root.join("current.txt"), "0.5.0").unwrap();
        assert!(
            install_verified(
                &manifest,
                &request_id,
                root,
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                false,
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("pending")
        );
        assert!(bundle_selection::read(root, VERSION).unwrap().is_none());
    }

    #[tokio::test]
    async fn package_repair_public_request_requires_exact_consent_before_network_io() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("eefn.exe");
        fs::write(&executable, EXECUTABLE).unwrap();
        let mut request = RepairRequest {
            schema_version: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            version: VERSION.into(),
            url: "https://example.invalid/never-downloaded".into(),
            sha256: "a".repeat(64),
            size_bytes: 1,
            allow_prerelease: false,
        };
        assert!(
            apply_for(
                &request,
                dir.path(),
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("allow_prerelease")
        );
        request.allow_prerelease = true;
        assert!(
            apply_for(
                &request,
                dir.path(),
                "0.5.0",
                &executable,
                "eefn",
                Duration::from_secs(1),
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("match")
        );
        assert!(
            apply_for(
                &request,
                dir.path(),
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                || -> Result<()> { bail!("denied before I/O") }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("denied")
        );
        request.url = "file:///untrusted".into();
        assert!(request.validate().is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn package_repair_selector_lock_preserves_old_pointer_without_copy_fallback() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (executable, manifest) = fixture(root, "eefn", true, "valid");
        let selection = BundleSelection {
            schema_version: 1,
            version: VERSION.into(),
            slot: VERSION.into(),
            previous_slot: None,
        };
        bundle_selection::write(root, &selection).unwrap();
        let pointer = root.join("versions").join(format!("{VERSION}.bundle.json"));
        let original = fs::read(&pointer).unwrap();
        let locked = File::options()
            .read(true)
            .share_mode(3)
            .open(&pointer)
            .unwrap();
        let request_id = uuid::Uuid::new_v4().to_string();
        assert!(
            install_verified(
                &manifest,
                &request_id,
                root,
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                false,
                || Ok(())
            )
            .await
            .is_err()
        );
        assert_eq!(fs::read(&pointer).unwrap(), original);
        assert_eq!(fs::read(&executable).unwrap(), EXECUTABLE);
        let staged = bundle_selection::directory(
            root,
            VERSION,
            &bundle_selection::repair_slot(VERSION, &request_id).unwrap(),
        )
        .unwrap();
        assert!(
            staged.join("eefn.exe").is_file(),
            "uncertain staging is retained, never overwritten/deleted"
        );
        drop(locked);
        assert!(
            install_verified(
                &manifest,
                &request_id,
                root,
                VERSION,
                &executable,
                "eefn",
                Duration::from_secs(1),
                false,
                || Ok(())
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("already staged")
        );
        assert_eq!(fs::read(&pointer).unwrap(), original);
    }

    #[test]
    fn package_repair_archive_rejects_case_collisions_traversal_and_expansion() {
        for names in [["A.dll", "a.dll"], ["../escape", "safe"]] {
            let dir = tempfile::tempdir().unwrap();
            let mut archive = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
            for name in names {
                archive
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                archive.write_all(b"fixture").unwrap();
            }
            let bytes = archive.finish().unwrap().into_inner();
            assert!(updater::extract_zip(&bytes, dir.path()).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = package("eef", "valid");
        let central = bytes.windows(4).position(|p| p == b"PK\x01\x02").unwrap();
        bytes[central + 24..central + 28]
            .copy_from_slice(&(3_u32 * 1024 * 1024 * 1024).to_le_bytes());
        assert!(
            updater::extract_zip(&bytes, dir.path())
                .unwrap_err()
                .to_string()
                .contains("2 GiB")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
