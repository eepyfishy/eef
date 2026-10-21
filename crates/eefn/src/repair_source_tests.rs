use super::*;
use clap::Parser;
use std::fs;

const SOURCE: &str = "https://publisher.invalid/current.json?private-source-token";
const ARTIFACT: &str = "https://publisher.invalid/exact.exe?private-artifact-token";

#[derive(Parser)]
struct Owner {
    #[command(flatten)]
    args: package_repair::RepairArgs,
}

fn args(values: &[&str]) -> package_repair::RepairArgs {
    Owner::try_parse_from(std::iter::once("repair").chain(values.iter().copied()))
        .unwrap()
        .args
}

fn fixture(role: &str) -> (tempfile::TempDir, PathBuf, Vec<u8>) {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join(format!("{role}.exe"));
    fs::write(&executable, b"exact running build").unwrap();
    let manifest = json!({"schema_version":1,"program":role,"version":crate::VERSION,
        "executable_sha256":updater::sha256_file(&executable).unwrap(),
        "artifact":{"url":ARTIFACT,"sha256":"a".repeat(64),"size_bytes":123}});
    (root, executable, serde_json::to_vec(&manifest).unwrap())
}

fn plan(root: &Path, executable: &Path, role: &str, bytes: &[u8]) -> Plan {
    let report = propose(root, executable, role, crate::VERSION, SOURCE, bytes, true).unwrap();
    assert_eq!(report["repair_admitted"], false);
    assert_eq!(report["artifact_download_started"], false);
    serde_json::from_value(report["plan"].clone()).unwrap()
}

#[test]
fn repair_source_cli_requires_check_or_exact_approval_without_raw_artifact_options() {
    assert!(args(&["--check", "--manifest", SOURCE]).check);
    let id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        args(&["--approve", &id, "--manifest", SOURCE])
            .approve
            .as_deref(),
        Some(id.as_str())
    );
    for input in [
        vec!["--manifest", SOURCE],
        vec!["--check"],
        vec!["--approve", &id],
        vec!["--check", "--approve", &id, "--manifest", SOURCE],
        vec!["--status", "--manifest", SOURCE],
        vec!["--rollback", "--check", "--manifest", SOURCE],
        vec!["--check", "--manifest", SOURCE, "--url", ARTIFACT],
        vec!["--check", "--manifest", SOURCE, "--sha256", &"a".repeat(64)],
        vec!["--check", "--manifest", SOURCE, "--size-bytes", "123"],
        vec!["--check", "--manifest", SOURCE, "--operation-id", &id],
        vec![
            "--approve",
            &id,
            "--manifest",
            SOURCE,
            "--operation-id",
            &id,
        ],
    ] {
        assert!(Owner::try_parse_from(std::iter::once("repair").chain(input)).is_err());
    }
}

#[test]
fn repair_source_proposal_is_bounded_metadata_not_download_admission_or_secret_retention() {
    for role in ["eef", "eefn"] {
        let (root, executable, bytes) = fixture(role);
        fs::write(root.path().join("config.json"), b"owner settings sentinel").unwrap();
        let first = plan(root.path(), &executable, role, &bytes);
        let retained = fs::read(store_path(root.path())).unwrap();
        let text = String::from_utf8(retained.clone()).unwrap();
        assert!(
            !text.contains("https://")
                && !text.contains("private-")
                && !text.contains("publisher.invalid")
        );
        assert!(!root.path().join("versions").exists());
        assert!(!root.path().join(".package-repairs.json").exists());
        let request = resolve(
            root.path(),
            &executable,
            role,
            crate::VERSION,
            &first,
            SOURCE,
            &bytes,
            true,
        )
        .unwrap();
        assert_eq!(request.request_id, first.id);
        assert_eq!(request.url, ARTIFACT);
        assert_eq!(request.size_bytes, 123);
        assert_eq!(
            fs::read(store_path(root.path())).unwrap(),
            retained,
            "resolution is read-only"
        );
        let second = plan(root.path(), &executable, role, &bytes);
        assert_ne!(first.id, second.id);
        assert_eq!(find(root.path(), &first.id).unwrap(), first);
        assert_eq!(
            fs::read(root.path().join("config.json")).unwrap(),
            b"owner settings sentinel"
        );
        let report = json!({"report_type":"repair_source_plan","plan":first});
        let human = package_repair::render(&report).unwrap();
        assert!(human.contains("--approve") && human.contains("No package download"));
        assert!(!human.contains("private-") && !human.contains("publisher.invalid"));
    }
}

