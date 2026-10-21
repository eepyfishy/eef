//! Owner-selected HTTPS source proposals. Checking is not repair authority;
//! approval binds exact source bytes, current build/slot and one operation ID.
//! No URL is retained in operation stores. A packaged public source can be
//! displayed for owner review; explicit/private URLs are never echoed.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{bundle_selection, package_repair, updater};

const MAX_PLANS: usize = 128;
const MAX_STORE: usize = 256 * 1024;
const MAX_MANIFEST: usize = 16 * 1024;
const MAX_HINT: usize = 8 * 1024;
const LIFETIME_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Refusal(&'static str);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceHint {
    schema_version: u32,
    program: String,
    version: String,
    manifest_url: String,
}

struct SelectedSource {
    url: String,
    hint_sha256: Option<String>,
}

impl SelectedSource {
    fn summary(&self) -> Value {
        json!({"schema_version":1,"source_kind":if self.hint_sha256.is_some() {"packaged_hint"} else {"explicit"},
            "publisher_source":self.hint_sha256.as_ref().map(|_|self.url.as_str()),
            "publisher_signature_verified":false,"authority_granted":false})
    }

    fn ensure_current(&self, executable: &Path, program: &str, version: &str) -> Result<()> {
        if self.hint_sha256.is_some() {
            let current = packaged_source(executable, program, version)
                .context(Refusal("repair_source_hint_changed"))?;
            if current.url != self.url || current.hint_sha256 != self.hint_sha256 {
                bail!(Refusal("repair_source_hint_changed"))
            }
        }
        Ok(())
    }
}

fn packaged_source(executable: &Path, program: &str, version: &str) -> Result<SelectedSource> {
    let layout = crate::runtime_layout::RuntimeLayout::from_executable(executable)
        .context(Refusal("repair_source_build_unavailable"))?;
    let path = layout
        .repair_source_hint(program)?
        .context(Refusal("repair_source_unconfigured"))?;
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!(Refusal("repair_source_unconfigured"))
        }
        Err(_) => bail!(Refusal("repair_source_hint_unavailable")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_HINT as u64
    {
        bail!(Refusal("repair_source_hint_invalid"))
    }
    let mut bytes = Vec::new();
    File::open(path)
        .context(Refusal("repair_source_hint_unavailable"))?
        .take(MAX_HINT as u64 + 1)
        .read_to_end(&mut bytes)
        .context(Refusal("repair_source_hint_unavailable"))?;
    if bytes.len() > MAX_HINT {
        bail!(Refusal("repair_source_hint_invalid"))
    }
    let hint: SourceHint =
        serde_json::from_slice(&bytes).context(Refusal("repair_source_hint_invalid"))?;
    if hint.schema_version != 1 || hint.program != program || hint.version != version {
        bail!(Refusal("repair_source_hint_invalid"))
    }
    source_url(&hint.manifest_url).context(Refusal("repair_source_hint_invalid"))?;
    let parsed =
        reqwest::Url::parse(&hint.manifest_url).context(Refusal("repair_source_hint_invalid"))?;
    // This file is distributed as public package metadata, not a credential
    // store. Private/presigned descriptor links remain explicit per-call input.
    if parsed.query().is_some() {
        bail!(Refusal("repair_source_hint_invalid"))
    }
    Ok(SelectedSource {
        url: hint.manifest_url,
        hint_sha256: Some(digest(&bytes)),
    })
}

fn selected_source(
    args: &package_repair::RepairArgs,
    executable: &Path,
    program: &str,
    version: &str,
) -> Result<SelectedSource> {
    match &args.manifest {
        Some(url) => Ok(SelectedSource {
            url: url.clone(),
            hint_sha256: None,
        }),
        None => packaged_source(executable, program, version),
    }
}

