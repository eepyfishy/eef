//! Bounded durable repair admission/evidence, never retry, rollback or health
//! authority. Reads do not create files, recover records or claim liveness from
//! a persisted phase. A live per-operation file lease supplies that observation.
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_RECORDS: usize = 128;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Step {
    Admitted,
    Downloading,
    Verifying,
    Staging,
    Staged,
    Selecting,
    Selected,
}

impl Step {
    fn ordinal(self) -> u8 {
        match self {
            Self::Admitted => 0,
            Self::Downloading => 1,
            Self::Verifying => 2,
            Self::Staging => 3,
            Self::Staged => 4,
            Self::Selecting => 5,
            Self::Selected => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    InProgress,
    Selected,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    sequence: u32,
    request_id: String,
    program: String,
    version: String,
    definition_sha256: String,
    artifact_sha256: String,
    artifact_bytes: u64,
    admitted_at_ms: u64,
    updated_at_ms: u64,
    step: Step,
    outcome: Outcome,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Records {
    schema_version: u32,
    records: Vec<Receipt>,
}

fn path(install: &Path) -> PathBuf {
    install.join(".package-repairs.json")
}
fn lease_path(install: &Path, id: &str) -> PathBuf {
    install
        .join(".package-repair-leases")
        .join(format!("{id}.lock"))
}

pub(crate) fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id)?.to_string() != id || uuid::Uuid::parse_str(id)?.is_nil() {
        bail!("repair operation ID must be a non-nil canonical UUID")
    }
    Ok(())
}

fn validate_program(program: &str) -> Result<()> {
    if !matches!(program, "eef" | "eefn") {
        bail!("unsupported repair receipt product")
    }
    Ok(())
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn raw(install: &Path) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path(install)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        bail!("repair receipt journal exceeds its bound; retained without repair")
    }
    Ok(Some(bytes))
}