#[test]
fn repair_source_descriptor_rejects_wrong_build_product_schema_and_unsafe_artifacts() {
    let (root, executable, bytes) = fixture("eefn");
    let base: Value = serde_json::from_slice(&bytes).unwrap();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("program", json!("eef")),
        ("version", json!("99.0.0")),
        ("executable_sha256", json!("b".repeat(64))),
        ("unknown", json!(true)),
    ] {
        let mut changed = base.clone();
        changed[field] = value;
        assert!(
            propose(
                root.path(),
                &executable,
                "eefn",
                crate::VERSION,
                SOURCE,
                &serde_json::to_vec(&changed).unwrap(),
                true
            )
            .is_err()
        );
    }
    for (field, value) in [
        ("url", json!("http://publisher.invalid/file")),
        ("url", json!("https://user:secret@publisher.invalid/file")),
        ("url", json!("https://publisher.invalid/file#part")),
        ("url", json!("file:///not-a-source")),
        ("sha256", json!("bad")),
        ("size_bytes", json!(0)),
        ("size_bytes", json!(512_u64 * 1024 * 1024 + 1)),
        ("unknown", json!(false)),
    ] {
        let mut changed = base.clone();
        changed["artifact"][field] = value;
        assert!(
            propose(
                root.path(),
                &executable,
                "eefn",
                crate::VERSION,
                SOURCE,
                &serde_json::to_vec(&changed).unwrap(),
                true
            )
            .is_err()
        );
    }
    assert!(
        propose(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            SOURCE,
            &vec![b' '; MAX_MANIFEST + 1],
            true
        )
        .is_err()
    );
    assert!(
        propose(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            "http://publisher.invalid",
            &bytes,
            true
        )
        .is_err()
    );
    if crate::VERSION.contains('-') {
        assert!(
            propose(
                root.path(),
                &executable,
                "eefn",
                crate::VERSION,
                SOURCE,
                &bytes,
                false
            )
            .is_err()
        );
    }
    assert!(!store_path(root.path()).exists());
}

#[test]
fn repair_source_approval_binds_raw_source_bytes_consent_product_and_build() {
    let (root, executable, bytes) = fixture("eefn");
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    let mut whitespace = bytes.clone();
    whitespace.push(b' ');
    for (url, body, role, consent) in [
        ("https://other.invalid/exact.json", &bytes, "eefn", true),
        (SOURCE, &whitespace, "eefn", true),
        (SOURCE, &bytes, "eef", true),
        (SOURCE, &bytes, "eefn", false),
    ] {
        assert!(
            resolve(
                root.path(),
                &executable,
                role,
                crate::VERSION,
                &saved,
                url,
                body,
                consent
            )
            .is_err()
        );
    }
    fs::write(&executable, b"replacement build").unwrap();
    assert!(
        resolve(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            &saved,
            SOURCE,
            &bytes,
            true
        )
        .is_err()
    );
    assert_eq!(find(root.path(), &saved.id).unwrap(), saved);
    assert!(!root.path().join(".package-repairs.json").exists());
}

#[test]
fn repair_source_proposals_refuse_expiry_clock_rollback_mutation_and_pending_packages() {
    let (root, executable, bytes) = fixture("eefn");
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    for created in [
        crate::protocol::now_ms() - LIFETIME_MS - 1,
        crate::protocol::now_ms() + 60_000,
    ] {
        let mut changed = saved.clone();
        changed.created_at_ms = created;
        changed.expires_at_ms = created + LIFETIME_MS;
        let store = Plans {
            schema_version: 1,
            plans: vec![changed.clone()],
        };
        fs::write(store_path(root.path()), serde_json::to_vec(&store).unwrap()).unwrap();
        assert!(
            validate_plan(
                root.path(),
                &executable,
                "eefn",
                crate::VERSION,
                &changed,
                true
            )
            .is_err()
        );
    }
    let mut changed = saved.clone();
    changed.artifact_bytes += 1;
    fs::write(
        store_path(root.path()),
        serde_json::to_vec(&Plans {
            schema_version: 1,
            plans: vec![changed],
        })
        .unwrap(),
    )
    .unwrap();
    assert!(
        validate_plan(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            &saved,
            true
        )
        .is_err()
    );
    fs::write(
        store_path(root.path()),
        serde_json::to_vec(&Plans {
            schema_version: 1,
            plans: vec![saved.clone()],
        })
        .unwrap(),
    )
    .unwrap();
    let slot =
        bundle_selection::repair_slot(crate::VERSION, &uuid::Uuid::new_v4().to_string()).unwrap();
    let selected = root.path().join("versions").join(&slot);
    fs::create_dir_all(&selected).unwrap();
    fs::write(
        selected.join("eefn.exe"),
        b"same executable different physical generation",
    )
    .unwrap();
    fs::write(root.path().join("current.txt"), crate::VERSION).unwrap();
    bundle_selection::write(
        root.path(),
        &bundle_selection::BundleSelection {
            schema_version: 1,
            version: crate::VERSION.into(),
            slot,
            previous_slot: Some("$root".into()),
        },
    )
    .unwrap();
    assert!(
        validate_plan(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            &saved,
            true
        )
        .is_err()
    );
}