pub(crate) fn hint_status(executable: &Path, program: &str, version: &str) -> Value {
    match packaged_source(executable, program, version) {
        Ok(source) => {
            let mut value = source.summary();
            value["state"] = json!("configured");
            value["network_inspected"] = json!(false);
            value
        }
        Err(error) => {
            let code = error
                .downcast_ref::<Refusal>()
                .map(|e| e.0)
                .unwrap_or("repair_source_hint_unavailable");
            json!({"schema_version":1,"state":if code=="repair_source_unconfigured" {"not_configured"} else {"unavailable"},
                "error_code":code,"publisher_source":null,"authority_granted":false,
                "publisher_signature_verified":false,"network_inspected":false})
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceManifest {
    pub schema_version: u32,
    pub program: String,
    pub version: String,
    pub executable_sha256: String,
    pub artifact: SourceArtifact,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceArtifact {
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    id: String,
    program: String,
    version: String,
    executable_sha256: String,
    running_slot: String,
    source_url_sha256: String,
    manifest_sha256: String,
    artifact_sha256: String,
    artifact_bytes: u64,
    allow_prerelease: bool,
    created_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plans {
    schema_version: u32,
    plans: Vec<Plan>,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

fn store_path(install: &Path) -> PathBuf {
    install.join(".package-repair-plans.json")
}

fn read_store(install: &Path) -> Result<(Plans, Option<Vec<u8>>)> {
    let file = match File::open(store_path(install)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                Plans {
                    schema_version: 1,
                    plans: vec![],
                },
                None,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STORE as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_STORE {
        bail!("repair proposal store exceeds its bound")
    }
    let plans: Plans = serde_json::from_slice(&bytes)?;
    if plans.schema_version != 1 || plans.plans.len() > MAX_PLANS {
        bail!("unsupported or excessive repair proposals")
    }
    let mut ids = std::collections::BTreeSet::new();
    for plan in &plans.plans {
        crate::repair_receipts::validate_id(&plan.id)?;
        updater::validate_version(&plan.version)?;
        if !matches!(plan.program.as_str(), "eef" | "eefn")
            || !ids.insert(&plan.id)
            || [
                &plan.executable_sha256,
                &plan.source_url_sha256,
                &plan.manifest_sha256,
                &plan.artifact_sha256,
            ]
            .iter()
            .any(|hash| !valid_digest(hash))
            || plan.artifact_bytes == 0
            || plan.artifact_bytes > 512 * 1024 * 1024
            || plan.expires_at_ms.checked_sub(plan.created_at_ms) != Some(LIFETIME_MS)
            || (plan.version.contains('-') && !plan.allow_prerelease)
            || bundle_selection::validate_slot(&plan.running_slot, &plan.version).is_err()
        {
            bail!("invalid retained repair proposal")
        }
    }
    Ok((plans, Some(bytes)))
}

fn retain(install: &Path, plan: &Plan) -> Result<()> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(install.join(".update.lock"))?;
    lock.try_lock_exclusive()
        .context("another update/repair is active")?;
    let (mut plans, before) = read_store(install)?;
    if plans.plans.len() >= MAX_PLANS || plans.plans.iter().any(|p| p.id == plan.id) {
        bail!("repair proposal capacity reached or ID already retained")
    }
    plans.plans.push(plan.clone());
    let bytes = serde_json::to_vec(&plans)?;
    if bytes.len() > MAX_STORE {
        bail!("repair proposal store capacity reached")
    }
    let (_, current) = read_store(install)?;
    if current != before {
        bail!("repair proposals changed before publication")
    }
    if before.is_some() {
        crate::atomic_file::write(&store_path(install), &bytes)?;
    } else {
        let mut pending = tempfile::Builder::new()
            .prefix(".repair-plan-")
            .tempfile_in(install)?;
        pending.write_all(&bytes)?;
        pending.as_file().sync_all()?;
        pending
            .persist_noclobber(store_path(install))
            .map_err(|error| error.error)?;
    }
    Ok(())
}

fn find(install: &Path, id: &str) -> Result<Plan> {
    crate::repair_receipts::validate_id(id)?;
    read_store(install)?
        .0
        .plans
        .into_iter()
        .find(|p| p.id == id)
        .context("repair proposal unavailable; check the source again")
}

pub(crate) fn status(install: &Path, program: &str, id: Option<&str>) -> Value {
    let inspected = (|| -> Result<Value> {
        if !matches!(program, "eef" | "eefn") {
            bail!("unsupported repair product")
        }
        if let Some(id) = id {
            crate::repair_receipts::validate_id(id)?;
        }
        let (store, _) = read_store(install)?;
        let now = crate::protocol::now_ms();
        let plans: Vec<_> = store
            .plans
            .iter()
            .filter(|p| p.program == program && id.is_none_or(|id| p.id == id))
            .map(|p| {
                let mut value = serde_json::to_value(p).expect("typed repair proposal");
                value["within_approval_time_window"] =
                    json!(now >= p.created_at_ms && now < p.expires_at_ms);
                value
            })
            .collect();
        Ok(
            json!({"schema_version":1,"available":true,"stored_count":store.plans.len(),
            "record_limit":MAX_PLANS,"byte_limit":MAX_STORE,"plans":plans,"approval_authorized":false,
            "repair_admission_inspected":false,
            "note":"Retained metadata only. Time window is not build/consent/source/admission eligibility. No approval, download, recovery or rewrite."}),
        )
    })();
    inspected.unwrap_or_else(|_| {
        json!({"schema_version":1,"available":false,"plans":null,
        "approval_authorized":false,"error_code":"repair_source_plans_unavailable",
        "note":"Repair source proposals unavailable; retained without reset, pruning or approval."})
    })
}

fn source_url(url: &str) -> Result<()> {
    updater::validate_https_artifact(url)
}

fn running(
    install: &Path,
    executable: &Path,
    program: &str,
    version: &str,
) -> Result<(String, String)> {
    if !matches!(program, "eef" | "eefn") {
        bail!("unsupported repair product")
    }
    updater::validate_version(version)?;
    let slot = bundle_selection::running_slot(install, version, executable)?;
    if updater::installed_version(install)?.is_some_and(|v| v != version)
        || bundle_selection::pending_executable(install, version, program, executable)?.is_some()
    {
        bail!("a package is pending; restart and inspect first")
    }
    Ok((slot, updater::sha256_file(executable)?))
}

fn unadmitted(install: &Path, program: &str, id: &str) -> Result<()> {
    if !matches!(program, "eef" | "eefn") {
        bail!("unsupported repair product")
    }
    for role in ["eef", "eefn"] {
        let receipts = package_repair::receipt_status(install, role, Some(id));
        if receipts["available"] != true
            || receipts["attempts"]
                .as_array()
                .is_none_or(|a| !a.is_empty())
        {
            bail!("repair evidence unavailable or operation already admitted")
        }
    }
    Ok(())
}

fn parse(
    bytes: &[u8],
    program: &str,
    version: &str,
    executable_sha256: &str,
    allow_prerelease: bool,
) -> Result<SourceManifest> {
    if bytes.len() > MAX_MANIFEST {
        bail!("repair source descriptor too large")
    }
    let source: SourceManifest = serde_json::from_slice(bytes)?;
    if source.schema_version != 1
        || source.program != program
        || source.version != version
        || !source
            .executable_sha256
            .eq_ignore_ascii_case(executable_sha256)
    {
        bail!("repair source does not match the exact running build")
    }
    updater::validate_artifact(
        &source.version,
        &source.artifact.url,
        &source.artifact.sha256,
        source.artifact.size_bytes,
        allow_prerelease,
    )?;
    Ok(source)
}

fn propose(
    install: &Path,
    executable: &Path,
    program: &str,
    version: &str,
    url: &str,
    bytes: &[u8],
    allow_prerelease: bool,
) -> Result<Value> {
    source_url(url)?;
    let (slot, executable_sha256) = running(install, executable, program, version)?;
    let source = parse(
        bytes,
        program,
        version,
        &executable_sha256,
        allow_prerelease,
    )?;
    let now = crate::protocol::now_ms();
    let plan = Plan {
        id: uuid::Uuid::new_v4().to_string(),
        program: program.into(),
        version: version.into(),
        executable_sha256,
        running_slot: slot,
        source_url_sha256: digest(url.as_bytes()),
        manifest_sha256: digest(bytes),
        artifact_sha256: source.artifact.sha256.to_ascii_lowercase(),
        artifact_bytes: source.artifact.size_bytes,
        allow_prerelease,
        created_at_ms: now,
        expires_at_ms: now.checked_add(LIFETIME_MS).context("clock overflow")?,
    };
    unadmitted(install, program, &plan.id)?;
    retain(install, &plan)?;
    Ok(
        json!({"schema_version":1,"report_type":"repair_source_plan","success":true,"plan":plan,
        "plan_retained":true,"repair_admitted":false,"artifact_download_started":false,
        "automatic_policy_changed":false,"restart_requested":false,"publisher_signature_verified":false,
        "note":"Proposal only. Approve this exact source and retained build separately. No automatic repair, retry, deletion, restart or publisher-signature/health claim."}),
    )
}

fn validate_plan(
    install: &Path,
    executable: &Path,
    program: &str,
    version: &str,
    plan: &Plan,
    allow_prerelease: bool,
) -> Result<()> {
    let now = crate::protocol::now_ms();
    let (slot, executable_sha256) = running(install, executable, program, version)?;
    if plan.program != program
        || plan.version != version
        || plan.running_slot != slot
        || plan.executable_sha256 != executable_sha256
        || plan.allow_prerelease != allow_prerelease
        || now < plan.created_at_ms
        || now >= plan.expires_at_ms
        || find(install, &plan.id)? != *plan
    {
        bail!("repair proposal expired, changed or differs from the running build/consent")
    }
    Ok(())
}

fn resolve(
    install: &Path,
    executable: &Path,
    program: &str,
    version: &str,
    plan: &Plan,
    url: &str,
    bytes: &[u8],
    allow_prerelease: bool,
) -> Result<package_repair::RepairRequest> {
    source_url(url)?;
    validate_plan(
        install,
        executable,
        program,
        version,
        plan,
        allow_prerelease,
    )?;
    unadmitted(install, program, &plan.id)?;
    if digest(url.as_bytes()) != plan.source_url_sha256 || digest(bytes) != plan.manifest_sha256 {
        bail!(
            "repair source changed after proposal; check again rather than approving replacement bytes"
        )
    }
    let source = parse(
        bytes,
        program,
        version,
        &plan.executable_sha256,
        allow_prerelease,
    )?;
    if source.artifact.sha256.to_ascii_lowercase() != plan.artifact_sha256
        || source.artifact.size_bytes != plan.artifact_bytes
    {
        bail!("repair artifact differs from retained proposal")
    }
    Ok(package_repair::RepairRequest {
        schema_version: 1,
        request_id: plan.id.clone(),
        version: version.into(),
        url: source.artifact.url,
        sha256: plan.artifact_sha256.clone(),
        size_bytes: plan.artifact_bytes,
        allow_prerelease,
    })
}

/// Shared backend entrypoint for local coordinator/node adapters. The manifest
/// URL is owner-selected for this call, never silently taken from an update feed.
pub async fn execute(
    args: &package_repair::RepairArgs,
    install: &Path,
    executable: &Path,
    program: &str,
) -> Result<Value> {
    execute_with_fetch(args, install, executable, program, |url| async move {
        updater::fetch_with_policy(&url, Duration::from_secs(20), MAX_MANIFEST, true).await
    })
    .await
}

async fn execute_with_fetch<F, T>(
    args: &package_repair::RepairArgs,
    install: &Path,
    executable: &Path,
    program: &str,
    fetch: F,
) -> Result<Value>
where
    F: FnOnce(String) -> T,
    T: std::future::Future<Output = Result<Vec<u8>>>,
{
    let prepared = async {
        if args.check == args.approve.is_some()
            || args.status
            || args.rollback
            || args.operation_id.is_some()
            || args.url.is_some()
            || args.sha256.is_some()
            || args.size_bytes.is_some()
        {
            bail!(Refusal("repair_source_arguments"))
        }
        let source = selected_source(args, executable, program, crate::VERSION)?;
        let url = source.url.as_str();
        source_url(url).context(Refusal("repair_source_requires_https"))?;
        running(install, executable, program, crate::VERSION)
            .context(Refusal("repair_source_build_unavailable"))?;
        if crate::VERSION.contains('-') && !args.allow_prerelease {
            bail!(Refusal("repair_source_prerelease_consent_required"))
        }
        let plan = if let Some(id) = &args.approve {
            let plan = find(install, id).context(Refusal("repair_source_proposal_unavailable"))?;
            validate_plan(
                install,
                executable,
                program,
                crate::VERSION,
                &plan,
                args.allow_prerelease,
            )
            .context(Refusal("repair_source_proposal_ineligible"))?;
            unadmitted(install, program, id).context(Refusal(
                "repair_source_already_admitted_or_evidence_unavailable",
            ))?;
            if digest(url.as_bytes()) != plan.source_url_sha256 {
                bail!(Refusal("repair_source_url_changed"))
            }
            Some(plan)
        } else {
            if !args.check
                || read_store(install)
                    .context(Refusal("repair_source_store_unavailable"))?
                    .0
                    .plans
                    .len()
                    >= MAX_PLANS
            {
                bail!(Refusal("repair_source_capacity_reached"))
            }
            None
        };
        let bytes = fetch(url.to_owned())
            .await
            .context(Refusal("repair_source_fetch_failed"))?;
        source.ensure_current(executable, program, crate::VERSION)?;
        if let Some(plan) = plan {
            let request = resolve(
                install,
                executable,
                program,
                crate::VERSION,
                &plan,
                url,
                &bytes,
                args.allow_prerelease,
            )
            .context(Refusal("repair_source_proposal_changed"))?;
            Ok((None, Some((plan, request, source))))
        } else {
            let mut report = propose(
                install,
                executable,
                program,
                crate::VERSION,
                url,
                &bytes,
                args.allow_prerelease,
            )
            .context(Refusal("repair_source_descriptor_rejected"))?;
            report["source"] = source.summary();
            Ok::<_, anyhow::Error>((Some(report), None))
        }
    }
    .await;
    match prepared {
        Ok((Some(report), _)) => Ok(report),
        Ok((_, Some((plan, request, source)))) => {
            let result = package_repair::apply_for(
                &request,
                install,
                crate::VERSION,
                executable,
                program,
                Duration::from_secs(120),
                || {
                    source.ensure_current(executable, program, crate::VERSION)?;
                    validate_plan(
                        install,
                        executable,
                        program,
                        crate::VERSION,
                        &plan,
                        args.allow_prerelease,
                    )
                },
            )
            .await;
            Ok(package_repair::application_report(
                &request, install, program, result,
            ))
        }
        failed => {
            let code = match failed {
                Err(error) => error
                    .downcast_ref::<Refusal>()
                    .map(|e| e.0)
                    .unwrap_or("repair_source_not_approved"),
                _ => "repair_source_not_approved",
            };
            let request_id = args
                .approve
                .as_deref()
                .filter(|id| crate::repair_receipts::validate_id(id).is_ok());
            Ok(
                json!({"schema_version":1,"report_type":"repair_source_refused","success":false,
            "error_code":code,"request_id":request_id,"new_repair_admission_started":false,"artifact_download_started_by_this_call":false,
            "automatic_policy_changed":false,"restart_requested":false,
            "note":"Source/retained proposal not approved. Check HTTPS source, exact running build, proposal capacity/expiry and prerelease consent. Existing evidence is not reset. Inspect repair --status for earlier attempts."}),
            )
        }
    }
}

#[cfg(test)]
#[path = "repair_source_tests.rs"]
mod tests;