fn read(install: &Path) -> Result<Records> {
    let Some(bytes) = raw(install)? else {
        if !leases(install)?.is_empty() {
            bail!("repair journal missing with retained leases; never assume a fresh installation")
        }
        return Ok(Records {
            schema_version: 1,
            records: vec![],
        });
    };
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<Records> {
    let records: Records = serde_json::from_slice(bytes)
        .context("repair receipt journal is damaged; retained without repair")?;
    if records.schema_version != 1 || records.records.len() > MAX_RECORDS {
        bail!("unsupported repair receipt journal schema/size")
    }
    let mut ids = BTreeSet::new();
    for (index, record) in records.records.iter().enumerate() {
        validate_id(&record.request_id)?;
        validate_program(&record.program)?;
        crate::updater::validate_version(&record.version)?;
        if record.schema_version != 1
            || record.sequence as usize != index + 1
            || !ids.insert(&record.request_id)
            || !digest(&record.definition_sha256)
            || !digest(&record.artifact_sha256)
            || record.artifact_bytes == 0
            || record.artifact_bytes > 512 * 1024 * 1024
            || record.admitted_at_ms == 0
            || record.updated_at_ms < record.admitted_at_ms
            || record.updated_at_ms > i64::MAX as u64
            || (record.outcome == Outcome::Selected) != (record.step == Step::Selected)
        {
            bail!("damaged repair receipt; retained without repair")
        }
    }
    Ok(records)
}

fn write(install: &Path, records: &Records) -> Result<String> {
    let bytes = serde_json::to_vec(records)?;
    if bytes.len() > MAX_BYTES {
        bail!("repair receipt journal byte quota is full")
    }
    crate::atomic_file::write(&path(install), &bytes).context("publish complete repair receipt")?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn initialize(install: &Path) -> Result<String> {
    let bytes = serde_json::to_vec(&Records {
        schema_version: 1,
        records: vec![],
    })?;
    let mut pending = tempfile::Builder::new()
        .prefix(".eef-write-")
        .tempfile_in(install)?;
    pending.write_all(&bytes)?;
    pending.as_file().sync_all()?;
    // A concurrent owner-created journal must never be overwritten as "empty".
    drop(
        pending
            .persist_noclobber(path(install))
            .map_err(|error| error.error)?,
    );
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn guarded_write(install: &Path, records: &Records, expected: &str) -> Result<String> {
    let bytes = raw(install)?.context("repair journal disappeared; no recovery or reset")?;
    if hex::encode(Sha256::digest(bytes)) != expected {
        bail!("repair journal changed; retained without overwrite")
    }
    write(install, records)
}

fn leases(install: &Path) -> Result<BTreeSet<String>> {
    let directory = match std::fs::read_dir(install.join(".package-repair-leases")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    let mut ids = BTreeSet::new();
    for entry in directory {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("invalid repair lease name")?;
        let id = name
            .strip_suffix(".lock")
            .context("unrecognized repair lease file; retained")?;
        validate_id(id)?;
        if !entry.file_type()?.is_file() || entry.metadata()?.len() != 0 {
            bail!("damaged repair lease; retained")
        }
        ids.insert(id.into());
        if ids.len() > MAX_RECORDS {
            bail!("repair lease directory exceeds its bound")
        }
    }
    Ok(ids)
}

pub(crate) struct Attempt {
    install: PathBuf,
    records: Records,
    index: usize,
    expected_sha256: String,
    _lease: File,
}

impl Attempt {
    /// Caller holds the shared installation update lock through the whole
    /// attempt. Admission is durable before artifact work begins; the lease is
    /// already live when that admission becomes visible.
    pub(crate) fn begin(
        install: &Path,
        program: &str,
        request: &crate::package_repair::RepairRequest,
        executable_sha256: &str,
        running_slot: &str,
    ) -> Result<Self> {
        validate_program(program)?;
        validate_id(&request.request_id)?;
        crate::updater::validate_version(&request.version)?;
        if request.schema_version != 1
            || !digest(&request.sha256)
            || !digest(executable_sha256)
            || request.size_bytes == 0
            || request.size_bytes > 512 * 1024 * 1024
        {
            bail!("invalid repair admission binding")
        }
        let original = raw(install)?;
        let mut records = match original.as_deref() {
            Some(bytes) => decode(bytes)?,
            None => read(install)?,
        };
        if records
            .records
            .iter()
            .any(|r| r.request_id == request.request_id)
        {
            bail!("repair operation ID was already admitted; inspect its receipt, never replay it")
        }
        if records.records.len() >= MAX_RECORDS {
            bail!("repair receipt quota is full; no work admitted")
        }
        let leases = leases(install)?;
        if !leases.contains(&request.request_id) && leases.len() >= MAX_RECORDS {
            bail!("repair lease quota is full; no work admitted")
        }
        let expected = match original {
            Some(bytes) => hex::encode(Sha256::digest(bytes)),
            None => initialize(install)?,
        };
        // Retain only digests of potentially private URLs; never the URL, owner
        // configuration or secrets. Source/consent/executable/slot bind identity.
        #[derive(Serialize)]
        struct Binding<'a> {
            schema_version: u32,
            request: &'a crate::package_repair::RepairRequest,
            program: &'a str,
            executable_sha256: &'a str,
            running_slot: &'a str,
        }
        let binding = serde_json::to_vec(&Binding {
            schema_version: 1,
            request,
            program,
            executable_sha256,
            running_slot,
        })?;
        let now = crate::protocol::now_ms();
        let index = records.records.len();
        records.records.push(Receipt {
            schema_version: 1,
            sequence: index as u32 + 1,
            request_id: request.request_id.clone(),
            program: program.into(),
            version: request.version.clone(),
            definition_sha256: hex::encode(Sha256::digest(binding)),
            artifact_sha256: request.sha256.to_ascii_lowercase(),
            artifact_bytes: request.size_bytes,
            admitted_at_ms: now,
            updated_at_ms: now,
            step: Step::Admitted,
            outcome: Outcome::InProgress,
        });
        // Acquire before admission publication: a visible in-progress record
        // cannot briefly look interrupted between admission and lease creation.
        // Crash-created pre-admission lease files are counted in the same quota.
        std::fs::create_dir_all(install.join(".package-repair-leases"))?;
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lease_path(install, &request.request_id))?;
        lease
            .try_lock_exclusive()
            .context("repair operation lease is busy")?;
        let expected_sha256 = guarded_write(install, &records, &expected)
            .context("persist repair admission before artifact work")?;
        Ok(Self {
            install: install.into(),
            records,
            index,
            expected_sha256,
            _lease: lease,
        })
    }

    fn persist(&mut self) -> Result<()> {
        self.expected_sha256 = guarded_write(&self.install, &self.records, &self.expected_sha256)?;
        Ok(())
    }

    pub(crate) fn step(&mut self, step: Step) -> Result<()> {
        let record = &mut self.records.records[self.index];
        if record.outcome != Outcome::InProgress || step.ordinal() != record.step.ordinal() + 1 {
            bail!("invalid repair receipt transition")
        }
        let previous = record.clone();
        record.step = step;
        record.updated_at_ms = crate::protocol::now_ms().max(record.updated_at_ms);
        if step == Step::Selected {
            record.outcome = Outcome::Selected;
        }
        if let Err(error) = self.persist() {
            self.records.records[self.index] = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Best effort only: if persistence fails the last durable step remains
    /// unresolved after the lease closes. Do not overwrite corruption or reset.
    pub(crate) fn failed(&mut self) -> Result<()> {
        let record = &mut self.records.records[self.index];
        if record.outcome != Outcome::InProgress {
            bail!("repair attempt is already terminal")
        }
        record.outcome = Outcome::Failed;
        record.updated_at_ms = crate::protocol::now_ms().max(record.updated_at_ms);
        self.persist()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Lease {
    Active,
    Released,
    Unavailable,
}

fn lease(install: &Path, id: &str) -> Lease {
    let file = match File::open(lease_path(install, id)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Lease::Released,
        Err(_) => return Lease::Unavailable,
    };
    match file.try_lock_exclusive() {
        Ok(()) => Lease::Released,
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Lease::Active
        }
        Err(_) => Lease::Unavailable,
    }
}

pub(crate) fn status(install: &Path, program: &str, id: Option<&str>) -> Result<Value> {
    validate_program(program)?;
    if let Some(id) = id {
        validate_id(id)?;
    }
    let records = read(install)?;
    let leases = leases(install)?;
    let orphan_leases: Vec<_> = leases
        .iter()
        .filter(|id| !records.records.iter().any(|r| &r.request_id == *id))
        .collect();
    let mut observations = Vec::new();
    for record in records
        .records
        .iter()
        .filter(|r| r.program == program && id.is_none_or(|id| r.request_id == id))
    {
        let lease = lease(install, &record.request_id);
        let interrupted = record.outcome == Outcome::InProgress && matches!(lease, Lease::Released);
        let unresolved = record.outcome == Outcome::InProgress && !matches!(lease, Lease::Active);
        let slot = crate::bundle_selection::repair_slot(&record.version, &record.request_id)?;
        let bundle = crate::bundle_selection::directory(install, &record.version, &slot)?;
        let present = bundle.try_exists().ok();
        let selected = (|| -> Result<bool> {
            Ok(crate::updater::installed_version(install)?.as_deref()
                == Some(record.version.as_str())
                && crate::bundle_selection::read(install, &record.version)?
                    .is_some_and(|s| s.slot == slot))
        })()
        .ok();
        observations.push(
            json!({"receipt":record,"lease":lease,"interrupted":interrupted,
            "outcome_unresolved":unresolved,
            "package_may_be_staged":record.step.ordinal()>=Step::Staging.ordinal(),
            "selection_outcome_unknown":record.step==Step::Selecting,
            "bundle_present":present,"selected_now":selected,
            "automatically_retried":false,"runtime_health_checked":false}),
        );
    }
    Ok(
        json!({"schema_version":1,"available":true,"record_count":records.records.len(),
        "max_records":MAX_RECORDS,"max_bytes":MAX_BYTES,"operation_id":id,"attempts":observations,
        "lease_slots_used":leases.len(),"unadmitted_lease_ids":orphan_leases,
        "note":"Read-only retained evidence. Selected is not running or healthy. Released in-progress leases are interrupted, never resumed. No receipt is deleted or rewritten by inspection."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn request() -> crate::package_repair::RepairRequest {
        crate::package_repair::RepairRequest {
            schema_version: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            version: "0.4.0-alpha.6".into(),
            url: "https://example.invalid/archive?token=PRIVATE-SOURCE".into(),
            sha256: "a".repeat(64),
            size_bytes: 123,
            allow_prerelease: true,
        }
    }
    fn begin(root: &Path, request: &crate::package_repair::RepairRequest) -> Attempt {
        Attempt::begin(
            root,
            "eefn",
            request,
            &"b".repeat(64),
            crate::bundle_selection::ROOT_SLOT,
        )
        .unwrap()
    }

    #[test]
    fn repair_receipts_empty_inspection_and_bad_ids_are_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let report = status(dir.path(), "eefn", None).unwrap();
        assert_eq!(report["record_count"], 0);
        assert_eq!(report["lease_slots_used"], 0);
        assert!(status(dir.path(), "eefn", Some("../escape")).is_err());
        assert!(status(dir.path(), "eefn", Some(&uuid::Uuid::nil().to_string())).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn repair_receipts_live_lease_then_interruption_without_recovery_or_url_disclosure() {
        let dir = tempfile::tempdir().unwrap();
        let request = request();
        let mut attempt = begin(dir.path(), &request);
        attempt.step(Step::Downloading).unwrap();
        let bytes = fs::read(path(dir.path())).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE-SOURCE"));
        let live = status(dir.path(), "eefn", Some(&request.request_id)).unwrap();
        assert_eq!(live["attempts"][0]["lease"], "active");
        assert_eq!(live["attempts"][0]["interrupted"], false);
        assert_eq!(
            status(dir.path(), "eef", None).unwrap()["attempts"],
            json!([])
        );
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
        drop(attempt);
        let interrupted = status(dir.path(), "eefn", Some(&request.request_id)).unwrap();
        assert_eq!(interrupted["attempts"][0]["lease"], "released");
        assert_eq!(interrupted["attempts"][0]["interrupted"], true);
        assert_eq!(interrupted["attempts"][0]["outcome_unresolved"], true);
        assert_eq!(
            interrupted["attempts"][0]["receipt"]["outcome"],
            "in_progress"
        );
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
    }

    #[test]
    fn repair_receipts_steps_ordered_and_selection_is_historical_not_health() {
        let dir = tempfile::tempdir().unwrap();
        let request = request();
        let mut attempt = begin(dir.path(), &request);
        assert!(attempt.step(Step::Selected).is_err());
        for step in [
            Step::Downloading,
            Step::Verifying,
            Step::Staging,
            Step::Staged,
            Step::Selecting,
            Step::Selected,
        ] {
            attempt.step(step).unwrap();
        }
        assert!(attempt.failed().is_err());
        assert!(attempt.step(Step::Selected).is_err());
        drop(attempt);
        let report = status(dir.path(), "eefn", None).unwrap();
        assert_eq!(report["attempts"][0]["receipt"]["outcome"], "selected");
        assert_eq!(report["attempts"][0]["selected_now"], false);
        assert_eq!(report["attempts"][0]["runtime_health_checked"], false);
        assert_eq!(report["attempts"][0]["automatically_retried"], false);
    }

    #[test]
    fn repair_receipts_reused_id_never_retargets_or_replays_a_failed_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = request();
        let mut attempt = begin(dir.path(), &request);
        attempt.failed().unwrap();
        drop(attempt);
        let bytes = fs::read(path(dir.path())).unwrap();
        request.url = "https://other.invalid/a".into();
        assert!(
            Attempt::begin(dir.path(), "eefn", &request, &"c".repeat(64), "$root")
                .err()
                .unwrap()
                .to_string()
                .contains("already admitted")
        );
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
    }

    #[test]
    fn repair_receipts_record_quota_refuses_without_pruning_or_new_lease() {
        let dir = tempfile::tempdir().unwrap();
        let request = request();
        let mut attempt = begin(dir.path(), &request);
        attempt.failed().unwrap();
        drop(attempt);
        let template = read(dir.path()).unwrap().records[0].clone();
        let mut records = Records {
            schema_version: 1,
            records: vec![],
        };
        for index in 0..MAX_RECORDS {
            let mut r = template.clone();
            r.sequence = index as u32 + 1;
            r.request_id = uuid::Uuid::new_v4().to_string();
            records.records.push(r);
        }
        write(dir.path(), &records).unwrap();
        let bytes = fs::read(path(dir.path())).unwrap();
        assert!(
            Attempt::begin(
                dir.path(),
                "eefn",
                &super::tests::request(),
                &"b".repeat(64),
                "$root"
            )
            .err()
            .unwrap()
            .to_string()
            .contains("quota")
        );
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
        assert_eq!(leases(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn repair_receipts_pre_admission_lease_orphans_are_bounded_and_visible() {
        let dir = tempfile::tempdir().unwrap();
        let empty = Records {
            schema_version: 1,
            records: vec![],
        };
        write(dir.path(), &empty).unwrap();
        let original = fs::read(path(dir.path())).unwrap();
        fs::create_dir_all(dir.path().join(".package-repair-leases")).unwrap();
        for _ in 0..MAX_RECORDS {
            File::create(lease_path(dir.path(), &uuid::Uuid::new_v4().to_string())).unwrap();
        }
        let report = status(dir.path(), "eefn", None).unwrap();
        assert_eq!(report["record_count"], 0);
        assert_eq!(
            report["unadmitted_lease_ids"].as_array().unwrap().len(),
            MAX_RECORDS
        );
        assert!(
            Attempt::begin(dir.path(), "eefn", &request(), &"b".repeat(64), "$root")
                .err()
                .unwrap()
                .to_string()
                .contains("quota")
        );
        assert_eq!(fs::read(path(dir.path())).unwrap(), original);
        assert_eq!(leases(dir.path()).unwrap().len(), MAX_RECORDS);
    }

    #[test]
    fn repair_receipts_corruption_future_schema_and_changed_evidence_are_never_reset() {
        for bytes in [
            b"{\"schema_version\":99,\"records\":[]}".to_vec(),
            b"not JSON".to_vec(),
            vec![b'x'; MAX_BYTES + 1],
        ] {
            let dir = tempfile::tempdir().unwrap();
            fs::write(path(dir.path()), &bytes).unwrap();
            assert!(status(dir.path(), "eefn", None).is_err());
            assert!(
                Attempt::begin(dir.path(), "eefn", &request(), &"b".repeat(64), "$root").is_err()
            );
            assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
            assert!(!dir.path().join(".package-repair-leases").exists());
        }
        let dir = tempfile::tempdir().unwrap();
        let mut attempt = begin(dir.path(), &request());
        let changed = b"damaged after admission";
        fs::write(path(dir.path()), changed).unwrap();
        assert!(attempt.step(Step::Downloading).is_err());
        assert!(attempt.failed().is_err());
        drop(attempt);
        assert_eq!(fs::read(path(dir.path())).unwrap(), changed);
        let dir = tempfile::tempdir().unwrap();
        let attempt = begin(dir.path(), &request());
        drop(attempt);
        fs::remove_file(path(dir.path())).unwrap();
        assert!(status(dir.path(), "eefn", None).is_err());
        assert!(Attempt::begin(dir.path(), "eefn", &request(), &"b".repeat(64), "$root").is_err());
        assert!(!path(dir.path()).exists());
        assert_eq!(leases(dir.path()).unwrap().len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn repair_receipts_atomic_write_denial_preserves_uncertain_selection_evidence() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let request = request();
        let mut attempt = begin(dir.path(), &request);
        for step in [
            Step::Downloading,
            Step::Verifying,
            Step::Staging,
            Step::Staged,
            Step::Selecting,
        ] {
            attempt.step(step).unwrap();
        }
        let slot =
            crate::bundle_selection::repair_slot(&request.version, &request.request_id).unwrap();
        let bundle =
            crate::bundle_selection::directory(dir.path(), &request.version, &slot).unwrap();
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("eefn.exe"), b"never executed").unwrap();
        crate::bundle_selection::write(
            dir.path(),
            &crate::bundle_selection::BundleSelection {
                schema_version: 1,
                version: request.version.clone(),
                slot,
                previous_slot: Some("$root".into()),
            },
        )
        .unwrap();
        fs::write(dir.path().join("current.txt"), &request.version).unwrap();
        let bytes = fs::read(path(dir.path())).unwrap();
        let locked = File::options()
            .read(true)
            .share_mode(3)
            .open(path(dir.path()))
            .unwrap();
        assert!(attempt.step(Step::Selected).is_err());
        assert!(attempt.failed().is_err());
        drop(attempt);
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
        let status = status(dir.path(), "eefn", Some(&request.request_id)).unwrap();
        assert_eq!(status["attempts"][0]["selection_outcome_unknown"], true);
        assert_eq!(status["attempts"][0]["selected_now"], true);
        assert_eq!(status["attempts"][0]["interrupted"], true);
        drop(locked);
        assert!(Attempt::begin(dir.path(), "eefn", &request, &"b".repeat(64), "$root").is_err());
        assert_eq!(fs::read(path(dir.path())).unwrap(), bytes);
    }
}