#[test]
fn repair_source_store_damage_and_capacity_do_not_reset_history() {
    let (root, executable, bytes) = fixture("eefn");
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    for damaged in [
        b"not JSON".to_vec(),
        br#"{"schema_version":99,"plans":[]}"#.to_vec(),
        vec![b' '; MAX_STORE + 1],
    ] {
        fs::write(store_path(root.path()), &damaged).unwrap();
        assert!(read_store(root.path()).is_err());
        assert!(retain(root.path(), &saved).is_err());
        assert_eq!(fs::read(store_path(root.path())).unwrap(), damaged);
    }
    let plans: Vec<_> = (0..MAX_PLANS)
        .map(|_| {
            let mut p = saved.clone();
            p.id = uuid::Uuid::new_v4().to_string();
            p
        })
        .collect();
    let full = serde_json::to_vec(&Plans {
        schema_version: 1,
        plans,
    })
    .unwrap();
    fs::write(store_path(root.path()), &full).unwrap();
    assert!(retain(root.path(), &saved).is_err());
    assert_eq!(fs::read(store_path(root.path())).unwrap(), full);
    fs::remove_file(store_path(root.path())).unwrap();
    assert!(
        find(root.path(), &saved.id).is_err(),
        "a missing proposal cannot be approved"
    );
}

#[test]
fn repair_source_retention_honors_shared_update_lock_without_overwrite() {
    let (root, executable, bytes) = fixture("eefn");
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    let before = fs::read(store_path(root.path())).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.path().join(".update.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let mut next = saved.clone();
    next.id = uuid::Uuid::new_v4().to_string();
    assert!(retain(root.path(), &next).is_err());
    assert_eq!(fs::read(store_path(root.path())).unwrap(), before);
}

#[test]
fn repair_source_status_is_read_only_filtered_and_independent_of_damaged_receipts() {
    let (root, executable, bytes) = fixture("eefn");
    let empty = status(root.path(), "eefn", None);
    assert_eq!(empty["stored_count"], 0);
    assert!(!store_path(root.path()).exists());
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    let retained = fs::read(store_path(root.path())).unwrap();
    let inspected = status(root.path(), "eefn", Some(&saved.id));
    assert_eq!(inspected["plans"][0]["id"], saved.id);
    assert_eq!(inspected["approval_authorized"], false);
    assert_eq!(inspected["plans"][0]["within_approval_time_window"], true);
    assert_eq!(status(root.path(), "eef", None)["plans"], json!([]));
    assert_eq!(
        status(root.path(), "eefn", Some(&uuid::Uuid::new_v4().to_string()))["plans"],
        json!([])
    );
    fs::write(
        root.path().join(".package-repairs.json"),
        b"damaged receipt evidence",
    )
    .unwrap();
    let partial = package_repair::inspect(
        root.path(),
        crate::VERSION,
        "eefn",
        &executable,
        Some(&saved.id),
    )
    .unwrap();
    assert_eq!(partial["success"], false);
    assert_eq!(partial["repair_source_plans"]["available"], true);
    assert_eq!(partial["repair_receipts"]["available"], false);
    assert!(
        package_repair::render(&partial)
            .unwrap()
            .contains(&saved.id)
    );
    assert_eq!(fs::read(store_path(root.path())).unwrap(), retained);
    fs::write(store_path(root.path()), b"future/broken proposals").unwrap();
    assert_eq!(status(root.path(), "eefn", None)["available"], false);
    assert_eq!(
        fs::read(store_path(root.path())).unwrap(),
        b"future/broken proposals"
    );
}

#[tokio::test]
async fn repair_source_admitted_plan_cannot_refetch_manifest_or_replay_artifact() {
    let (root, executable, mut bytes) = fixture("eefn");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}/descriptor", listener.local_addr().unwrap());
    let report = propose(
        root.path(),
        &executable,
        "eefn",
        crate::VERSION,
        &url,
        &bytes,
        true,
    )
    .unwrap();
    let saved: Plan = serde_json::from_value(report["plan"].clone()).unwrap();
    let request = resolve(
        root.path(),
        &executable,
        "eefn",
        crate::VERSION,
        &saved,
        &url,
        &bytes,
        true,
    )
    .unwrap();
    let mut attempt = crate::repair_receipts::Attempt::begin(
        root.path(),
        "eefn",
        &request,
        &updater::sha256_file(&executable).unwrap(),
        "$root",
    )
    .unwrap();
    attempt.failed().unwrap();
    drop(attempt);
    let retained = fs::read(root.path().join(".package-repairs.json")).unwrap();
    let approval = args(&[
        "--approve",
        &saved.id,
        "--manifest",
        &url,
        "--allow-prerelease",
    ]);
    let refused = execute(&approval, root.path(), &executable, "eefn")
        .await
        .unwrap();
    assert_eq!(refused["success"], false);
    assert_eq!(refused["artifact_download_started_by_this_call"], false);
    assert_eq!(refused["new_repair_admission_started"], false);
    assert_eq!(refused["request_id"], saved.id);
    assert!(
        refused.get("repair_admitted").is_none(),
        "a refused call must not deny prior admission"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(root.path().join(".package-repairs.json")).unwrap(),
        retained
    );
    bytes.push(b' ');
    assert!(
        resolve(
            root.path(),
            &executable,
            "eefn",
            crate::VERSION,
            &saved,
            &url,
            &bytes,
            true
        )
        .is_err()
    );
}

#[tokio::test]
async fn repair_source_public_errors_do_not_echo_private_urls_or_create_proposals() {
    let (root, executable, _) = fixture("eefn");
    let request = args(&[
        "--check",
        "--manifest",
        "http://user:private-token@127.0.0.1/secret",
    ]);
    let report = execute(&request, root.path(), &executable, "eefn")
        .await
        .unwrap();
    let public = serde_json::to_string(&report).unwrap();
    assert_eq!(report["success"], false);
    assert!(!public.contains("private-token") && !public.contains("127.0.0.1"));
    assert!(!store_path(root.path()).exists());
}

#[tokio::test]
async fn repair_source_backend_check_and_approval_reject_changed_metadata_without_admission() {
    let (root, executable, bytes) = fixture("eefn");
    let check = args(&["--check", "--manifest", SOURCE, "--allow-prerelease"]);
    // The private transport seam supplies descriptor bytes only. It does not
    // replace request validation, proposal storage, source pinning or artifact I/O.
    let descriptor = bytes.clone();
    let checked = execute_with_fetch(&check, root.path(), &executable, "eefn", |url| async move {
        assert_eq!(url, SOURCE);
        Ok(descriptor)
    })
    .await
    .unwrap();
    assert_eq!(checked["success"], true);
    let saved: Plan = serde_json::from_value(checked["plan"].clone()).unwrap();
    let retained = fs::read(store_path(root.path())).unwrap();
    let approval = args(&[
        "--approve",
        &saved.id,
        "--manifest",
        SOURCE,
        "--allow-prerelease",
    ]);
    let mut changed = bytes;
    changed.push(b' ');
    let refused = execute_with_fetch(&approval, root.path(), &executable, "eefn", |_| async {
        Ok(changed)
    })
    .await
    .unwrap();
    assert_eq!(refused["error_code"], "repair_source_proposal_changed");
    assert!(!root.path().join(".package-repairs.json").exists());
    assert_eq!(fs::read(store_path(root.path())).unwrap(), retained);
    let other = args(&[
        "--approve",
        &saved.id,
        "--manifest",
        "https://other.invalid/source",
        "--allow-prerelease",
    ]);
    let refused = execute_with_fetch(&other, root.path(), &executable, "eefn", |_| async {
        panic!("a mismatched source must be refused before descriptor fetch")
    })
    .await
    .unwrap();
    assert_eq!(refused["error_code"], "repair_source_url_changed");
}

#[tokio::test]
async fn repair_source_backend_approval_uses_original_operation_and_existing_repair_receipts() {
    let (root, executable, bytes) = fixture("eefn");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let mut descriptor: Value = serde_json::from_slice(&bytes).unwrap();
    descriptor["artifact"]["url"] = json!(format!("https://{closed}/synthetic-no-artifact"));
    let bytes = serde_json::to_vec(&descriptor).unwrap();
    let saved = plan(root.path(), &executable, "eefn", &bytes);
    let approval = args(&[
        "--approve",
        &saved.id,
        "--manifest",
        SOURCE,
        "--allow-prerelease",
    ]);
    let applied = execute_with_fetch(&approval, root.path(), &executable, "eefn", |_| async {
        Ok(bytes.clone())
    })
    .await
    .unwrap();
    assert_eq!(applied["report_type"], "explicit_repair");
    assert_eq!(applied["request_id"], saved.id);
    assert_eq!(applied["success"], false);
    if cfg!(windows) {
        assert_eq!(applied["attempt"]["receipt"]["step"], "downloading");
        assert_eq!(applied["attempt"]["receipt"]["outcome"], "failed");
        let replay = execute_with_fetch(&approval, root.path(), &executable, "eefn", |_| async {
            panic!("admitted approval must not fetch the descriptor or retry the artifact")
        })
        .await
        .unwrap();
        assert_eq!(
            replay["error_code"],
            "repair_source_already_admitted_or_evidence_unavailable"
        );
    }
    assert_eq!(find(root.path(), &saved.id).unwrap(), saved);
}
